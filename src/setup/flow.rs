//! Step-by-step collection of configuration values.

use super::live::{LiveCheckError, LiveChecker};
use super::prompt::{Prompter, SetupError};
use super::writer::mask_secret;
use crate::config::{
    parse_urls, validate_telegram_bot_token, FieldSpec, FieldTier, Settings, FIELD_CATALOG,
};
use std::collections::HashMap;
use std::path::Path;

const TOKEN: &str = "TELEGRAM_BOT_TOKEN";
const ADMINS: &str = "ADMIN_TELEGRAM_IDS";
const GECKO_KEY: &str = "COINGECKO_API_KEY";
const GECKO_URLS: &str = "COINGECKO_API_URLS";
const RPC: &str = "SOLANA_RPC_ENDPOINTS";

pub async fn collect_answers<P: Prompter, L: LiveChecker>(
    prompter: &mut P,
    checker: &L,
    existing: &HashMap<String, String>,
    env_path: &Path,
) -> Result<HashMap<String, String>, SetupError> {
    let mut answers = HashMap::new();

    answers.insert(
        TOKEN.to_string(),
        prompt_token(prompter, checker, existing).await?,
    );
    answers.insert(ADMINS.to_string(), prompt_admins(prompter, existing)?);

    let configured_providers = prompt_providers(prompter, checker, existing, &mut answers).await?;
    let configured_advanced = prompt_advanced(prompter, existing, &mut answers)?;
    preserve_skipped_sections(
        &mut answers,
        existing,
        configured_providers,
        configured_advanced,
    );

    if !confirm_recap(prompter, existing, &answers, env_path)? {
        return Err(SetupError::Cancelled);
    }

    Ok(answers)
}

fn field_body(field: &FieldSpec) -> String {
    let mut body = field.explanation.to_string();
    if let Some(how) = field.how_to_get {
        body.push_str("\n\nHow to get it:\n");
        body.push_str(how);
    }
    body.push_str("\n\n");
    body.push_str(field.constraints);
    if let Some(default) = field.default {
        body.push_str("\nDefault: ");
        body.push_str(default);
    }
    body
}

fn show_field<P: Prompter>(prompter: &mut P, key: &str) {
    let field = FieldSpec::get(key).expect("catalog field");
    prompter.section(&field.title(), &field_body(field));
}

async fn prompt_token<P: Prompter, L: LiveChecker>(
    prompter: &mut P,
    checker: &L,
    existing: &HashMap<String, String>,
) -> Result<String, SetupError> {
    show_field(prompter, TOKEN);

    let mut pending = None;
    if let Some(existing_token) = existing.get(TOKEN).filter(|value| !value.is_empty()) {
        if prompter.confirm(
            &format!(
                "A token is already set ({}). Keep it?",
                mask_secret(existing_token)
            ),
            true,
        )? {
            pending = Some(existing_token.clone());
        }
    }

    loop {
        let token = match pending.take() {
            Some(token) => token,
            None => {
                let token = prompter.password(TOKEN)?;
                if let Err(err) = validate_telegram_bot_token(&token) {
                    prompter.note(&err.to_string());
                    continue;
                }
                token
            }
        };

        if let Err(err) = validate_telegram_bot_token(&token) {
            prompter.note(&err.to_string());
            continue;
        }

        match checker.telegram_get_me(&token).await {
            Ok(bot) => {
                let username = bot.username.trim_start_matches('@');
                prompter.note(&format!("Authenticated as @{username} (id {}).", bot.id));
                return Ok(token);
            }
            Err(err) => {
                prompter.note(&err.to_string());
                if !prompter.confirm("Try a different token?", true)? {
                    return Err(SetupError::Cancelled);
                }
            }
        }
    }
}

