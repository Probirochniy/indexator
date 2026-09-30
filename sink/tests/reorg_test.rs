use bigdecimal::BigDecimal;
use sink::address_cache::AddressCache;
use sink::processor::{RawTransfer, process_block, process_undo};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;

fn make_transfer(
    from: &[u8; 20],
    to: &[u8; 20],
    token: &[u8; 20],
    amount: &str,
    block: i64,
    log_idx: i32,
) -> RawTransfer {
    RawTransfer {
        token: token.to_vec(),
        from: from.to_vec(),
        to: to.to_vec(),
        amount: amount.to_string(),
        tx_hash: vec![0xaa; 32],
        block_number: block,
        log_index: log_idx,
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn test_reorg_multiple_blocks_rollback_cleanly(pool: PgPool) {
    let cache = Arc::new(AddressCache::new(10_000));

    let zero_addr = [0u8; 20];
    let alice = [0x11u8; 20];
    let token = [0x22u8; 20];

    let t1 = make_transfer(&zero_addr, &alice, &token, "500", 101, 1);
    process_block(&pool, &cache, 101, 100, &[t1], "cursor_101")
        .await
        .unwrap();

    let t2 = make_transfer(&zero_addr, &alice, &token, "300", 102, 1);
    process_block(&pool, &cache, 102, 100, &[t2], "cursor_102")
        .await
        .unwrap();

    let bal_before: BigDecimal = sqlx::query_scalar(
        r#"
        SELECT b.amount FROM balances b
        JOIN addresses a ON b.account_id = a.id
        WHERE a.hash = $1
        "#,
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(bal_before, BigDecimal::from_str("800").unwrap());

    process_undo(&pool, 100, "cursor_100").await.unwrap();

    let bal_after: BigDecimal = sqlx::query_scalar(
        r#"
        SELECT b.amount FROM balances b
        JOIN addresses a ON b.account_id = a.id
        WHERE a.hash = $1
        "#,
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(bal_after, BigDecimal::from(0));

    let tx_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transfers WHERE block_number > 100")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tx_count, 0);

    let delta_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM balance_deltas WHERE block_number > 100")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(delta_count, 0);

    let (last_block, cursor): (i64, String) =
        sqlx::query_as("SELECT last_block_number, cursor FROM sync_state WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(last_block, 100);
    assert_eq!(cursor, "cursor_100");
}
