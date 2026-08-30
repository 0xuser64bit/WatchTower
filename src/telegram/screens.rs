//! Screen renderers.
//!
//! A "screen" is one focused view — the main menu, a list, a detail page, a
//! confirmation — built as HTML text plus an inline keyboard and painted onto a
//! [`Surface`]. Commands open screens as new messages; callbacks edit the message the
//! button lived on. Because every screen is a pure function of the database, a command
//! and a tap that lead to the same place produce the same view.
//!
//! Nothing here decides *who* may act: authorization happens before a renderer is
//! called. Admin screens still re-check role-sensitive invariants (the last-admin
//! guard) against the database at the moment of the mutation.

use crate::alerts::format;
use crate::app_state::AppState;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::users::{Role, User, UserRepo};
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::rules::types::{abbreviate, Operator, Rule, RuleState, TargetKind};
use crate::telegram::ui::{self, back_menu, button, code, esc, menu_row, Screen, Surface};
use crate::telegram::{copy, menu};
use teloxide::types::InlineKeyboardButton;

/// Items per list page. Small enough that a list plus its navigation never scrolls far
/// on a phone.
const PAGE_SIZE: usize = 6;

// ── Shared value formatting ───────────────────────────────────────────────────────

/// A short, human name for a rule's target: its label if it has one, otherwise a
/// shortened address.
fn target_short(rule: &Rule) -> String {
    match &rule.target.label {
        Some(label) => label.clone(),
        None => abbreviate(&rule.target.reference),
    }
}

/// A value with its unit, e.g. `$0.0000025` or `2 SOL`.
pub(crate) fn valued(kind: TargetKind, value: f64) -> String {
    match kind {
        TargetKind::Token => format!("${}", format::amount(value)),
        TargetKind::Wallet => format!("{} SOL", format::amount(value)),
    }
}

/// A plain-language rendering of a rule's condition, e.g. `below $0.99` or `up 10%`.
fn condition_phrase(rule: &Rule) -> String {
    condition_from(rule.target.kind, rule.operator, rule.threshold)
}

/// The condition phrase from raw parts, for use before a [`Rule`] exists (mid-flow).
pub(crate) fn condition_from(kind: TargetKind, operator: Operator, threshold: f64) -> String {
    match operator {
        Operator::Gt => format!("above {}", valued(kind, threshold)),
        Operator::Lt => format!("below {}", valued(kind, threshold)),
        Operator::Gte => format!("at or above {}", valued(kind, threshold)),
        Operator::Lte => format!("at or below {}", valued(kind, threshold)),
        Operator::PctUp => format!("up {}%", format::amount(threshold)),
        Operator::PctDown => format!("down {}%", format::amount(threshold)),
    }
}

/// 🟢 armed · 🔴 firing · ⚪ disabled — the at-a-glance state of a rule.
fn state_dot(rule: &Rule) -> &'static str {
    match (rule.enabled, rule.state) {
        (false, _) => "⚪",
        (true, RuleState::Firing) => "🔴",
        (true, RuleState::Ok) => "🟢",
    }
}

fn state_word(rule: &Rule) -> &'static str {
    match (rule.enabled, rule.state) {
        (false, _) => "disabled",
        (true, RuleState::Firing) => "firing",
        (true, RuleState::Ok) => "armed",
    }
}

/// Builds a previous/next row for a paged list, or nothing when a single page suffices.
///
/// The middle button is an inert page indicator: Telegram requires a callback on every
/// button, so it carries the no-op marker and is simply acknowledged.
fn pager(domain: &str, page: usize, total: usize) -> Option<Vec<InlineKeyboardButton>> {
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    if pages <= 1 {
        return None;
    }

    let mut row = Vec::new();
    if page > 0 {
        row.push(button("◀", format!("{domain}:p:{}", page - 1)));
    }
    row.push(button(
        format!("{} / {}", page + 1, pages),
        crate::telegram::callback::NOOP,
    ));
    if page + 1 < pages {
        row.push(button("▶", format!("{domain}:p:{}", page + 1)));
    }
    Some(row)
}

