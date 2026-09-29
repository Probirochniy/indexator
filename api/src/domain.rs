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
            return Err(AppError::InvalidAddress("надо 20 байт (40 hex)".into()));
        }
        let mut bytes = [0u8; 20];
        hex::decode_to_slice(clean, &mut bytes)
            .map_err(|_| AppError::InvalidAddress("битый hex".into()))?;
        Ok(Self(bytes))
    }
}

impl Address {
    pub fn to_hex(&self) -> String {
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
