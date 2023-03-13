pub mod btc;
mod ltc;

pub use btc::*;

use crate::conf::SPVBlockHeader;
use crate::storage::{BlockHeaderStorageError, BlockHeaderStorageOps};
use crate::work::btc::{btc_mainnet_next_block_bits, btc_testnet_next_block_bits};
use chain::BlockHeaderBits;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::cmp;

/// The lower bounds for retargeting timespan.
const fn min_timespan(target_timespan_secs: u32, retarget_factor: u32) -> i64 {
    (target_timespan_secs / retarget_factor) as i64
}

/// The lower bound for retargeting timespan.
const fn max_timespan(target_timespan_secs: u32, retarget_factor: u32) -> i64 {
    (target_timespan_secs * retarget_factor) as i64
}

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
    #[serde(rename = "Litecoin")]
    Litecoin,
}

pub async fn next_block_bits(
    coin: &str,
    current_block_timestamp: u32,
    last_block_header: SPVBlockHeader,
    storage: &dyn BlockHeaderStorageOps,
    algorithm: &DifficultyAlgorithm,
) -> Result<BlockHeaderBits, NextBlockBitsError> {
    match algorithm {
        DifficultyAlgorithm::BitcoinMainnet => btc_mainnet_next_block_bits(coin, last_block_header, storage).await,
        DifficultyAlgorithm::BitcoinTestnet => {
            btc_testnet_next_block_bits(coin, current_block_timestamp, last_block_header, storage).await
        },
        DifficultyAlgorithm::Litecoin => todo!(),
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