/// Clamps a requested page to the available range and returns the visible slice.
fn paginate<T>(items: &[T], page: usize) -> (usize, &[T]) {
    if items.is_empty() {
        return (0, &[]);
    }
    let pages = items.len().div_ceil(PAGE_SIZE);
    let page = page.min(pages - 1);
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(items.len());
    (page, &items[start..end])
}

/// A minimal "nothing to see here" screen with a single route back.
fn notice(text: impl Into<String>, back: &str) -> Screen {
    Screen::new(text, vec![back_menu(back)])
}

// ── Main menu & help ────────────────────────────────────────────────────────────────

pub async fn show_main(state: &AppState, surface: Surface, is_admin: bool) -> Result<()> {
    let tokens = TokenRepo::new(&state.db).count().await?;
    let wallets = WalletRepo::new(&state.db).count().await?;
    let rules = RuleRepo::new(&state.db).count_all().await?;

    let mut rows = vec![
        vec![button("🚨 Alerts", "al"), button("🪙 Tokens", "tk")],
        vec![button("👛 Wallets", "wl"), button("📜 History", "hi")],
        vec![button("⚙️ Status", "st"), button("❔ Help", "hl")],
    ];
    if is_admin {
        rows.push(vec![button("🛡 Admin", "ad")]);
    }

    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::main_menu(tokens, wallets, rules), rows),
    )
    .await
}

pub async fn show_help(state: &AppState, surface: Surface) -> Result<()> {
    let rows = vec![
        vec![button("🚨 Create Alert", "ac:new")],
        vec![
            button("⭐ Popular Tokens", "at:pop"),
            button("👛 Add Wallet", "aw:new"),
        ],
        menu_row(),
    ];
    ui::render(&state.bot, surface, Screen::new(copy::HELP, rows)).await
}

// ── Alerts ──────────────────────────────────────────────────────────────────────────

pub async fn show_alerts(state: &AppState, surface: Surface, page: usize) -> Result<()> {
    let rules = RuleRepo::new(&state.db).list_all().await?;

    if rules.is_empty() {
        let rows = vec![vec![button("➕ Create Alert", "ac:new")], menu_row()];
        return ui::render(&state.bot, surface, Screen::new(copy::EMPTY_ALERTS, rows)).await;
    }

    let (page, visible) = paginate(&rules, page);

    let mut rows: Vec<Vec<InlineKeyboardButton>> = visible
        .iter()
        .map(|rule| {
            vec![button(
                format!(
                    "{} {} · {}",
                    state_dot(rule),
                    target_short(rule),
                    condition_phrase(rule)
                ),
                format!("al:v:{}", rule.id),
            )]
        })
        .collect();

    if let Some(row) = pager("al", page, rules.len()) {
        rows.push(row);
    }
    rows.push(vec![button("➕ Create Alert", "ac:new")]);
    rows.push(menu_row());

    let text = format!(
        "<b>🚨 Your Alerts</b>  ({})\n\n🟢 armed · 🔴 firing · ⚪ disabled\nTap one to manage it.",
        rules.len()
    );
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn show_alert(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(rule) = RuleRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::ALERT_GONE, "al")).await;
    };

    let text = alert_detail_text(&rule, state.settings.poll_interval.as_secs());

    let toggle = if rule.enabled {
        button("⏸ Disable", format!("al:t:{id}"))
    } else {
        button("▶️ Enable", format!("al:t:{id}"))
    };

    let rows = vec![
        vec![toggle],
        vec![button("🗑 Delete", format!("al:d:{id}"))],
        back_menu("al"),
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

fn alert_detail_text(rule: &Rule, poll_seconds: u64) -> String {
    let kind_icon = match rule.target.kind {
        TargetKind::Token => "🪙",
        TargetKind::Wallet => "👛",
    };

    let mut lines = vec![
        format!("{kind_icon} <b>{}</b>", esc(&target_short(rule))),
        String::new(),
        format!("<b>Condition:</b> {}", esc(&condition_phrase(rule))),
        format!("<b>Status:</b> {} {}", state_dot(rule), state_word(rule)),
    ];

    if let Some(last) = rule.last_value {
        lines.push(format!(
            "<b>Last reading:</b> {}",
            esc(&valued(rule.target.kind, last))
        ));
    }

    if rule.operator.is_percentage() {
        match rule.reference_value {
            Some(baseline) => lines.push(format!(
                "<b>Baseline:</b> {}",
                esc(&valued(rule.target.kind, baseline))
            )),
            None => lines.push("<b>Baseline:</b> set on next reading".to_string()),
        }
    }

    if let Some(at) = rule.last_triggered_at {
        lines.push(format!(
            "<b>Last fired:</b> {}",
            esc(&format::timestamp(at))
        ));
    }

    lines.push(String::new());
    lines.push(format!(
        "Cooldown {}s · checked every {}s",
        rule.cooldown_seconds, poll_seconds
    ));

    lines.join("\n")
}

pub async fn toggle_alert(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let repo = RuleRepo::new(&state.db);
    let Some(rule) = repo.find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::ALERT_GONE, "al")).await;
    };

    repo.set_enabled(id, !rule.enabled).await?;
    // Re-render the detail so the flipped button and refreshed state are visible.
    show_alert(state, surface, id).await
}

