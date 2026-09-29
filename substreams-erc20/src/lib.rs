mod pb;
use pb::erc20::types::v1::{Transfer, Transfers};
use substreams_ethereum::pb::eth::v2 as eth;

const TRANSFER_TOPIC: [u8; 32] = [
    0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b,
    0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d, 0xaa,
    0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16,
    0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23, 0xb3, 0xef,
];

#[substreams::handlers::map]
fn map_transfers(blk: eth::Block) -> Result<Transfers, substreams::errors::Error> {
    let mut transfers = Vec::new();

    for tx in blk.transaction_traces {
        if tx.status != 1 {
            continue;
        }

        for receipt in tx.receipt {
            for log in receipt.logs {
                if log.topics.is_empty() || log.topics[0] != TRANSFER_TOPIC {
                    continue;
                }

                // not include ERC-721 NFT (4 topics)
                if log.topics.len() != 3 || log.data.len() != 32 {
                    continue;
                }

                // get only the last 20 bytes of the topic for from and to addresses (other are zeros)
                let from = &log.topics[1][12..];
                let to = &log.topics[2][12..];
                let amount = num_bigint::BigUint::from_bytes_be(&log.data).to_string();

                transfers.push(Transfer {
                    token_address: log.address,
                    from: from.to_vec(),
                    to: to.to_vec(),
                    amount,
                    transaction_hash: tx.hash.clone(),
                    block_number: blk.number,
                    log_index: log.block_index,
                });
            }
        }
    }

    Ok(Transfers { transfers })
}