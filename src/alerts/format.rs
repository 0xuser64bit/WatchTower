use crate::rules::types::Rule;
use chrono::Utc;

pub fn format_alert(rule: &Rule, current: f64, threshold: f64) -> String {
    let now = Utc::now();
    let reason = match rule.kind() {
        crate::rules::types::RuleKind::Price => "price crossed threshold",
        crate::rules::types::RuleKind::Balance => "balance crossed threshold",
        crate::rules::types::RuleKind::Activity => "wallet activity detected",
    };

    format!(
        "⚠️ {} alert\nTarget: {}\nChain: Solana\nReason: {}\nCurrent: {:.2}\nThreshold: {:.2}\nAt: {}",
        rule.kind,
        rule.target_ref,
        reason,
        current,
        threshold,
        now.format("%Y-%m-%d %H:%M:%S UTC")
    )
}
