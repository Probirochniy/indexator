use crate::domain::Address;
use crate::error::AppError;
use serde::Deserialize;
use serde_json::json;

const SIG_NAME: &str = "0x06fdde03"; // name()
const SIG_SYMBOL: &str = "0x95d89b41"; // symbol()
const SIG_DECIMALS: &str = "0x313ce567"; // decimals()

#[derive(Deserialize)]
struct RpcResponse {
    id: usize,
    result: Option<String>,
}

#[derive(Debug)]
pub struct FetchedMetadata {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<i16>,
}

pub async fn fetch_token_metadata_from_chain(
    rpc_url: &str,
    token_addr: &Address,
) -> Result<FetchedMetadata, AppError> {
    let client = reqwest::Client::new();
    let addr_hex = token_addr.to_hex();

    let batch_payload = json!([
        { "jsonrpc": "2.0", "id": 1, "method": "eth_call", "params": [{ "to": addr_hex, "data": SIG_NAME }, "latest"] },
        { "jsonrpc": "2.0", "id": 2, "method": "eth_call", "params": [{ "to": addr_hex, "data": SIG_SYMBOL }, "latest"] },
        { "jsonrpc": "2.0", "id": 3, "method": "eth_call", "params": [{ "to": addr_hex, "data": SIG_DECIMALS }, "latest"] }
    ]);

    let res = client
        .post(rpc_url)
        .json(&batch_payload)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("rpc call failed: {:?}", e);
            AppError::NotFound
        })?;

    let mut responses: Vec<RpcResponse> = res.json().await.map_err(|_| AppError::NotFound)?;
    responses.sort_by_key(|r| r.id);

    if responses.len() < 3 {
        return Err(AppError::NotFound);
    }

    let name = responses[0].result.as_deref().and_then(parse_abi_string);
    let symbol = responses[1].result.as_deref().and_then(parse_abi_string);
    let decimals = responses[2].result.as_deref().and_then(parse_abi_uint);

    Ok(FetchedMetadata {
        name,
        symbol,
        decimals,
    })
}

fn parse_abi_string(hex_raw: &str) -> Option<String> {
    let clean = hex_raw.trim_start_matches("0x");
    let bytes = hex::decode(clean).ok()?;
    if bytes.is_empty() {
        return None;
    }

    if bytes.len() >= 64 {
        let len = u32::from_be_bytes(bytes[60..64].try_into().ok()?) as usize;
        if bytes.len() >= 64 + len {
            return String::from_utf8(bytes[64..64 + len].to_vec()).ok();
        }
    }

    if bytes.len() == 32 {
        let trimmed: Vec<u8> = bytes.into_iter().take_while(|&b| b != 0).collect();
        return String::from_utf8(trimmed).ok();
    }

    None
}

fn parse_abi_uint(hex_raw: &str) -> Option<i16> {
    let clean = hex_raw.trim_start_matches("0x");
    let bytes = hex::decode(clean).ok()?;
    if bytes.is_empty() {
        return None;
    }
    bytes.last().map(|&b| b as i16)
}

#[derive(Deserialize)]
struct BlockNumResponse {
    result: Option<String>,
}

pub async fn fetch_head_block_number(rpc_url: &str) -> Result<i64, AppError> {
    let client = reqwest::Client::new();
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_blockNumber",
        "params": []
    });

    let res = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("rpc eth_blockNumber failed: {:?}", e);
            AppError::NotFound
        })?;

    let parsed: BlockNumResponse = res.json().await.map_err(|_| AppError::NotFound)?;
    let hex_val = parsed.result.ok_or(AppError::NotFound)?;
    let clean = hex_val.trim_start_matches("0x").trim_start_matches("0X");

    i64::from_str_radix(clean, 16).map_err(|_| AppError::NotFound)
}
