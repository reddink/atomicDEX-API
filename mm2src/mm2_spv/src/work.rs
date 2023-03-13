pub mod btc;
pub mod zcash;

pub use btc::*;
pub use zcash::*;

use crate::conf::SPVBlockHeader;
use crate::storage::{BlockHeaderStorageError, BlockHeaderStorageOps};
use crate::work::btc::{btc_mainnet_next_block_bits, btc_testnet_next_block_bits};
use chain::BlockHeaderBits;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::cmp;

/// The maximum value for bits corresponding to lowest difficulty of 1
pub(crate) const LTC_MAX_BITS: u32 = 504365040;

/// The Target number of blocks equals to 2 weeks or 2016 blocks (for btc).
const fn retarget_interval(target_timespan_secs: u32, target_spacing_secs: u32) -> u32 {
    target_timespan_secs / target_spacing_secs
}

const fn is_retarget_height(height: u64, target_timespan_secs: u32, target_spacing_secs: u32) -> bool {
    height % retarget_interval(target_timespan_secs, target_spacing_secs) as u64 == 0
}

#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub enum NextBlockBitsError {
    #[display(fmt = "Block headers storage error: {}", _0)]
    StorageError(BlockHeaderStorageError),
    #[display(fmt = "Can't find Block header for {} with height {}", coin, height)]
    NoSuchBlockHeader { coin: String, height: u64 },
    #[display(fmt = "Can't find a Block header for {} with no max bits", coin)]
    NoBlockHeaderWithNoMaxBits { coin: String },
}

impl From<BlockHeaderStorageError> for NextBlockBitsError {
    fn from(e: BlockHeaderStorageError) -> Self { NextBlockBitsError::StorageError(e) }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum DifficultyAlgorithm {
    #[serde(rename = "Bitcoin Mainnet")]
    BitcoinMainnet,
    #[serde(rename = "Bitcoin Testnet")]
    BitcoinTestnet,
    Litecoin,
    Zcash,
}

impl DifficultyAlgorithm {
    pub(crate) fn max_bits(&self) -> u32 {
        match self {
            DifficultyAlgorithm::BitcoinMainnet | DifficultyAlgorithm::BitcoinTestnet => MAX_BITS_BTC,
            DifficultyAlgorithm::Litecoin => LTC_MAX_BITS,
            _ => todo!(),
        }
    }

    pub(crate) fn min_max_timespan(&self) -> (i64, i64) {
        match self {
            DifficultyAlgorithm::BitcoinMainnet | DifficultyAlgorithm::BitcoinTestnet => (MIN_TIMESPAN, MAX_TIMESPAN),
            DifficultyAlgorithm::Litecoin => (MIN_TIMESPAN / 4, MAX_TIMESPAN / 4),
            _ => todo!(),
        }
    }

    pub(crate) fn target_timespan_spacing_secs(&self) -> (u32, u32) {
        match self {
            DifficultyAlgorithm::BitcoinMainnet | DifficultyAlgorithm::BitcoinTestnet => {
                (TARGET_TIMESPAN_SECONDS, TARGET_SPACING_SECONDS)
            },
            DifficultyAlgorithm::Litecoin => (TARGET_TIMESPAN_SECONDS / 4, TARGET_SPACING_SECONDS / 4),
            _ => todo!(),
        }
    }
}

pub async fn next_block_bits(
    coin: &str,
    current_block_timestamp: u32,
    last_block_header: SPVBlockHeader,
    storage: &dyn BlockHeaderStorageOps,
    algorithm: &DifficultyAlgorithm,
) -> Result<BlockHeaderBits, NextBlockBitsError> {
    match algorithm {
        DifficultyAlgorithm::BitcoinMainnet | DifficultyAlgorithm::Litecoin => {
            btc_mainnet_next_block_bits(coin, last_block_header, storage, algorithm).await
        },
        DifficultyAlgorithm::BitcoinTestnet => {
            btc_testnet_next_block_bits(coin, current_block_timestamp, last_block_header, storage, algorithm).await
        },
        _ => todo!(),
    }
}

fn range_constrain(value: i64, min: i64, max: i64) -> i64 { cmp::min(cmp::max(value, min), max) }

/// Returns constrained number of seconds since last retarget
fn retarget_timespan(retarget_timestamp: u32, last_timestamp: u32, min_ts: i64, max_ts: i64) -> u32 {
    // subtract unsigned 32 bit numbers in signed 64 bit space in
    // order to prevent underflow before applying the range constraint.
    let timespan = last_timestamp as i64 - retarget_timestamp as i64;
    range_constrain(timespan, min_ts, max_ts) as u32
}
