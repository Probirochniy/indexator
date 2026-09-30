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
    tx_hash_byte: u8,
) -> RawTransfer {
    RawTransfer {
        token: token.to_vec(),
        from: from.to_vec(),
        to: to.to_vec(),
        amount: amount.to_string(),
        tx_hash: vec![tx_hash_byte; 32],
        block_number: block,
        log_index: log_idx,
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn test_reorg_with_alternative_fork_execution(pool: PgPool) {
    let cache = Arc::new(AddressCache::new(10_000));

    let zero_addr = [0u8; 20];
    let alice = [0x11u8; 20];
    let bob = [0x33u8; 20];
    let token = [0x22u8; 20];

    // orphaned fork
    let t1 = make_transfer(&zero_addr, &alice, &token, "500", 101, 1, 0xaa);
    process_block(&pool, &cache, 101, 100, &[t1], "cursor_101_fork_a")
        .await
        .unwrap();

    let t2 = make_transfer(&zero_addr, &alice, &token, "300", 102, 1, 0xaa);
    process_block(&pool, &cache, 102, 100, &[t2], "cursor_102_fork_a")
        .await
        .unwrap();

    let bal_a: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal_a, BigDecimal::from_str("800").unwrap());

    // REORG
    process_undo(&pool, 100, "cursor_100").await.unwrap();

    let bal_after_undo: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal_after_undo, BigDecimal::from(0));

    // canonical chain
    let t1_b = make_transfer(&zero_addr, &bob, &token, "1000", 101, 1, 0xbb);
    process_block(&pool, &cache, 101, 100, &[t1_b], "cursor_101_fork_b")
        .await
        .unwrap();

    let t2_b = make_transfer(&bob, &alice, &token, "400", 102, 1, 0xbb);
    process_block(&pool, &cache, 102, 100, &[t2_b], "cursor_102_fork_b")
        .await
        .unwrap();

    let bal_alice: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal_alice, BigDecimal::from_str("400").unwrap());

    let bal_bob: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&bob[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal_bob, BigDecimal::from_str("600").unwrap());

    let tx_hashes: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT tx_hash FROM transfers WHERE block_number > 100 ORDER BY block_number, log_index",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(tx_hashes.len(), 2);
    assert_eq!(tx_hashes[0], vec![0xbb; 32]);
    assert_eq!(tx_hashes[1], vec![0xbb; 32]);

    let (last_block, cursor): (i64, String) =
        sqlx::query_as("SELECT last_block_number, cursor FROM sync_state WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(last_block, 102);
    assert_eq!(cursor, "cursor_102_fork_b");
}

#[sqlx::test(migrations = "../migrations")]
async fn test_reorg_on_cursor_restart_boundary(pool: PgPool) {
    let cache = Arc::new(AddressCache::new(10_000));

    let zero_addr = [0u8; 20];
    let alice = [0x11u8; 20];
    let token = [0x22u8; 20];

    let t_init = make_transfer(&zero_addr, &alice, &token, "100", 200, 1, 0x11);
    process_block(&pool, &cache, 200, 200, &[t_init], "cursor_200")
        .await
        .unwrap();

    let t_reorg = make_transfer(&zero_addr, &alice, &token, "999", 201, 1, 0x22);
    process_block(&pool, &cache, 201, 200, &[t_reorg], "cursor_201_stale")
        .await
        .unwrap();

    let (saved_cursor, saved_block): (String, i64) =
        sqlx::query_as("SELECT cursor, last_block_number FROM sync_state WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(saved_block, 201);
    assert_eq!(saved_cursor, "cursor_201_stale");

    process_undo(&pool, 200, "cursor_200").await.unwrap();

    let (curr_cursor, curr_block): (String, i64) =
        sqlx::query_as("SELECT cursor, last_block_number FROM sync_state WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(curr_block, 200);
    assert_eq!(curr_cursor, "cursor_200");

    let bal: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal, BigDecimal::from_str("100").unwrap());

    let t_canon = make_transfer(&zero_addr, &alice, &token, "50", 201, 1, 0x33);
    process_block(&pool, &cache, 201, 200, &[t_canon], "cursor_201_canon")
        .await
        .unwrap();

    let final_bal: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(final_bal, BigDecimal::from_str("150").unwrap());

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transfers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
