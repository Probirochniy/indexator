use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use bigdecimal::BigDecimal;
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
    transfers: &[RawTransfer],
    cursor: &str,
) -> anyhow::Result<()> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    let mut all_hashes = HashSet::new();
    for t in transfers {
        all_hashes.insert(t.from.clone());
        all_hashes.insert(t.to.clone());
        all_hashes.insert(t.token.clone());
    }
    let addr_map = cache.resolve_addresses(all_hashes, &mut tx).await?;

    let mut deltas: HashMap<(i64, i64), BigDecimal> = HashMap::new();
    let zero_addr = vec![0u8; 20]; // mint or burn address

    for t in transfers {
        let token_id = *addr_map.get(&t.token).unwrap();
        let from_id = *addr_map.get(&t.from).unwrap();
        let to_id = *addr_map.get(&t.to).unwrap();
        let amt = BigDecimal::from_str(&t.amount)?;

        if t.from != zero_addr {
            let entry = deltas.entry((from_id, token_id)).or_insert(BigDecimal::from(0));
            *entry = &*entry - &amt;
        }

        if t.to != zero_addr {
            let entry = deltas.entry((to_id, token_id)).or_insert(BigDecimal::from(0));
            *entry = &*entry + &amt;
        }

        sqlx::query(
            r#"
            INSERT INTO transfers 
                (block_number, log_index, tx_hash, token_id, from_address_id, to_address_id, amount)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (block_number, log_index) DO NOTHING;
            "#,
        )
        .bind(block_number)
        .bind(t.log_index)
        .bind(&t.tx_hash)
        .bind(token_id)
        .bind(from_id)
        .bind(to_id)
        .bind(&amt)
        .execute(&mut *tx)
        .await?;
    }

    for ((address_id, token_id), delta) in deltas {
        sqlx::query(
            r#"
            INSERT INTO balance_deltas (block_number, address_id, token_id, delta)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (block_number, address_id, token_id) DO UPDATE SET delta = EXCLUDED.delta;
            "#,
        )
        .bind(block_number)
        .bind(address_id)
        .bind(token_id)
        .bind(&delta)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO balances (address_id, token_id, amount)
            VALUES ($1, $2, $3)
            ON CONFLICT (address_id, token_id) 
            DO UPDATE SET amount = balances.amount + EXCLUDED.amount;
            "#,
        )
        .bind(address_id)
        .bind(token_id)
        .bind(&delta)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO sync_state (id, cursor, last_block_number, last_block_hash, updated_at)
        VALUES (1, $1, $2, $3, NOW())
        ON CONFLICT (id) DO UPDATE 
        SET cursor = EXCLUDED.cursor, last_block_number = EXCLUDED.last_block_number, updated_at = NOW();
        "#,
    )
    .bind(cursor)
    .bind(block_number)
    .bind(&vec![0u8; 32])
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
        UPDATE balances b
        SET amount = b.amount - d.delta
        FROM balance_deltas d
        WHERE b.address_id = d.address_id 
          AND b.token_id = d.token_id 
          AND d.block_number > $1;
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

