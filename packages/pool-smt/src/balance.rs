//! Encoding of a miner payout-lock hash (key) and accrued balance (value).

use sparse_merkle_tree::{traits::Value, H256};

/// Accrued balance in shannons (1 CKB = 100_000_000 shannons).
/// Encoded little-endian into the low 8 bytes of the 32-byte SMT value.
/// Balance 0 -> all-zero value -> key absent from the tree (crate prunes it).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BalanceValue(pub u64);

impl Value for BalanceValue {
    fn to_h256(&self) -> H256 {
        let mut v = [0u8; 32];
        v[0..8].copy_from_slice(&self.0.to_le_bytes());
        v.into()
    }
    fn zero() -> Self {
        BalanceValue(0)
    }
}

/// Parse a `0x`-prefixed 32-byte hex string into an SMT key.
pub fn parse_key(input: &str) -> Result<H256, String> {
    let trimmed = input.trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| format!("hex decode failed: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32-byte key, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(H256::from(arr))
}

/// Parse a decimal shannon string into a `BalanceValue`.
/// Decimal string (not JS number) preserves u64 precision across the wasm boundary.
pub fn parse_balance(input: &str) -> Result<BalanceValue, String> {
    input
        .parse::<u64>()
        .map(BalanceValue)
        .map_err(|e| format!("balance parse failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sparse_merkle_tree::traits::Value;

    #[test]
    fn balance_zero_encodes_to_zero_h256() {
        let v = BalanceValue(0);
        assert!(v.to_h256().is_zero());
    }

    #[test]
    fn balance_one_ckb_encodes_le_in_low_8_bytes() {
        let v = BalanceValue(100_000_000); // 1 CKB in shannons
        let h = v.to_h256();
        let bytes = h.as_slice();
        // 100_000_000 = 0x05F5E100 -> LE bytes 00 E1 F5 05 00 00 00 00
        assert_eq!(&bytes[0..8], &100_000_000u64.to_le_bytes());
        assert_eq!(&bytes[8..32], &[0u8; 24]);
    }

    #[test]
    fn parse_key_round_trips() {
        let hex = format!("0x{}", "ab".repeat(32));
        let key = parse_key(&hex).unwrap();
        assert_eq!(key.as_slice(), &[0xabu8; 32]);
    }

    #[test]
    fn parse_key_rejects_wrong_length() {
        assert!(parse_key("0x1234").is_err());
    }

    #[test]
    fn parse_balance_parses_decimal_string() {
        assert_eq!(parse_balance("100000000").unwrap(), BalanceValue(100_000_000));
    }

    #[test]
    fn parse_balance_rejects_overflow() {
        // u64::MAX + 1
        assert!(parse_balance("18446744073709551616").is_err());
    }

    #[test]
    fn parse_balance_rejects_non_numeric() {
        assert!(parse_balance("not a number").is_err());
    }
}
