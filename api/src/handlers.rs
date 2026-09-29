use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use crate::{domain::*, error::AppError, repo::Repo, AppState};

pub async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, AppError> {
    let state_row = Repo::get_sync_state(&state.pool).await?;

    match state_row {
        Some((last_block, updated_at)) => {
            let finalized = (last_block - 64).max(0);
            Ok(Json(json!({
                "last_indexed_block": last_block,
                "last_finalized_block": finalized,
                "updated_at": updated_at
            })))
        }
        None => Ok(Json(json!({ "status": "indexing not started" }))),
    }
}

pub async fn get_token_metadata(
    State(state): State<Arc<AppState>>,
    Path(raw_addr): Path<String>,
) -> Result<Json<Value>, AppError> {
    let addr: Address = raw_addr.parse()?;

    let (symbol, name, decimals) = Repo::get_token_metadata(&state.pool, &addr)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(json!({
        "address": addr.to_hex(),
        "symbol": symbol,
        "name": name,
        "decimals": decimals
    })))
}

pub async fn get_address_balances(
    State(state): State<Arc<AppState>>,
    Path(raw_addr): Path<String>,
) -> Result<Json<Value>, AppError> {
    let addr: Address = raw_addr.parse()?;
    let rows = Repo::get_address_balances(&state.pool, &addr).await?;

    let balances: Vec<_> = rows.into_iter().map(|(hash, amount)| {
        json!({
            "token_address": format!("0x{}", hex::encode(hash)),
            "amount": amount.unwrap_or_else(|| "0".into())
        })
    }).collect();

    Ok(Json(json!({
        "address": addr.to_hex(),
        "balances": balances
    })))
}

pub async fn get_token_transfers(
    State(state): State<Arc<AppState>>,
    Path(raw_addr): Path<String>,
    Query(params): Query<KeysetParams>,
) -> Result<Json<Page<TransferDto>>, AppError> {
    let addr: Address = raw_addr.parse()?;
    let page = Repo::get_token_transfers(&state.pool, &addr, &params).await?;
    Ok(Json(page))
}

pub async fn get_address_transfers(
    State(state): State<Arc<AppState>>,
    Path(raw_addr): Path<String>,
    Query(params): Query<KeysetParams>,
) -> Result<Json<Page<TransferDto>>, AppError> {
    let addr: Address = raw_addr.parse()?;
    let page = Repo::get_address_transfers(&state.pool, &addr, &params).await?;
    Ok(Json(page))
}
