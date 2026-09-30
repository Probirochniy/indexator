use bigdecimal::BigDecimal;
use sink::address_cache::AddressCache;
use sink::processor::{BatchMeta, RawTransfer, process_batch, process_undo};
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

fn meta(last_block: i64, final_block: i64, cursor: &str) -> BatchMeta {
    BatchMeta {
        last_block_number: last_block,
        last_final_block_number: final_block,
        head_block_number: last_block,
        last_cursor: cursor.to_string(),
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn test_reorg_with_alternative_fork_execution(pool: PgPool) {
    let cache = Arc::new(AddressCache::new(10_000));

    let zero_addr = [0u8; 20];
    let alice = [0x11u8; 20];
    let bob = [0x33u8; 20];
    let token = [0x22u8; 20];

    let t1 = make_transfer(&zero_addr, &alice, &token, "500", 101, 1, 0xaa);
    process_batch(&pool, &cache, &[t1], &meta(101, 100, "cursor_101_fork_a"))
        .await
        .unwrap();

    let t2 = make_transfer(&zero_addr, &alice, &token, "300", 102, 1, 0xaa);
    process_batch(&pool, &cache, &[t2], &meta(102, 100, "cursor_102_fork_a"))
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

    process_undo(&pool, 100, "cursor_100").await.unwrap();

    let bal_after_undo: BigDecimal = sqlx::query_scalar(
        "SELECT b.amount FROM balances b JOIN addresses a ON b.account_id = a.id WHERE a.hash = $1",
    )
    .bind(&alice[..])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bal_after_undo, BigDecimal::from(0));

    let t1_b = make_transfer(&zero_addr, &bob, &token, "1000", 101, 1, 0xbb);
    let t2_b = make_transfer(&bob, &alice, &token, "400", 102, 1, 0xbb);

    process_batch(
        &pool,
        &cache,
        &[t1_b, t2_b],
        &meta(102, 100, "cursor_102_fork_b"),
    )
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
    process_batch(&pool, &cache, &[t_init], &meta(200, 200, "cursor_200"))
        .await
        .unwrap();

    let t_reorg = make_transfer(&zero_addr, &alice, &token, "999", 201, 1, 0x22);
    process_batch(
        &pool,
        &cache,
        &[t_reorg],
        &meta(201, 200, "cursor_201_stale"),
    )
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
    process_batch(
        &pool,
        &cache,
        &[t_canon],
        &meta(201, 200, "cursor_201_canon"),
    )
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
