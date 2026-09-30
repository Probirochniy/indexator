use anyhow::Context;
use prost::Message;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

mod address_cache;
mod processor;
use address_cache::AddressCache;
use processor::{RawTransfer, process_block, process_undo};

pub mod sf {
    pub mod substreams {
        pub mod rpc {
            pub mod v2 {
                tonic::include_proto!("sf.substreams.rpc.v2");
            }
        }
        pub mod v1 {
            tonic::include_proto!("sf.substreams.v1");
        }
    }
}
pub mod erc20 {
    tonic::include_proto!("erc20.types.v1");
}

use sf::substreams::rpc::v2::stream_client::StreamClient;
use sf::substreams::rpc::v2::{Request, response::Message as SubstreamsMessage};
use sf::substreams::v1::Package;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let token = env::var("SUBSTREAMS_API_TOKEN").context("SUBSTREAMS_API_TOKEN is required")?;
    let endpoint = env::var("SUBSTREAMS_ENDPOINT").context("SUBSTREAMS_ENDPOINT is required")?;
    let spkg_path = env::var("SPKG_PATH").context("SPKG_PATH is required")?;
    let default_start_block: i64 = env::var("START_BLOCK")
        .context("START_BLOCK is required")?
        .parse()?;

    let pool = PgPoolOptions::new()
        .max_connections(50)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET synchronous_commit = 'off'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&db_url)
        .await
        .context("postgres connection failed")?;

    sqlx::migrate!("../migrations").run(&pool).await?;
    tracing::info!("migrations applied successfully");

    let sync_state: Option<(String, i64)> =
        sqlx::query_as("SELECT cursor, last_block_number FROM sync_state WHERE id = 1")
            .fetch_optional(&pool)
            .await?;

    let (start_cursor, start_block) = match sync_state {
        Some((cursor, last_block)) => {
            tracing::info!("found saved cursor starting from block {}", last_block);
            (cursor, last_block)
        }
        None => {
            tracing::info!(
                "no saved state found, starting from block {}",
                default_start_block
            );
            ("".into(), default_start_block)
        }
    };

    let spkg_bytes = std::fs::read(&spkg_path)
        .context(format!("failed to read .spkg file from path {}", spkg_path))?;
    let package =
        Package::decode(spkg_bytes.as_slice()).context("failed to decode .spkg package")?;

    let channel = Channel::from_shared(endpoint)?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await?;

    let mut client = StreamClient::with_interceptor(channel, move |mut req: tonic::Request<()>| {
        let auth_header = MetadataValue::try_from(format!("Bearer {}", token))
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;
        req.metadata_mut().insert("authorization", auth_header);
        Ok(req)
    })
    .accept_compressed(tonic::codec::CompressionEncoding::Gzip)
    .accept_compressed(tonic::codec::CompressionEncoding::Zstd);

    let request = Request {
        start_block_num: start_block,
        start_cursor,
        stop_block_num: 0,
        final_blocks_only: false,
        modules: package.modules,
        output_module: "map_transfers".to_string(),
        production_mode: true,
    };

    tracing::info!("connecting to substreams...");
    let mut stream = client.blocks(request).await?.into_inner();
    let cache = Arc::new(AddressCache::new(500_000));

    while let Some(resp) = stream.message().await? {
        match resp.message {
            Some(SubstreamsMessage::BlockScopedData(block_data)) => {
                let clock = block_data.clock.context("no clock in block")?;
                let block_num = clock.number as i64;
                let final_block_num = block_data.final_block_height as i64;
                let cursor = block_data.cursor;

                // СХЛОПНУЛИ ДВА IF В ОДИН ЧЕРЕЗ and_then
                if let Some(map_output) = block_data.output.and_then(|o| o.map_output) {
                    let proto_transfers = erc20::Transfers::decode(map_output.value.as_slice())?;

                    let raw_transfers: Vec<RawTransfer> = proto_transfers
                        .transfers
                        .into_iter()
                        .map(|t| RawTransfer {
                            token: t.token_address,
                            from: t.from,
                            to: t.to,
                            amount: t.amount,
                            tx_hash: t.transaction_hash,
                            block_number: block_num,
                            log_index: t.log_index as i32,
                        })
                        .collect();

                    process_block(
                        &pool,
                        &cache,
                        block_num,
                        final_block_num,
                        &raw_transfers,
                        &cursor,
                    )
                    .await?;
                    tracing::info!(
                        "block {} written transfers: {}",
                        block_num,
                        raw_transfers.len()
                    );
                }
            }

            // Reorg
            Some(SubstreamsMessage::BlockUndoSignal(undo)) => {
                let last_valid = undo.last_valid_block.context("no last_valid_block")?;
                let last_valid_num = last_valid.number as i64;
                let cursor = undo.last_valid_cursor;

                process_undo(&pool, last_valid_num, &cursor).await?;
            }

            _ => {}
        }
    }

    Ok(())
}
