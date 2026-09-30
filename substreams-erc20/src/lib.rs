#[rustfmt::skip]
#[allow(clippy::all, dead_code, unused_imports)]
mod pb; // generated code

use pb::erc20::types::v1::{Transfer, Transfers};
use substreams_ethereum::pb::eth::v2 as eth;

const TRANSFER_TOPIC: [u8; 32] = [
    0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d, 0xaa,
    0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23, 0xb3, 0xef,
];

// substreams wrapper
#[substreams::handlers::map]
fn map_transfers(blk: eth::Block) -> Result<Transfers, substreams::errors::Error> {
    process_transfers(&blk)
}

// buisness logic
pub fn process_transfers(blk: &eth::Block) -> Result<Transfers, substreams::errors::Error> {
    let mut transfers = Vec::new();

    for tx in &blk.transaction_traces {
        if tx.status != 1 {
            continue;
        }

        if let Some(receipt) = &tx.receipt {
            for log in &receipt.logs {
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
                    token_address: log.address.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_block(status: i32, topics: Vec<Vec<u8>>, data: Vec<u8>) -> eth::Block {
        let log = eth::Log {
            address: vec![0x11; 20],
            topics,
            data,
            block_index: 42,
            ..Default::default()
        };

        let tx = eth::TransactionTrace {
            status,
            hash: vec![0xaa; 32],
            receipt: Some(eth::TransactionReceipt {
                logs: vec![log],
                ..Default::default()
            }),
            ..Default::default()
        };

        eth::Block {
            number: 1337,
            transaction_traces: vec![tx],
            ..Default::default()
        }
    }

    fn pad_address(addr_byte: u8) -> Vec<u8> {
        let mut topic = vec![0u8; 12];
        topic.extend_from_slice(&[addr_byte; 20]);
        topic
    }

    #[test]
    fn test_valid_erc20_transfer_works() {
        let from_topic = pad_address(0x01);
        let to_topic = pad_address(0x02);

        let mut data = vec![0u8; 30];
        data.extend_from_slice(&[0x03, 0xe8]); // 1000

        let blk = mock_block(1, vec![TRANSFER_TOPIC.to_vec(), from_topic, to_topic], data);

        let res = process_transfers(&blk).expect("parser should work");
        assert_eq!(res.transfers.len(), 1);

        let t = &res.transfers[0];
        assert_eq!(t.amount, "1000");
        assert_eq!(t.from, vec![0x01; 20]);
        assert_eq!(t.to, vec![0x02; 20]);
        assert_eq!(t.block_number, 1337);
        assert_eq!(t.log_index, 42);
    }

    #[test]
    fn test_failed_tx_ignored() {
        let blk = mock_block(
            0,
            vec![
                TRANSFER_TOPIC.to_vec(),
                pad_address(0x01),
                pad_address(0x02),
            ],
            vec![0u8; 32],
        );

        let res = process_transfers(&blk).unwrap();
        assert_eq!(res.transfers.len(), 0);
    }

    #[test]
    fn test_erc721_nft_transfer_ignored() {
        let blk = mock_block(
            1,
            vec![
                TRANSFER_TOPIC.to_vec(),
                pad_address(0x01),
                pad_address(0x02),
                vec![0x99; 32],
            ],
            vec![],
        );

        let res = process_transfers(&blk).unwrap();
        assert_eq!(res.transfers.len(), 0);
    }

    #[test]
    fn test_foreign_event_ignored() {
        let fake_topic = vec![0x69; 32];

        let blk = mock_block(
            1,
            vec![fake_topic, pad_address(0x01), pad_address(0x02)],
            vec![0u8; 32],
        );

        let res = process_transfers(&blk).unwrap();
        assert_eq!(res.transfers.len(), 0);
    }

    #[test]
    fn test_max_uint256_amount() {
        let max_u256_bytes = vec![0xff; 32];
        let blk = mock_block(
            1,
            vec![
                TRANSFER_TOPIC.to_vec(),
                pad_address(0x01),
                pad_address(0x02),
            ],
            max_u256_bytes,
        );

        let res = process_transfers(&blk).unwrap();
        assert_eq!(
            res.transfers[0].amount,
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
    }
}
