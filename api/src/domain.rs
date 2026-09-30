use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address(pub [u8; 20]);

impl FromStr for Address {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        if clean.len() != 40 {
            return Err(AppError::InvalidAddress("need 20 bytes (40 hex)".into()));
        }
        let mut bytes = [0u8; 20];
        hex::decode_to_slice(clean, &mut bytes)
            .map_err(|_| AppError::InvalidAddress("invalid hex".into()))?;
        Ok(Self(bytes))
    }
}

impl Address {
    pub fn to_hex(self) -> String {
        format!("0x{}", hex::encode(self.0))
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub next_cursor_block: Option<i64>,
    pub next_cursor_log: Option<i32>,
}

#[derive(Serialize)]
pub struct TransferDto {
    pub block_number: i64,
    pub log_index: i32,
    pub tx_hash: String,
    pub token_address: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub is_finalized: bool,
}

#[derive(Deserialize)]
pub struct KeysetParams {
    pub limit: Option<i64>,
    pub cursor_block: Option<i64>,
    pub cursor_log: Option<i32>,
}

impl KeysetParams {
    pub fn sanitized_limit(&self) -> i64 {
        self.limit.unwrap_or(100).clamp(1, 1000)
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_HEX: &str = "d8da6bf26964af9d7eed9e03e53415d37aa96045";

    #[test]
    fn test_address_parsing_valid_variants() {
        let addr1: Address = format!("0x{}", VALID_HEX).parse().unwrap();
        assert_eq!(addr1.to_hex(), format!("0x{}", VALID_HEX));

        let addr2: Address = format!("0X{}", VALID_HEX).parse().unwrap();
        assert_eq!(addr1, addr2);

        let addr3: Address = VALID_HEX.parse().unwrap();
        assert_eq!(addr1, addr3);

        let addr4: Address = format!("   0x{}   \n", VALID_HEX).parse().unwrap();
        assert_eq!(addr1, addr4);

        let upper_hex = VALID_HEX.to_uppercase();
        let addr5: Address = format!("0x{}", upper_hex).parse().unwrap();
        assert_eq!(addr1, addr5);
    }

    #[test]
    fn test_address_parsing_invalid_input() {
        assert!(matches!(
            "".parse::<Address>(),
            Err(AppError::InvalidAddress(_))
        ));

        assert!(matches!(
            "0x".parse::<Address>(),
            Err(AppError::InvalidAddress(_))
        ));

        // (38 symbols)
        let short = "0xd8da6bf26964af9d7eed9e03e53415d37aa960";
        assert!(matches!(
            short.parse::<Address>(),
            Err(AppError::InvalidAddress(_))
        ));

        // (42 symbols)
        let long = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045ff";
        assert!(matches!(
            long.parse::<Address>(),
            Err(AppError::InvalidAddress(_))
        ));

        // Z 🐘
        let invalid_chars = "0xd8da6bf26964af9d7eed9e03e53415d37aa9604Z";
        assert!(matches!(
            invalid_chars.parse::<Address>(),
            Err(AppError::InvalidAddress(_))
        ));
    }

    #[test]
    fn test_address_as_slice_and_to_hex() {
        let raw = [0xabu8; 20];
        let addr = Address(raw);

        assert_eq!(addr.as_slice(), &raw);

        assert_eq!(addr.to_hex(), "0xabababababababababababababababababababab");
    }

    #[test]
    fn test_keyset_params_sanitizer() {
        let params_default = KeysetParams {
            limit: None,
            cursor_block: None,
            cursor_log: None,
        };
        assert_eq!(params_default.sanitized_limit(), 100);

        let params_ok = KeysetParams {
            limit: Some(50),
            cursor_block: None,
            cursor_log: None,
        };
        assert_eq!(params_ok.sanitized_limit(), 50);

        let params_zero = KeysetParams {
            limit: Some(0),
            cursor_block: None,
            cursor_log: None,
        };
        assert_eq!(params_zero.sanitized_limit(), 1);

        let params_neg = KeysetParams {
            limit: Some(-999),
            cursor_block: None,
            cursor_log: None,
        };
        assert_eq!(params_neg.sanitized_limit(), 1);

        let params_huge = KeysetParams {
            limit: Some(1000000),
            cursor_block: None,
            cursor_log: None,
        };
        assert_eq!(params_huge.sanitized_limit(), 1000);
    }

    #[test]
    fn test_to_hex_helper() {
        let bytes = [0xde, 0xad, 0xbe, 0xef];
        assert_eq!(to_hex(&bytes), "0xdeadbeef");

        let empty: [u8; 0] = [];
        assert_eq!(to_hex(&empty), "0x");
    }
}
