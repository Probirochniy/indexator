CREATE TABLE IF NOT EXISTS sync_state (
    id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    cursor TEXT NOT NULL,
    last_block_number BIGINT NOT NULL,
    last_block_hash BYTEA NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS addresses (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hash BYTEA NOT NULL UNIQUE CHECK (octet_length(hash) = 20)
);

CREATE TABLE IF NOT EXISTS tokens (
    id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    address_id BIGINT NOT NULL UNIQUE REFERENCES addresses(id),
    symbol TEXT,
    name TEXT,
    decimals SMALLINT
);

CREATE TABLE IF NOT EXISTS transfers (
    block_number BIGINT NOT NULL,
    log_index INT NOT NULL,
    tx_hash BYTEA NOT NULL CHECK (octet_length(tx_hash) = 32),
    token_id INT NOT NULL REFERENCES tokens(id),
    from_address_id BIGINT NOT NULL REFERENCES addresses(id),
    to_address_id BIGINT NOT NULL REFERENCES addresses(id),
    amount NUMERIC(78, 0) NOT NULL,
    
    PRIMARY KEY (block_number, log_index)
);

CREATE INDEX IF NOT EXISTS idx_transfers_from_keyset 
    ON transfers (from_address_id, block_number DESC, log_index DESC);

CREATE INDEX IF NOT EXISTS idx_transfers_to_keyset 
    ON transfers (to_address_id, block_number DESC, log_index DESC);

CREATE INDEX IF NOT EXISTS idx_transfers_token_keyset 
    ON transfers (token_id, block_number DESC, log_index DESC);

-- for GET /balances/{address}
CREATE TABLE IF NOT EXISTS balances (
    address_id BIGINT NOT NULL REFERENCES addresses(id),
    token_id INT NOT NULL REFERENCES tokens(id),
    amount NUMERIC(78, 0) NOT NULL DEFAULT 0,
    PRIMARY KEY (address_id, token_id)
);

-- for reorgs
CREATE TABLE IF NOT EXISTS balance_deltas (
    block_number BIGINT NOT NULL,
    address_id BIGINT NOT NULL REFERENCES addresses(id),
    token_id INT NOT NULL REFERENCES tokens(id),
    delta NUMERIC(78, 0) NOT NULL,
    PRIMARY KEY (block_number, address_id, token_id)
);

CREATE INDEX IF NOT EXISTS idx_balance_deltas_block ON balance_deltas (block_number);