use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// SPL Token account layout:
///   offset 0:  mint address (32 bytes)
///   offset 32: owner address (32 bytes)
///   offset 64: amount (u64 little-endian)
///   offset 72: delegate option, delegated amount, state, is_native option,
///              is_native value, close authority option, close authority
/// Total length is 165 bytes.
pub fn parse_token_account(data: &[u8]) -> Option<(String, u64)> {
    if data.len() < 72 {
        return None;
    }

    let mint_bytes = &data[0..32];
    let amount_bytes = &data[64..72];

    let mint = bs58::encode(mint_bytes).into_string();
    let amount = u64::from_le_bytes(amount_bytes.try_into().ok()?);

    Some((mint, amount))
}

pub fn decode_base64_account(data: &str) -> Option<Vec<u8>> {
    STANDARD.decode(data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_165_byte_token_account() {
        let mut data = vec![0u8; 165];

        let mint = [1u8; 32];
        data[0..32].copy_from_slice(&mint);

        let amount: u64 = 1_234_567_890;
        data[64..72].copy_from_slice(&amount.to_le_bytes());

        let (parsed_mint, parsed_amount) = parse_token_account(&data).unwrap();
        assert_eq!(parsed_mint, bs58::encode(mint).into_string());
        assert_eq!(parsed_amount, amount);
    }

    #[test]
    fn rejects_short_data() {
        assert!(parse_token_account(&[0u8; 71]).is_none());
    }
}