pub async fn confirm_delete_alert(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(rule) = RuleRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::ALERT_GONE, "al")).await;
    };

    let text = format!(
        "<b>Delete this alert?</b>\n\n{} <b>{}</b>\n{}\n\nHistory of past firings is kept.",
        state_dot(&rule),
        esc(&target_short(&rule)),
        esc(&condition_phrase(&rule))
    );
    let rows = vec![
        vec![button("🗑 Yes, delete", format!("al:dy:{id}"))],
        vec![button("← Keep it", format!("al:v:{id}"))],
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn delete_alert(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    // Tolerate a double-tap: if it is already gone, just return to the list.
    if let Err(err) = RuleRepo::new(&state.db).delete(id).await {
        if !matches!(err, crate::error::AppError::NotFound(_)) {
            return Err(err);
        }
    }
    show_alerts(state, surface, 0).await
}

// ── Tokens ────────────────────────────────────────────────────────────────────────

pub async fn show_tokens(state: &AppState, surface: Surface, page: usize) -> Result<()> {
    let tokens = TokenRepo::new(&state.db).list().await?;

    if tokens.is_empty() {
        let rows = vec![
            vec![button("⭐ Popular", "at:pop")],
            vec![button("➕ Add Token", "at:new")],
            menu_row(),
        ];
        return ui::render(&state.bot, surface, Screen::new(copy::EMPTY_TOKENS, rows)).await;
    }

    let (page, visible) = paginate(&tokens, page);
    let mut rows: Vec<Vec<InlineKeyboardButton>> = visible
        .iter()
        .map(|token| {
            let name = token
                .symbol
                .clone()
                .unwrap_or_else(|| "unnamed".to_string());
            vec![button(
                format!("🪙 {} · {}", name, abbreviate(&token.mint_address)),
                format!("tk:v:{}", token.id),
            )]
        })
        .collect();

    if let Some(row) = pager("tk", page, tokens.len()) {
        rows.push(row);
    }
    rows.push(vec![
        button("⭐ Popular", "at:pop"),
        button("➕ Add Token", "at:new"),
    ]);
    rows.push(menu_row());

    let text = format!(
        "<b>🪙 Tracked Tokens</b>  ({})\n\nTap one to see details or remove it.",
        tokens.len()
    );
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn show_token(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(token) = TokenRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::TOKEN_GONE, "tk")).await;
    };
    // Rule count comes from the list projection.
    let rule_count = TokenRepo::new(&state.db)
        .list()
        .await?
        .into_iter()
        .find(|t| t.id == id)
        .map(|t| t.rule_count)
        .unwrap_or(0);

    let text = format!(
        "🪙 <b>{}</b>\n\n<b>Mint:</b>\n{}\n\n<b>Alerts:</b> {}",
        esc(token.symbol.as_deref().unwrap_or("unnamed")),
        code(&token.mint_address),
        rule_count
    );
    let rows = vec![
        vec![button("🗑 Remove", format!("tk:d:{id}"))],
        back_menu("tk"),
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn confirm_delete_token(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(token) = TokenRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::TOKEN_GONE, "tk")).await;
    };
    let rule_count = TokenRepo::new(&state.db)
        .list()
        .await?
        .into_iter()
        .find(|t| t.id == id)
        .map(|t| t.rule_count)
        .unwrap_or(0);

    let text = format!(
        "<b>Stop tracking this token?</b>\n\n🪙 <b>{}</b>\n{}{}",
        esc(token.symbol.as_deref().unwrap_or("unnamed")),
        code(&token.mint_address),
        cascade_warning(rule_count)
    );
    let rows = vec![
        vec![button("🗑 Yes, remove", format!("tk:dy:{id}"))],
        vec![button("← Keep it", format!("tk:v:{id}"))],
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn delete_token(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    if let Err(err) = TokenRepo::new(&state.db).delete(id).await {
        if !matches!(err, crate::error::AppError::NotFound(_)) {
            return Err(err);
        }
    }
    show_tokens(state, surface, 0).await
}

// ── Wallets ─────────────────────────────────────────────────────────────────────────

pub async fn show_wallets(state: &AppState, surface: Surface, page: usize) -> Result<()> {
    let wallets = WalletRepo::new(&state.db).list().await?;

    if wallets.is_empty() {
        let rows = vec![vec![button("➕ Add Wallet", "aw:new")], menu_row()];
        return ui::render(&state.bot, surface, Screen::new(copy::EMPTY_WALLETS, rows)).await;
    }

    let (page, visible) = paginate(&wallets, page);
    let mut rows: Vec<Vec<InlineKeyboardButton>> = visible
        .iter()
        .map(|wallet| {
            let name = wallet
                .label
                .clone()
                .unwrap_or_else(|| "unnamed".to_string());
            vec![button(
                format!("👛 {} · {}", name, abbreviate(&wallet.address)),
                format!("wl:v:{}", wallet.id),
            )]
        })
        .collect();

    if let Some(row) = pager("wl", page, wallets.len()) {
        rows.push(row);
    }
    rows.push(vec![button("➕ Add Wallet", "aw:new")]);
    rows.push(menu_row());

    let text = format!(
        "<b>👛 Tracked Wallets</b>  ({})\n\nTap one to see details or remove it.",
        wallets.len()
    );
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn show_wallet(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(wallet) = WalletRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::WALLET_GONE, "wl")).await;
    };
    let rule_count = WalletRepo::new(&state.db)
        .list()
        .await?
        .into_iter()
        .find(|w| w.id == id)
        .map(|w| w.rule_count)
        .unwrap_or(0);

    let text = format!(
        "👛 <b>{}</b>\n\n<b>Address:</b>\n{}\n\n<b>Alerts:</b> {}",
        esc(wallet.label.as_deref().unwrap_or("unnamed")),
        code(&wallet.address),
        rule_count
    );
    let rows = vec![
        vec![button("🗑 Remove", format!("wl:d:{id}"))],
        back_menu("wl"),
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn confirm_delete_wallet(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    let Some(wallet) = WalletRepo::new(&state.db).find(id).await? else {
        return ui::render(&state.bot, surface, notice(copy::WALLET_GONE, "wl")).await;
    };
    let rule_count = WalletRepo::new(&state.db)
        .list()
        .await?
        .into_iter()
        .find(|w| w.id == id)
        .map(|w| w.rule_count)
        .unwrap_or(0);

    let text = format!(
        "<b>Stop tracking this wallet?</b>\n\n👛 <b>{}</b>\n{}{}",
        esc(wallet.label.as_deref().unwrap_or("unnamed")),
        code(&wallet.address),
        cascade_warning(rule_count)
    );
    let rows = vec![
        vec![button("🗑 Yes, remove", format!("wl:dy:{id}"))],
        vec![button("← Keep it", format!("wl:v:{id}"))],
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn delete_wallet(state: &AppState, surface: Surface, id: i64) -> Result<()> {
    if let Err(err) = WalletRepo::new(&state.db).delete(id).await {
        if !matches!(err, crate::error::AppError::NotFound(_)) {
            return Err(err);
        }
    }
    show_wallets(state, surface, 0).await
}

/// Spells out a cascading delete so removing a target never silently drops its alerts.
fn cascade_warning(rule_count: i64) -> String {
    match rule_count {
        0 => String::new(),
        1 => "\n\n⚠️ This also deletes <b>1 alert</b> watching it.".to_string(),
        n => format!("\n\n⚠️ This also deletes <b>{n} alerts</b> watching it."),
    }
}

// ── History ─────────────────────────────────────────────────────────────────────────

pub async fn show_history(state: &AppState, surface: Surface, page: usize) -> Result<()> {
    // A generous window so paging is meaningful without unbounded reads.
    let events = AlertEventRepo::new(&state.db).list_recent(60).await?;

    if events.is_empty() {
        return ui::render(
            &state.bot,
            surface,
            Screen::new(copy::EMPTY_HISTORY, vec![menu_row()]),
        )
        .await;
    }

    let (page, visible) = paginate(&events, page);

    let body = visible
        .iter()
        .map(|event| {
            let unit = event.kind().map(|kind| kind.unit()).unwrap_or("");
            let target = event
                .target_label
                .clone()
                .unwrap_or_else(|| abbreviate(&event.target_ref));

            let comparison = match Operator::parse(&event.operator) {
                Some(operator) if operator.is_percentage() => match event.reference_value {
                    Some(baseline) => format::change_pct(event.observed_value, baseline)
                        .map(|pct| {
                            format!("{} from {}", format::percent(pct), format::amount(baseline))
                        })
                        .unwrap_or_else(|| format!("{}%", format::amount(event.threshold_value))),
                    None => format!("{}%", format::amount(event.threshold_value)),
                },
                Some(operator) => {
                    format!(
                        "{} {}",
                        operator.symbol(),
                        format::amount(event.threshold_value)
                    )
                }
                None => format::amount(event.threshold_value),
            };

            format!(
                "🔔 <b>{}</b>\n{}\n{} {} ({})",
                esc(&target),
                esc(&format::timestamp(event.triggered_at)),
                esc(&format::amount(event.observed_value)),
                esc(unit),
                esc(&comparison)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut rows = Vec::new();
    if let Some(row) = pager("hi", page, events.len()) {
        rows.push(row);
    }
    rows.push(menu_row());

    let text = format!("<b>📜 Alert History</b>\n\n{body}");
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

// ── Status ──────────────────────────────────────────────────────────────────────────

pub async fn show_status(state: &AppState, surface: Surface) -> Result<()> {
    let body = crate::telegram::commands::status::build(state).await?;
    let text = format!("<b>⚙️ Status</b>\n\n{}", esc(&body));
    let rows = vec![vec![button("🔄 Refresh", "st")], menu_row()];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

// ── Admin ───────────────────────────────────────────────────────────────────────────

pub async fn show_admin(state: &AppState, surface: Surface) -> Result<()> {
    let admins = UserRepo::new(&state.db).count_active_admins().await?;
    let text = copy::admin_panel(admins);
    let rows = vec![
        vec![button("👥 Users", "ad:u")],
        vec![button("➕ Add Admin", "ad:add")],
        menu_row(),
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn show_users(
    state: &AppState,
    surface: Surface,
    page: usize,
    actor_id: i64,
) -> Result<()> {
    let users = UserRepo::new(&state.db).list().await?;
    let (page, visible) = paginate(&users, page);

    let mut rows: Vec<Vec<InlineKeyboardButton>> = visible
        .iter()
        .map(|user| {
            vec![button(
                user_row_label(user, actor_id),
                format!("ad:v:{}", user.telegram_id),
            )]
        })
        .collect();

    if let Some(row) = pager("ad:u", page, users.len()) {
        rows.push(row);
    }
    rows.push(vec![button("➕ Add Admin", "ad:add")]);
    rows.push(back_menu("ad"));

    let text = format!(
        "<b>👥 Users</b>  ({})\n\nTap someone to manage their access.",
        users.len()
    );
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

fn user_row_label(user: &User, actor_id: i64) -> String {
    let role = if user.role == Role::Admin {
        "🛡"
    } else {
        "👤"
    };
    let blocked = if user.blocked { " ⛔" } else { "" };
    let you = if user.telegram_id == actor_id {
        " (you)"
    } else {
        ""
    };
    format!("{role} {}{blocked}{you}", user.telegram_id)
}

pub async fn show_user(
    state: &AppState,
    surface: Surface,
    target_id: i64,
    actor_id: i64,
) -> Result<()> {
    let Some(user) = UserRepo::new(&state.db)
        .find_by_telegram_id(target_id)
        .await?
    else {
        return ui::render(&state.bot, surface, notice(copy::USER_GONE, "ad:u")).await;
    };

    let is_self = user.telegram_id == actor_id;
    let text = format!(
        "<b>User</b> {}\n\n<b>Role:</b> {}\n<b>Access:</b> {}{}",
        code(&user.telegram_id.to_string()),
        if user.role == Role::Admin {
            "admin 🛡"
        } else {
            "user 👤"
        },
        if user.blocked {
            "blocked ⛔"
        } else {
            "active ✅"
        },
        if is_self { "\n\nThis is you." } else { "" }
    );

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    if !is_self {
        // Role.
        if user.role == Role::Admin {
            rows.push(vec![button("⬇ Remove admin", format!("ad:dm:{target_id}"))]);
        } else {
            rows.push(vec![button("⬆ Make admin", format!("ad:pr:{target_id}"))]);
        }
        // Access.
        if user.blocked {
            rows.push(vec![button("✅ Unblock", format!("ad:ub:{target_id}"))]);
        } else {
            rows.push(vec![button("⛔ Block", format!("ad:bl:{target_id}"))]);
        }
    }

    rows.push(back_menu("ad:u"));
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn promote_user(
    state: &AppState,
    surface: Surface,
    target_id: i64,
    actor_id: i64,
) -> Result<()> {
    UserRepo::new(&state.db)
        .upsert(target_id, Role::Admin)
        .await?;
    // A freshly promoted admin gets their command menu right away.
    menu::publish_for_admin(&state.bot, target_id, true).await;
    show_user(state, surface, target_id, actor_id).await
}

pub async fn unblock_user(
    state: &AppState,
    surface: Surface,
    target_id: i64,
    actor_id: i64,
) -> Result<()> {
    UserRepo::new(&state.db)
        .set_blocked(target_id, false)
        .await?;
    show_user(state, surface, target_id, actor_id).await
}

pub async fn confirm_demote(state: &AppState, surface: Surface, target_id: i64) -> Result<()> {
    let text = format!(
        "<b>Remove admin from {}?</b>\n\nThey keep access to the bot but stop receiving alerts \
         and lose admin actions.",
        code(&target_id.to_string())
    );
    let rows = vec![
        vec![button("⬇ Yes, remove admin", format!("ad:dmy:{target_id}"))],
        vec![button("← Cancel", format!("ad:v:{target_id}"))],
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn demote_user(
    state: &AppState,
    surface: Surface,
    target_id: i64,
    actor_id: i64,
) -> Result<()> {
    if target_id == actor_id {
        return ui::render(&state.bot, surface, notice(copy::CANNOT_SELF, "ad:u")).await;
    }
    if crate::telegram::commands::admin::would_orphan_admins(&state.db, target_id).await? {
        return ui::render(&state.bot, surface, notice(copy::LAST_ADMIN, "ad:u")).await;
    }

    UserRepo::new(&state.db)
        .set_role(target_id, Role::User)
        .await?;
    show_user(state, surface, target_id, actor_id).await
}

pub async fn confirm_block(state: &AppState, surface: Surface, target_id: i64) -> Result<()> {
    let text = format!(
        "<b>Block {}?</b>\n\nThey lose all access immediately, even mid-action, and stop \
         receiving alerts.",
        code(&target_id.to_string())
    );
    let rows = vec![
        vec![button("⛔ Yes, block", format!("ad:bly:{target_id}"))],
        vec![button("← Cancel", format!("ad:v:{target_id}"))],
    ];
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

pub async fn block_user(
    state: &AppState,
    surface: Surface,
    target_id: i64,
    actor_id: i64,
) -> Result<()> {
    if target_id == actor_id {
        return ui::render(&state.bot, surface, notice(copy::CANNOT_SELF, "ad:u")).await;
    }
    if crate::telegram::commands::admin::would_orphan_admins(&state.db, target_id).await? {
        return ui::render(&state.bot, surface, notice(copy::LAST_ADMIN, "ad:u")).await;
    }

    UserRepo::new(&state.db)
        .set_blocked(target_id, true)
        .await?;
    show_user(state, surface, target_id, actor_id).await
}
