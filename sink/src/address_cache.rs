use moka::sync::Cache;
use sqlx::{PgConnection, Row};
use std::collections::{HashMap, HashSet};

pub struct AddressCache {
    cache: Cache<Vec<u8>, i64>,
}

impl AddressCache {
    pub fn new(capacity: u64) -> Self {
        Self {
            cache: Cache::builder().max_capacity(capacity).build(),
        }
    }

    pub async fn resolve_addresses(
        &self,
        hashes: HashSet<Vec<u8>>,
        tx: &mut PgConnection,
    ) -> anyhow::Result<HashMap<Vec<u8>, i64>> {
        let mut result = HashMap::with_capacity(hashes.len());
        let mut missing = Vec::new();

        for hash in hashes {
            if let Some(id) = self.cache.get(&hash) {
                result.insert(hash, id);
            } else {
                missing.push(hash);
            }
        }

        if missing.is_empty() {
            return Ok(result);
        }

        let rows = sqlx::query(
            r#"
            WITH inserted AS (
                INSERT INTO addresses (hash)
                SELECT UNNEST($1::bytea[])
                ON CONFLICT (hash) DO UPDATE SET hash = EXCLUDED.hash
                RETURNING id, hash
            )
            SELECT id, hash FROM inserted;
            "#,
        )
        .bind(&missing)
        .fetch_all(&mut *tx)
        .await?;

        for row in rows {
            let id: i64 = row.get("id");
            let hash: Vec<u8> = row.get("hash");
            self.cache.insert(hash.clone(), id);
            result.insert(hash, id);
        }

        Ok(result)
    }
}
