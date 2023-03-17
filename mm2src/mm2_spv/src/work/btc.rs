use crate::conf::SPVBlockHeader;
use crate::storage::BlockHeaderStorageOps;
use crate::work::{is_retarget_height, retarget_timespan, DifficultyAlgorithm, NextBlockBitsError};
use chain::BlockHeaderBits;
use primitives::compact::Compact;
use primitives::U256;

const RETARGETING_FACTOR: u32 = 4;
pub(crate) const TARGET_SPACING_SECONDS: u32 = 10 * 60;
pub(crate) const TARGET_TIMESPAN_SECONDS: u32 = 2 * 7 * 24 * 60 * 60;

/// The Target number of blocks equals to 2 weeks or 2016 blocks
pub(crate) const RETARGETING_INTERVAL: u32 = TARGET_TIMESPAN_SECONDS / TARGET_SPACING_SECONDS;

/// The upper and lower bounds for retargeting timespan
pub(crate) const MIN_TIMESPAN: i64 = (TARGET_TIMESPAN_SECONDS / RETARGETING_FACTOR) as i64;
pub(crate) const MAX_TIMESPAN: i64 = (TARGET_TIMESPAN_SECONDS * RETARGETING_FACTOR) as i64;

/// The maximum value for bits corresponding to lowest difficulty of 1
pub const MAX_BITS_BTC: u32 = 486604799;

pub(crate) async fn btc_retarget_bits(
    coin: &str,
    last_block_header: SPVBlockHeader,
    storage: &dyn BlockHeaderStorageOps,
    algorithm: &DifficultyAlgorithm,
) -> Result<BlockHeaderBits, NextBlockBitsError> {
    let max_bits_compact: Compact = algorithm.max_bits().into();

    let retarget_ref = last_block_header.height + 1 - RETARGETING_INTERVAL as u64;
    if retarget_ref == 0 {
        return Ok(BlockHeaderBits::Compact(max_bits_compact));
    }
    let retarget_header = storage
        .get_block_header(retarget_ref)
        .await?
        .map(|h| SPVBlockHeader::from_block_header_and_height(&h, retarget_ref))
        .ok_or(NextBlockBitsError::NoSuchBlockHeader {
            coin: coin.into(),
            height: retarget_ref,
        })?;

    // timestamp of block(height - RETARGETING_INTERVAL)
    let retarget_timestamp = retarget_header.time;
    // timestamp of last block
    let last_timestamp = last_block_header.time;
    let (min_timespan, max_timespan) = algorithm.min_max_timespan();

    let retarget: Compact = last_block_header.bits.into();
    let retarget: U256 = retarget.into();
    let retarget_timespan: U256 =
        retarget_timespan(retarget_timestamp, last_timestamp, min_timespan, max_timespan).into();
    let retarget: U256 = retarget * retarget_timespan;
    let (target_timespan_seconds, _) = algorithm.target_timespan_spacing_secs();
    let retarget = retarget / target_timespan_seconds;

    let max_bits: U256 = max_bits_compact.into();
    if retarget > max_bits {
        Ok(BlockHeaderBits::Compact(max_bits_compact))
    } else {
        Ok(BlockHeaderBits::Compact(retarget.into()))
    }
}

pub(crate) async fn btc_mainnet_next_block_bits(
    coin: &str,
    last_block_header: SPVBlockHeader,
    storage: &dyn BlockHeaderStorageOps,
    algorithm: &DifficultyAlgorithm,
) -> Result<BlockHeaderBits, NextBlockBitsError> {
    let max_bits_compact: Compact = algorithm.max_bits().into();
    if last_block_header.height == 0 {
        return Ok(BlockHeaderBits::Compact(max_bits_compact));
    }

    let next_height = last_block_header.height + 1;
    let last_block_bits = last_block_header.bits.clone();

    let (target_timespan_secs, target_spacing_secs) = algorithm.target_timespan_spacing_secs();

    if is_retarget_height(next_height, target_timespan_secs, target_spacing_secs) {
        btc_retarget_bits(coin, last_block_header, storage, algorithm).await
    } else {
        Ok(last_block_bits)
    }
}

pub(crate) async fn btc_testnet_next_block_bits(
    coin: &str,
    current_block_timestamp: u32,
    last_block_header: SPVBlockHeader,
    storage: &dyn BlockHeaderStorageOps,
    algorithm: &DifficultyAlgorithm,
) -> Result<BlockHeaderBits, NextBlockBitsError> {
    let max_bits = BlockHeaderBits::Compact(MAX_BITS_BTC.into());
    if last_block_header.height == 0 {
        return Ok(max_bits);
    }

    let next_height = last_block_header.height + 1;
    let last_block_bits = last_block_header.bits.clone();
    let max_time_gap = last_block_header.time + 2 * TARGET_SPACING_SECONDS;

    if is_retarget_height(next_height, TARGET_TIMESPAN_SECONDS, TARGET_SPACING_SECONDS) {
        btc_retarget_bits(coin, last_block_header, storage, algorithm).await
    } else if current_block_timestamp > max_time_gap {
        Ok(max_bits)
    } else if last_block_bits != max_bits {
        Ok(last_block_bits.clone())
    } else {
        let last_non_max_bits = storage
            .get_last_block_header_with_non_max_bits(MAX_BITS_BTC)
            .await?
            .map(|header| header.bits)
            .unwrap_or(max_bits);
        Ok(last_non_max_bits)
    }
}
