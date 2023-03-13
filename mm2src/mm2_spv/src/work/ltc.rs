const RETARGETING_FACTOR: u32 = 4;
const TARGET_SPACING_SECONDS: u32 = 4 * 60;
const TARGET_TIMESPAN_SECONDS: u32 = (3.5 * 24. * 60. * 60.) as u32;

/// The Target number of blocks equals to 2 weeks or 2016 blocks
pub(crate) const RETARGETING_INTERVAL: u32 = TARGET_TIMESPAN_SECONDS / TARGET_SPACING_SECONDS;

/// The upper and lower bounds for retargeting timespan
pub(crate) const MIN_TIMESPAN: i64 = (TARGET_TIMESPAN_SECONDS / RETARGETING_FACTOR) as i64;
pub(crate) const MAX_TIMESPAN: i64 = (TARGET_TIMESPAN_SECONDS * RETARGETING_FACTOR) as i64;

/// The maximum value for bits corresponding to lowest difficulty of 1
pub const MAX_BITS_BTC: u32 = 486604799;

#[test]
fn test_ltc_difficulty_check() {}
