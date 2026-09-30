use crate::domain::{Address, KeysetParams, Page, TransferDto, to_hex};
use crate::error::AppError;
use sqlx::PgPool;

pub struct Repo;

#[derive(sqlx::FromRow)]
struct RawTransferRow {
    block_number: i64,
    log_index: i32,
    tx_hash: Vec<u8>,
    amount: Option<String>,
    from_hash: Vec<u8>,
    to_hash: Vec<u8>,
    token_hash: Option<Vec<u8>>,
}

impl Repo {
    pub async fn get_sync_state(pool: &PgPool) -> Result<Option<(i64, i64, String)>, AppError> {
        let row: Option<(i64, i64, String)> = sqlx::query_as(
            "SELECT last_block_number, last_final_block_number, updated_at::text FROM sync_state WHERE id = 1"
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    pub async fn save_token_metadata(
        pool: &PgPool,
        addr: &Address,
        symbol: Option<&str>,
        name: Option<&str>,
        decimals: Option<i16>,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            WITH target_addr AS (
                INSERT INTO addresses (hash)
                VALUES ($1)
                ON CONFLICT (hash) DO UPDATE SET hash = EXCLUDED.hash
                RETURNING id
            )
            INSERT INTO tokens (address_id, symbol, name, decimals)
            SELECT id, $2, $3, $4 FROM target_addr
            ON CONFLICT (address_id) DO UPDATE
            SET symbol = EXCLUDED.symbol, name = EXCLUDED.name, decimals = EXCLUDED.decimals;
            "#,
        )
        .bind(addr.as_slice())
        .bind(symbol)
        .bind(name)
        .bind(decimals)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn get_token_metadata(
        pool: &PgPool,
        addr: &Address,
    ) -> Result<Option<(Option<String>, Option<String>, Option<i16>)>, AppError> {
        let row = sqlx::query_as(
            r#"
            SELECT t.symbol, t.name, t.decimals
            FROM tokens t
            WHERE t.address_id = (SELECT id FROM addresses WHERE hash = $1)
            "#,
        )
        .bind(addr.as_slice())
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    pub async fn get_address_balances(
        pool: &PgPool,
        addr: &Address,
    ) -> Result<Vec<(Vec<u8>, Option<String>)>, AppError> {
        let rows = sqlx::query_as(
            r#"
            SELECT a.hash, b.amount::text
            FROM balances b
            JOIN addresses a ON b.token_address_id = a.id
            WHERE b.account_id = (SELECT id FROM addresses WHERE hash = $1)
              AND b.amount > 0
            "#,
        )
        .bind(addr.as_slice())
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_finalized_boundary(pool: &PgPool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT last_final_block_number FROM sync_state WHERE id = 1")
            .fetch_optional(pool)
            .await
            .unwrap_or_default()
            .unwrap_or(0)
    }

    pub async fn get_token_transfers(
        pool: &PgPool,
        token_addr: &Address,
        params: &KeysetParams,
    ) -> Result<Page<TransferDto>, AppError> {
        let limit = params.sanitized_limit();
        let finalized_boundary = Self::get_finalized_boundary(pool).await;

        let rows = sqlx::query_as::<_, RawTransferRow>(
            r#"
            SELECT
                t.block_number, t.log_index, t.tx_hash, t.amount::text AS amount,
                fa.hash AS from_hash, ta.hash AS to_hash, NULL::bytea AS token_hash
            FROM transfers t
            JOIN addresses fa ON t.from_address_id = fa.id
            JOIN addresses ta ON t.to_address_id = ta.id
            WHERE t.token_address_id = (SELECT id FROM addresses WHERE hash = $1)
              AND ($2::bigint IS NULL OR (t.block_number, t.log_index) < ($2, $3))
            ORDER BY t.block_number DESC, t.log_index DESC
            LIMIT $4
            "#,
        )
        .bind(token_addr.as_slice())
        .bind(params.cursor_block)
        .bind(params.cursor_log)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let next_cursor = rows.last().map(|r| (r.block_number, r.log_index));

        let data = rows
            .into_iter()
            .map(|r| TransferDto {
                block_number: r.block_number,
                log_index: r.log_index,
                tx_hash: to_hex(&r.tx_hash),
                token_address: token_addr.to_hex(),
                from: to_hex(&r.from_hash),
                to: to_hex(&r.to_hash),
                amount: r.amount.unwrap_or_default(),
                is_finalized: r.block_number <= finalized_boundary,
            })
            .collect();

        Ok(Page {
            data,
            next_cursor_block: next_cursor.map(|c| c.0),
            next_cursor_log: next_cursor.map(|c| c.1),
        })
    }

    pub async fn get_address_transfers(
        pool: &PgPool,
        wallet_addr: &Address,
        params: &KeysetParams,
    ) -> Result<Page<TransferDto>, AppError> {
        let limit = params.sanitized_limit();
        let finalized_boundary = Self::get_finalized_boundary(pool).await;

        let rows = sqlx::query_as::<_, RawTransferRow>(
            r#"
            WITH target AS (
                SELECT id FROM addresses WHERE hash = $1
            ),
            wallet_transfers AS (
                (
                    SELECT t.block_number, t.log_index, t.tx_hash, t.amount::text AS amount,
                           fa.hash AS from_hash, ta.hash AS to_hash, toka.hash AS token_hash
                    FROM transfers t
                    CROSS JOIN target
                    JOIN addresses fa ON t.from_address_id = fa.id
                    JOIN addresses ta ON t.to_address_id = ta.id
                    JOIN addresses toka ON t.token_address_id = toka.id
                    WHERE t.from_address_id = target.id
                      AND ($2::bigint IS NULL OR (t.block_number, t.log_index) < ($2, $3))
                    ORDER BY t.block_number DESC, t.log_index DESC
                    LIMIT $4
                )
                UNION ALL
                (
                    SELECT t.block_number, t.log_index, t.tx_hash, t.amount::text AS amount,
                           fa.hash AS from_hash, ta.hash AS to_hash, toka.hash AS token_hash
                    FROM transfers t
                    CROSS JOIN target
                    JOIN addresses fa ON t.from_address_id = fa.id
                    JOIN addresses ta ON t.to_address_id = ta.id
                    JOIN addresses toka ON t.token_address_id = toka.id
                    WHERE t.to_address_id = target.id
                      AND t.from_address_id != target.id
                      AND ($2::bigint IS NULL OR (t.block_number, t.log_index) < ($2, $3))
                    ORDER BY t.block_number DESC, t.log_index DESC
                    LIMIT $4
                )
            )
            SELECT * FROM wallet_transfers
            ORDER BY block_number DESC, log_index DESC
            LIMIT $4;
            "#,
        )
        .bind(wallet_addr.as_slice())
        .bind(params.cursor_block)
        .bind(params.cursor_log)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let next_cursor = rows.last().map(|r| (r.block_number, r.log_index));

        let data = rows
            .into_iter()
            .map(|r| TransferDto {
                block_number: r.block_number,
                log_index: r.log_index,
                tx_hash: to_hex(&r.tx_hash),
                token_address: r.token_hash.map(|h| to_hex(&h)).unwrap_or_default(),
                from: to_hex(&r.from_hash),
                to: to_hex(&r.to_hash),
                amount: r.amount.unwrap_or_default(),
                is_finalized: r.block_number <= finalized_boundary,
            })
            .collect();

        Ok(Page {
            data,
            next_cursor_block: next_cursor.map(|c| c.0),
            next_cursor_log: next_cursor.map(|c| c.1),
        })
    }
}
