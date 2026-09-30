use bigdecimal::BigDecimal;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

pub struct RawTransfer {
    pub token: Vec<u8>,
    pub from: Vec<u8>,
    pub to: Vec<u8>,
    pub amount: String,
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub log_index: i32,
}

pub async fn process_block(
    pool: &PgPool,
    cache: &crate::address_cache::AddressCache,
    block_number: i64,
    final_block_number: i64,
    transfers: &[RawTransfer],
    cursor: &str,
) -> anyhow::Result<()> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    if !transfers.is_empty() {
        let mut all_hashes = HashSet::new();
        for t in transfers {
            all_hashes.insert(t.from.clone());
            all_hashes.insert(t.to.clone());
            all_hashes.insert(t.token.clone());
        }
        let addr_map = cache.resolve_addresses(all_hashes, &mut tx).await?;

        let mut b_nums = Vec::with_capacity(transfers.len());
        let mut l_idxs = Vec::with_capacity(transfers.len());
        let mut tx_hashes = Vec::with_capacity(transfers.len());
        let mut token_ids = Vec::with_capacity(transfers.len());
        let mut from_ids = Vec::with_capacity(transfers.len());
        let mut to_ids = Vec::with_capacity(transfers.len());
        let mut amounts = Vec::with_capacity(transfers.len());

        let mut deltas: HashMap<(i64, i64), BigDecimal> = HashMap::new();
        let zero_addr = [0u8; 20];

        for t in transfers {
            let token_id = *addr_map.get(&t.token).unwrap();
            let from_id = *addr_map.get(&t.from).unwrap();
            let to_id = *addr_map.get(&t.to).unwrap();
            let amt = BigDecimal::from_str(&t.amount)?;

            b_nums.push(t.block_number);
            l_idxs.push(t.log_index);
            tx_hashes.push(t.tx_hash.clone());
            token_ids.push(token_id);
            from_ids.push(from_id);
            to_ids.push(to_id);
            amounts.push(amt.clone());

            if t.from != zero_addr {
                let entry = deltas
                    .entry((from_id, token_id))
                    .or_insert(BigDecimal::from(0));
                *entry = &*entry - &amt;
            }
            if t.to != zero_addr {
                let entry = deltas
                    .entry((to_id, token_id))
                    .or_insert(BigDecimal::from(0));
                *entry = &*entry + &amt;
            }
        }

        sqlx::query(
            r#"
            INSERT INTO transfers
                (block_number, log_index, tx_hash, token_address_id, from_address_id, to_address_id, amount)
            SELECT * FROM UNNEST(
                $1::bigint[],
                $2::int[],
                $3::bytea[],
                $4::bigint[],
                $5::bigint[],
                $6::bigint[],
                $7::numeric[]
            )
            ON CONFLICT (block_number, log_index) DO NOTHING;
            "#,
        )
        .bind(&b_nums)
        .bind(&l_idxs)
        .bind(&tx_hashes)
        .bind(&token_ids)
        .bind(&from_ids)
        .bind(&to_ids)
        .bind(&amounts)
        .execute(&mut *tx)
        .await?;

        if !deltas.is_empty() {
            let mut d_accs = Vec::with_capacity(deltas.len());
            let mut d_tokens = Vec::with_capacity(deltas.len());
            let mut d_amts = Vec::with_capacity(deltas.len());

            for ((acc_id, tok_id), delta) in deltas {
                d_accs.push(acc_id);
                d_tokens.push(tok_id);
                d_amts.push(delta);
            }

            sqlx::query(
                r#"
                INSERT INTO balance_deltas (block_number, account_id, token_address_id, delta)
                SELECT $1, * FROM UNNEST(
                    $2::bigint[],
                    $3::bigint[],
                    $4::numeric[]
                )
                ON CONFLICT (block_number, account_id, token_address_id)
                DO UPDATE SET delta = EXCLUDED.delta;
                "#,
            )
            .bind(block_number)
            .bind(&d_accs)
            .bind(&d_tokens)
            .bind(&d_amts)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO balances (account_id, token_address_id, amount)
                SELECT * FROM UNNEST(
                    $1::bigint[],
                    $2::bigint[],
                    $3::numeric[]
                )
                ON CONFLICT (account_id, token_address_id)
                DO UPDATE SET amount = balances.amount + EXCLUDED.amount;
                "#,
            )
            .bind(&d_accs)
            .bind(&d_tokens)
            .bind(&d_amts)
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query(
        r#"
        INSERT INTO sync_state (id, cursor, last_block_number, last_block_hash, last_final_block_number, updated_at)
        VALUES (1, $1, $2, $3, $4, NOW())
        ON CONFLICT (id) DO UPDATE
        SET cursor = EXCLUDED.cursor, last_block_number = EXCLUDED.last_block_number, last_final_block_number = EXCLUDED.last_final_block_number, updated_at = NOW();
        "#,
    )
    .bind(cursor)
    .bind(block_number)
    .bind(&[0u8; 32][..])
    .bind(final_block_number)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn process_undo(
    pool: &PgPool,
    last_valid_block: i64,
    last_valid_cursor: &str,
) -> anyhow::Result<()> {
    tracing::warn!("REORG rollback to block {}", last_valid_block);
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        WITH aggregated_deltas AS (
        SELECT
            account_id,
            token_address_id,
            SUM(delta) AS total_delta
        FROM balance_deltas
        WHERE block_number > $1
        GROUP BY account_id, token_address_id
    )
    UPDATE balances b
    SET amount = b.amount - ad.total_delta
    FROM aggregated_deltas ad
    WHERE b.account_id = ad.account_id
      AND b.token_address_id = ad.token_address_id;
    "#,
    )
    .bind(last_valid_block)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM balance_deltas WHERE block_number > $1;")
        .bind(last_valid_block)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM transfers WHERE block_number > $1;")
        .bind(last_valid_block)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        r#"
        UPDATE sync_state
        SET last_block_number = $1, cursor = $2, updated_at = NOW()
        WHERE id = 1;
        "#,
    )
    .bind(last_valid_block)
    .bind(last_valid_cursor)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    tracing::info!("REORG successfully rolled back");
    Ok(())
}
