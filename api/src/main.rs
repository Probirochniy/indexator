mod domain;
mod error;
mod handlers;
mod repo;

use anyhow::Context;
use axum::{Router, routing::get};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

pub struct AppState {
    pub pool: sqlx::PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL missing")?;
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());

    let pool = PgPoolOptions::new()
        .max_connections(50)
        .connect(&db_url)
        .await?;
    let state = Arc::new(AppState { pool });

    let app = Router::new()
        .route("/v1/status", get(handlers::get_status))
        .route("/v1/tokens/{address}", get(handlers::get_token_metadata))
        .route(
            "/v1/tokens/{address}/transfers",
            get(handlers::get_token_transfers),
        )
        .route(
            "/v1/addresses/{address}/balances",
            get(handlers::get_address_balances),
        )
        .route(
            "/v1/addresses/{address}/transfers",
            get(handlers::get_address_transfers),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("server is on {}", port);
    axum::serve(listener, app).await?;

    Ok(())
}
