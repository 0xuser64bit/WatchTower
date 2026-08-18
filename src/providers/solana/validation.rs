pub fn is_valid_base58_address(raw: &str) -> bool {
    if raw.len() < 32 || raw.len() > 44 {
        return false;
    }

    match bs58::decode(raw).into_vec() {
        Ok(decoded) => decoded.len() == 32,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_32_byte_base58() {
        let raw = bs58::encode([7u8; 32]).into_string();
        assert!(is_valid_base58_address(&raw));
    }

    #[test]
    fn rejects_wrong_length_or_encoding() {
        assert!(!is_valid_base58_address("short"));
        assert!(!is_valid_base58_address("0OIl"));
    }
}