fn prompt_admins<P: Prompter>(
    prompter: &mut P,
    existing: &HashMap<String, String>,
) -> Result<String, SetupError> {
    show_field(prompter, ADMINS);

    let mut ids = Vec::new();
    if let Some(existing_ids) = existing.get(ADMINS).filter(|value| !value.is_empty()) {
        if prompter.confirm(
            &format!("Existing admin ids: {existing_ids}. Keep them?"),
            true,
        )? {
            ids = crate::config::parse_admin_ids(existing_ids).unwrap_or_default();
        }
    }

    if ids.is_empty() {
        ids.push(read_admin_id(prompter)?);
    }

    while prompter.confirm("Add another administrator?", false)? {
        let id = read_admin_id(prompter)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    Ok(ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

fn read_admin_id<P: Prompter>(prompter: &mut P) -> Result<i64, SetupError> {
    loop {
        let raw = prompter.text("Admin Telegram user id", None)?;
        match raw.trim().parse::<i64>() {
            Ok(id) if id > 0 => return Ok(id),
            _ => prompter.note(&format!(
                "`{}` is not a numeric Telegram user id. Use the number from @userinfobot.",
                raw.trim()
            )),
        }
    }
}

async fn prompt_providers<P: Prompter, L: LiveChecker>(
    prompter: &mut P,
    checker: &L,
    existing: &HashMap<String, String>,
    answers: &mut HashMap<String, String>,
) -> Result<bool, SetupError> {
    prompter.section(
        "Recommended providers",
        "A CoinGecko API key and a private Solana RPC keep price and balance\n\
         polls reliable. Public defaults work for a quick test and rate-limit\n\
         under real use.",
    );

    if !prompter.confirm(
        "Configure production providers now? Recommended unless this is a throwaway test.",
        true,
    )? {
        if existing
            .get(GECKO_KEY)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
            || existing
                .get(RPC)
                .map(|value| !value.is_empty())
                .unwrap_or(false)
        {
            prompter.note("Keeping the provider settings already in .env.");
        } else {
            prompter.note(
                "Using public CoinGecko and the public Solana RPC. Re-run setup before relying on this in production.",
            );
        }
        return Ok(false);
    }

    prompt_coingecko_key(prompter, checker, existing, answers).await?;
    prompt_rpc_endpoints(prompter, checker, existing, answers).await?;
    Ok(true)
}

async fn prompt_coingecko_key<P: Prompter, L: LiveChecker>(
    prompter: &mut P,
    checker: &L,
    existing: &HashMap<String, String>,
    answers: &mut HashMap<String, String>,
) -> Result<(), SetupError> {
    show_field(prompter, GECKO_KEY);

    if let Some(existing_key) = existing.get(GECKO_KEY).filter(|value| !value.is_empty()) {
        if prompter.confirm(
            &format!(
                "A CoinGecko key is already set ({}). Keep it?",
                mask_secret(existing_key)
            ),
            true,
        )? {
            match check_coingecko(checker, answers, Some(existing_key)).await {
                Ok(()) => {
                    answers.insert(GECKO_KEY.to_string(), existing_key.clone());
                    return Ok(());
                }
                Err(err) => prompter.note(&err.to_string()),
            }
        }
    }

    if !prompter.confirm(
        "Set a CoinGecko API key? Strongly recommended for real use.",
        true,
    )? {
        return Ok(());
    }

    loop {
        let key = prompter.password(GECKO_KEY)?;
        if key.trim().is_empty() {
            return Ok(());
        }
        match check_coingecko(checker, answers, Some(&key)).await {
            Ok(()) => {
                prompter.note("CoinGecko accepted the key.");
                answers.insert(GECKO_KEY.to_string(), key);
                return Ok(());
            }
            Err(err) => {
                prompter.note(&err.to_string());
                match prompter.choose(
                    "CoinGecko check failed. What next?",
                    &["Re-enter key", "Leave unset", "Save anyway", "Abort setup"],
                    0,
                )? {
                    1 => return Ok(()),
                    2 => {
                        answers.insert(GECKO_KEY.to_string(), key);
                        return Ok(());
                    }
                    3 => return Err(SetupError::Cancelled),
                    _ => continue,
                }
            }
        }
    }
}

async fn check_coingecko<L: LiveChecker>(
    checker: &L,
    answers: &HashMap<String, String>,
    key: Option<&str>,
) -> Result<(), LiveCheckError> {
    let base = answers
        .get(GECKO_URLS)
        .cloned()
        .unwrap_or_else(|| FieldSpec::default_value(GECKO_URLS).to_string());
    let base = base.split(',').next().unwrap_or(&base).trim();
    checker.ping_coingecko(base, key).await
}

async fn prompt_rpc_endpoints<P: Prompter, L: LiveChecker>(
    prompter: &mut P,
    checker: &L,
    existing: &HashMap<String, String>,
    answers: &mut HashMap<String, String>,
) -> Result<(), SetupError> {
    show_field(prompter, RPC);

    let mut urls = Vec::new();
    if let Some(existing_urls) = existing.get(RPC).filter(|value| !value.is_empty()) {
        if prompter.confirm(
            &format!("Existing RPC endpoints: {existing_urls}. Keep them?"),
            true,
        )? {
            if let Ok(parsed) = parse_urls(RPC, existing_urls) {
                for url in parsed {
                    match checker.ping_solana_rpc(&url).await {
                        Ok(()) => {
                            prompter.note(&format!("RPC ok: {url}"));
                            urls.push(url);
                        }
                        Err(err) => {
                            prompter.note(&format!("{url}: {err}"));
                            match prompter.choose(
                                "This endpoint failed. What next?",
                                &["Drop this endpoint", "Keep with warning", "Abort setup"],
                                0,
                            )? {
                                1 => urls.push(url),
                                2 => return Err(SetupError::Cancelled),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    if urls.is_empty() {
        let default = existing
            .get(RPC)
            .map(String::as_str)
            .or(FieldSpec::get(RPC).and_then(|field| field.default));
        loop {
            let raw = prompter.text("Solana RPC endpoint", default)?;
            match parse_urls(RPC, raw.trim()) {
                Err(err) => {
                    prompter.note(&err.to_string());
                    continue;
                }
                Ok(parsed) => {
                    let url = parsed.into_iter().next().expect("parse_urls non-empty");
                    match checker.ping_solana_rpc(&url).await {
                        Ok(()) => {
                            prompter.note(&format!("RPC ok: {url}"));
                            urls.push(url);
                        }
                        Err(err) => {
                            prompter.note(&err.to_string());
                            match prompter.choose(
                                "RPC check failed. What next?",
                                &[
                                    "Retry / re-enter",
                                    "Drop this endpoint",
                                    "Keep with warning",
                                    "Abort setup",
                                ],
                                0,
                            )? {
                                1 => {}
                                2 => urls.push(url),
                                3 => return Err(SetupError::Cancelled),
                                _ => continue,
                            }
                        }
                    }
                }
            }
            if urls.is_empty() {
                continue;
            }
            if !prompter.confirm("Add a failover RPC endpoint?", false)? {
                break;
            }
        }
    } else if prompter.confirm("Add a failover RPC endpoint?", false)? {
        loop {
            let raw = prompter.text("Solana RPC endpoint", None)?;
            match parse_urls(RPC, raw.trim()) {
                Ok(parsed) => {
                    for url in parsed {
                        if !urls.contains(&url) {
                            urls.push(url);
                        }
                    }
                }
                Err(err) => {
                    prompter.note(&err.to_string());
                    continue;
                }
            }
            if !prompter.confirm("Add another failover RPC endpoint?", false)? {
                break;
            }
        }
    }

    if !urls.is_empty() {
        answers.insert(RPC.to_string(), urls.join(","));
    }
    Ok(())
}

fn prompt_advanced<P: Prompter>(
    prompter: &mut P,
    existing: &HashMap<String, String>,
    answers: &mut HashMap<String, String>,
) -> Result<bool, SetupError> {
    if !prompter.confirm(
        "Configure advanced settings? (database, poll interval, logs, commitment, …)",
        false,
    )? {
        return Ok(false);
    }

    for field in FIELD_CATALOG
        .iter()
        .filter(|field| field.tier == FieldTier::Advanced)
    {
        show_field(prompter, field.key);
        let prompt_default = existing
            .get(field.key)
            .map(String::as_str)
            .or(field.default);
        let raw = prompter.text(field.key, prompt_default)?;
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if field.key != "RUST_LOG" {
            if let Err(err) = validate_field(field.key, value, answers) {
                prompter.note(&err);
                let retry = prompter.text(field.key, Some(value))?;
                let retry = retry.trim();
                if retry.is_empty() {
                    continue;
                }
                if let Err(err) = validate_field(field.key, retry, answers) {
                    return Err(SetupError::Invalid(err));
                }
                insert_if_not_default(answers, field, retry);
                continue;
            }
        }
        insert_if_not_default(answers, field, value);
    }

    Ok(true)
}

fn preserve_skipped_sections(
    answers: &mut HashMap<String, String>,
    existing: &HashMap<String, String>,
    configured_providers: bool,
    configured_advanced: bool,
) {
    for field in FIELD_CATALOG {
        if answers.contains_key(field.key) {
            continue;
        }
        let Some(value) = existing.get(field.key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let keep = match field.tier {
            FieldTier::Required => false,
            FieldTier::Recommended => !configured_providers,
            FieldTier::Advanced => !configured_advanced,
        };
        if keep {
            answers.insert(field.key.to_string(), value.clone());
        }
    }
}

fn insert_if_not_default(answers: &mut HashMap<String, String>, field: &FieldSpec, value: &str) {
    if field.default != Some(value) {
        answers.insert(field.key.to_string(), value.to_string());
    }
}

fn validate_field(key: &str, value: &str, answers: &HashMap<String, String>) -> Result<(), String> {
    let mut env = HashMap::from([
        (
            TOKEN.to_string(),
            answers
                .get(TOKEN)
                .cloned()
                .unwrap_or_else(|| "1234567890:AAEhBOweik6ad".into()),
        ),
        (
            ADMINS.to_string(),
            answers.get(ADMINS).cloned().unwrap_or_else(|| "1".into()),
        ),
    ]);
    env.extend(answers.clone());
    env.insert(key.to_string(), value.to_string());
    Settings::from_env_map(&env)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn confirm_recap<P: Prompter>(
    prompter: &mut P,
    existing: &HashMap<String, String>,
    answers: &HashMap<String, String>,
    env_path: &Path,
) -> Result<bool, SetupError> {
    let mut table = String::from("These values will be written:\n");
    for field in FIELD_CATALOG {
        let (origin, display) = recap_cell(field, existing, answers);
        table.push_str(&format!("  {:<32} {:<8} {}\n", field.key, origin, display));
    }
    table.push_str("\nSecrets are masked. The file will be mode 600.");
    prompter.section("Recap", &table);
    prompter.confirm(
        &format!("Write these values to {}?", env_path.display()),
        true,
    )
}

fn recap_cell(
    field: &FieldSpec,
    existing: &HashMap<String, String>,
    answers: &HashMap<String, String>,
) -> (&'static str, String) {
    if let Some(value) = answers.get(field.key) {
        let origin = if existing.get(field.key) == Some(value) {
            "existing"
        } else {
            "set"
        };
        let display = if field.secret {
            mask_secret(value)
        } else {
            value.clone()
        };
        (origin, display)
    } else if let Some(default) = field.default {
        ("default", default.to_string())
    } else {
        ("unset", "—".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::live::{BotInfo, LiveCheckError};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;

    enum Cue {
        Confirm(bool),
        Text(String),
        Password(String),
        Choose(usize),
    }

    struct Scripted {
        cues: VecDeque<Cue>,
        notes: Vec<String>,
    }

    impl Scripted {
        fn new(cues: Vec<Cue>) -> Self {
            Self {
                cues: cues.into(),
                notes: Vec::new(),
            }
        }
    }

    impl Prompter for Scripted {
        fn intro(&mut self, _text: &str) {}
        fn section(&mut self, _title: &str, _body: &str) {}
        fn note(&mut self, text: &str) {
            self.notes.push(text.to_string());
        }
        fn confirm(&mut self, _question: &str, _default: bool) -> Result<bool, SetupError> {
            match self.cues.pop_front() {
                Some(Cue::Confirm(value)) => Ok(value),
                other => panic!("expected confirm, got {other:?} remaining {:?}", self.cues),
            }
        }
        fn text(&mut self, _label: &str, _default: Option<&str>) -> Result<String, SetupError> {
            match self.cues.pop_front() {
                Some(Cue::Text(value)) => Ok(value),
                other => panic!("expected text, got {other:?} remaining {:?}", self.cues),
            }
        }
        fn password(&mut self, _label: &str) -> Result<String, SetupError> {
            match self.cues.pop_front() {
                Some(Cue::Password(value)) => Ok(value),
                other => panic!("expected password, got {other:?} remaining {:?}", self.cues),
            }
        }
        fn choose(
            &mut self,
            _question: &str,
            _options: &[&str],
            _default: usize,
        ) -> Result<usize, SetupError> {
            match self.cues.pop_front() {
                Some(Cue::Choose(value)) => Ok(value),
                other => panic!("expected choose, got {other:?} remaining {:?}", self.cues),
            }
        }
    }

    impl std::fmt::Debug for Cue {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Cue::Confirm(v) => write!(f, "Confirm({v})"),
                Cue::Text(v) => write!(f, "Text({v})"),
                Cue::Password(_) => write!(f, "Password(<redacted>)"),
                Cue::Choose(v) => write!(f, "Choose({v})"),
            }
        }
    }

    struct FakeLive {
        telegram: Mutex<VecDeque<Result<BotInfo, LiveCheckError>>>,
        coingecko: Mutex<VecDeque<Result<(), LiveCheckError>>>,
        solana: Mutex<VecDeque<Result<(), LiveCheckError>>>,
        telegram_tokens: Mutex<Vec<String>>,
    }

    impl FakeLive {
        fn ok() -> Self {
            Self {
                telegram: Mutex::new(VecDeque::new()),
                coingecko: Mutex::new(VecDeque::new()),
                solana: Mutex::new(VecDeque::new()),
                telegram_tokens: Mutex::new(Vec::new()),
            }
        }

        fn push_telegram(&self, result: Result<BotInfo, LiveCheckError>) {
            self.telegram.lock().unwrap().push_back(result);
        }

        fn push_gecko(&self, result: Result<(), LiveCheckError>) {
            self.coingecko.lock().unwrap().push_back(result);
        }

        fn push_solana(&self, result: Result<(), LiveCheckError>) {
            self.solana.lock().unwrap().push_back(result);
        }
    }

    fn bot() -> BotInfo {
        BotInfo {
            id: 42,
            username: "watchtower_bot".into(),
        }
    }

    #[async_trait::async_trait]
    impl LiveChecker for FakeLive {
        async fn telegram_get_me(&self, token: &str) -> Result<BotInfo, LiveCheckError> {
            self.telegram_tokens.lock().unwrap().push(token.to_string());
            match self.telegram.lock().unwrap().pop_front() {
                Some(result) => result,
                None => Ok(bot()),
            }
        }
        async fn ping_coingecko(
            &self,
            _base_url: &str,
            _api_key: Option<&str>,
        ) -> Result<(), LiveCheckError> {
            match self.coingecko.lock().unwrap().pop_front() {
                Some(result) => result,
                None => Ok(()),
            }
        }
        async fn ping_solana_rpc(&self, _url: &str) -> Result<(), LiveCheckError> {
            match self.solana.lock().unwrap().pop_front() {
                Some(result) => result,
                None => Ok(()),
            }
        }
    }

    const GOOD_TOKEN: &str = "1234567890:AAEhBOweik6ad";

    fn required_only() -> Vec<Cue> {
        vec![
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Text("111".into()),
            Cue::Confirm(false), // add another admin
            Cue::Confirm(false), // providers
            Cue::Confirm(false), // advanced
            Cue::Confirm(true),  // recap
        ]
    }

    #[tokio::test]
    async fn required_only_produces_valid_settings() {
        let mut prompter = Scripted::new(required_only());
        let live = FakeLive::ok();
        let answers = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap();

        assert_eq!(answers.get(TOKEN).unwrap(), GOOD_TOKEN);
        assert_eq!(answers.get(ADMINS).unwrap(), "111");
        assert!(!answers.contains_key(GECKO_KEY));
        Settings::from_env_map(&answers).unwrap();
        assert!(prompter
            .notes
            .iter()
            .any(|note| note.contains("@watchtower_bot")));
    }

    #[tokio::test]
    async fn bad_token_format_is_rejected_before_live_check() {
        let mut prompter = Scripted::new(vec![
            Cue::Password("nocolon".into()),
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Text("111".into()),
            Cue::Confirm(false),
            Cue::Confirm(false),
            Cue::Confirm(false),
            Cue::Confirm(true),
        ]);
        let live = FakeLive::ok();
        let answers = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap();

        assert_eq!(
            live.telegram_tokens.lock().unwrap().as_slice(),
            &[GOOD_TOKEN]
        );
        assert_eq!(answers.get(TOKEN).unwrap(), GOOD_TOKEN);
    }

    #[tokio::test]
    async fn recap_decline_cancels_without_answers_used() {
        let mut cues = required_only();
        cues.pop();
        cues.push(Cue::Confirm(false));
        let mut prompter = Scripted::new(cues);
        let live = FakeLive::ok();
        let err = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SetupError::Cancelled));
    }

    #[tokio::test]
    async fn merge_keeps_existing_secrets_when_confirmed() {
        let existing = HashMap::from([
            (TOKEN.to_string(), GOOD_TOKEN.to_string()),
            (ADMINS.to_string(), "111,222".to_string()),
        ]);
        let mut prompter = Scripted::new(vec![
            Cue::Confirm(true),  // keep token
            Cue::Confirm(true),  // keep admins
            Cue::Confirm(false), // another admin
            Cue::Confirm(false), // providers
            Cue::Confirm(false), // advanced
            Cue::Confirm(true),  // recap
        ]);
        let live = FakeLive::ok();
        let answers = collect_answers(&mut prompter, &live, &existing, &PathBuf::from(".env"))
            .await
            .unwrap();
        assert_eq!(answers.get(TOKEN).unwrap(), GOOD_TOKEN);
        assert_eq!(answers.get(ADMINS).unwrap(), "111,222");
    }

    #[tokio::test]
    async fn skipped_coingecko_key_after_failed_ping_is_omitted() {
        let mut prompter = Scripted::new(vec![
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Text("111".into()),
            Cue::Confirm(false), // another admin
            Cue::Confirm(true),  // providers
            Cue::Confirm(true),  // set gecko key
            Cue::Password("bad-key".into()),
            Cue::Choose(1), // leave unset
            Cue::Text("https://rpc.example".into()),
            Cue::Confirm(false), // failover
            Cue::Confirm(false), // advanced
            Cue::Confirm(true),  // recap
        ]);
        let live = FakeLive::ok();
        live.push_gecko(Err(LiveCheckError::Rejected("nope".into())));
        let answers = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap();
        assert!(!answers.contains_key(GECKO_KEY));
        assert_eq!(answers.get(RPC).unwrap(), "https://rpc.example");
    }

    #[tokio::test]
    async fn unhealthy_rpc_can_be_kept() {
        let mut prompter = Scripted::new(vec![
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Text("111".into()),
            Cue::Confirm(false),
            Cue::Confirm(true),  // providers
            Cue::Confirm(false), // no gecko key
            Cue::Text("https://rpc.example".into()),
            Cue::Choose(2),      // keep with warning
            Cue::Confirm(false), // failover
            Cue::Confirm(false),
            Cue::Confirm(true),
        ]);
        let live = FakeLive::ok();
        live.push_solana(Err(LiveCheckError::Unreachable("down".into())));
        let answers = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap();
        assert_eq!(answers.get(RPC).unwrap(), "https://rpc.example");
    }

    #[tokio::test]
    async fn recommended_and_advanced_round_trip_through_settings() {
        let mut prompter = Scripted::new(vec![
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Text("111".into()),
            Cue::Confirm(false),
            Cue::Confirm(true), // providers
            Cue::Confirm(true), // gecko
            Cue::Password("demo-key".into()),
            Cue::Text("https://rpc.example".into()),
            Cue::Confirm(false),
            Cue::Confirm(true), // advanced
            Cue::Text("sqlite://data/custom.db".into()),
            Cue::Text(FieldSpec::default_value(GECKO_URLS).into()),
            Cue::Text("confirmed".into()),
            Cue::Text("10".into()),
            Cue::Text("30".into()),
            Cue::Text("300".into()),
            Cue::Text("90".into()),
            Cue::Text("logs".into()),
            Cue::Text("14".into()),
            Cue::Text("info,watchtower=debug".into()),
            Cue::Confirm(true),
        ]);
        let live = FakeLive::ok();
        let answers = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap();

        let settings = Settings::from_env_map(&answers).unwrap();
        assert_eq!(settings.database_url, "sqlite://data/custom.db");
        assert_eq!(settings.poll_interval.as_secs(), 30);
        assert_eq!(answers.get("RUST_LOG").unwrap(), "info,watchtower=debug");
        assert_eq!(answers.get(GECKO_KEY).unwrap(), "demo-key");
    }

    #[tokio::test]
    async fn skipping_sections_preserves_existing_optional_values() {
        let existing = HashMap::from([
            (TOKEN.to_string(), GOOD_TOKEN.to_string()),
            (ADMINS.to_string(), "111".to_string()),
            (GECKO_KEY.to_string(), "demo-key".to_string()),
            (
                "DATABASE_URL".to_string(),
                "sqlite://data/custom.db".to_string(),
            ),
        ]);
        let mut prompter = Scripted::new(vec![
            Cue::Confirm(true),  // keep token
            Cue::Confirm(true),  // keep admins
            Cue::Confirm(false), // another admin
            Cue::Confirm(false), // providers
            Cue::Confirm(false), // advanced
            Cue::Confirm(true),  // recap
        ]);
        let live = FakeLive::ok();
        let answers = collect_answers(&mut prompter, &live, &existing, &PathBuf::from(".env"))
            .await
            .unwrap();
        assert_eq!(answers.get(GECKO_KEY).unwrap(), "demo-key");
        assert_eq!(
            answers.get("DATABASE_URL").unwrap(),
            "sqlite://data/custom.db"
        );
        Settings::from_env_map(&answers).unwrap();
    }

    #[tokio::test]
    async fn live_telegram_failure_can_abort() {
        let mut prompter = Scripted::new(vec![
            Cue::Password(GOOD_TOKEN.into()),
            Cue::Confirm(false), // do not retry
        ]);
        let live = FakeLive::ok();
        live.push_telegram(Err(LiveCheckError::Rejected("bad token".into())));
        let err = collect_answers(
            &mut prompter,
            &live,
            &HashMap::new(),
            &PathBuf::from(".env"),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SetupError::Cancelled));
    }
}
