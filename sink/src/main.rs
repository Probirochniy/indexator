use anyhow::Context;
use prost::Message;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

use sink::address_cache::*;
use sink::processor::*;

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

async fn poll_chain_head(rpc_url: String, head_atomic: Arc<AtomicU64>) {
    let client = reqwest::Client::new();
    loop {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_blockNumber",
            "params": []
        });

        if let Ok(res) = client.post(&rpc_url).body(payload.to_string()).send().await
            && let Ok(body) = res.text().await
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&body)
            && let Some(hex_str) = val.get("result").and_then(|r| r.as_str())
        {
            let clean = hex_str.trim_start_matches("0x").trim_start_matches("0X");
            if let Ok(num) = u64::from_str_radix(clean, 16) {
                head_atomic.store(num, Ordering::Relaxed);
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

struct ProgressTracker {
    head_atomic: Arc<AtomicU64>,
    last_log: Instant,
    blocks_batch: u64,
    transfers_batch: u64,
}

impl ProgressTracker {
    fn new(head_atomic: Arc<AtomicU64>) -> Self {
        Self {
            head_atomic,
            last_log: Instant::now(),
            blocks_batch: 0,
            transfers_batch: 0,
        }
    }

    fn on_block_processed(&mut self, block_num: i64, transfers_count: usize) {
        self.blocks_batch += 1;
        self.transfers_batch += transfers_count as u64;

        let head = self
            .head_atomic
            .load(Ordering::Relaxed)
            .max(block_num as u64);
        let elapsed = self.last_log.elapsed();
        let lag = head.saturating_sub(block_num as u64);
        let near_head = lag <= 5;

        if elapsed >= Duration::from_secs(2) || (near_head && transfers_count > 0) {
            let secs = elapsed.as_secs_f64();
            let bps = if secs > 0.0 {
                self.blocks_batch as f64 / secs
            } else {
                0.0
            };
            let tps = if secs > 0.0 {
                self.transfers_batch as f64 / secs
            } else {
                0.0
            };

            tracing::info!(
                "block #{} | speed: {:.1} blk/s ({:.0} tx/s) | lag: {} blk | head: #{}",
                block_num,
                bps,
                tps,
                lag,
                head
            );

            self.last_log = Instant::now();
            self.blocks_batch = 0;
            self.transfers_batch = 0;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let rpc_url = env::var("ETH_RPC_URL").context("ETH_RPC_URL missing")?;
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

    let chain_head = Arc::new(AtomicU64::new(0));
    tokio::spawn(poll_chain_head(rpc_url, chain_head.clone()));
    let mut tracker = ProgressTracker::new(chain_head.clone());

    while let Some(resp) = stream.message().await? {
        match resp.message {
            Some(SubstreamsMessage::BlockScopedData(block_data)) => {
                let clock = block_data.clock.context("no clock in block")?;
                let block_num = clock.number as i64;
                let final_block_num = block_data.final_block_height as i64;
                let cursor = block_data.cursor;

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

                    let tx_len = raw_transfers.len();
                    tracker.on_block_processed(block_num, tx_len);
                }
            }

            // Reorg
            Some(SubstreamsMessage::BlockUndoSignal(undo)) => {
                let last_valid = undo.last_valid_block.context("no last_valid_block")?;
                let last_valid_num = last_valid.number as i64;
                let cursor = undo.last_valid_cursor;

                tracing::warn!(
                    "REORG signal received! rolling back to block #{}",
                    last_valid_num
                );
                process_undo(&pool, last_valid_num, &cursor).await?;
            }

            _ => {}
        }
    }

    Ok(())
}
