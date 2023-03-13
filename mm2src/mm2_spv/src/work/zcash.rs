pub struct ZcashDifficultyParams {
    /// Time when BIP16 becomes active.
    /// See https://github.com/bitcoin/bips/blob/master/bip-0016.mediawiki
    pub bip16_time: u32,
    /// Block height at which BIP34 becomes active.
    /// See https://github.com/bitcoin/bips/blob/master/bip-0034.mediawiki
    pub bip34_height: u32,
    /// Block height at which BIP65 becomes active.
    /// See https://github.com/bitcoin/bips/blob/master/bip-0065.mediawiki
    pub bip65_height: u32,
    /// Block height at which BIP65 becomes active.
    /// See https://github.com/bitcoin/bips/blob/master/bip-0066.mediawiki
    pub bip66_height: u32,
    /// Version bits activation
    pub rule_change_activation_threshold: u32,
    /// Number of blocks with the same set of rules
    pub miner_confirmation_window: u32,

    /// Height of Overwinter activation.
    /// Details: https://zcash.readthedocs.io/en/latest/rtd_pages/nu_dev_guide.html#overwinter
    pub overwinter_height: u32,
    /// Height of Sapling activation.
    /// Details: https://zcash.readthedocs.io/en/latest/rtd_pages/nu_dev_guide.html#sapling
    pub sapling_height: u32,

    /// Interval (in blocks) to calculate average work.
    pub pow_averaging_window: u32,
    /// % of possible down adjustment of work.
    pub pow_max_adjust_down: u32,
    /// % of possible up adjustment of work.
    pub pow_max_adjust_up: u32,
    /// Optimal blocks interval (in seconds).
    pub pow_target_spacing: u32,
    /// Allow minimal difficulty after block at given height.
    pub pow_allow_min_difficulty_after_height: Option<u32>,

    /// 'Slow start' interval parameter.
    ///
    /// For details on how (and why) Zcash 'slow start' works, refer to:
    /// https://z.cash/support/faq/#what-is-slow-start-mining
    /// https://github.com/zcash/zcash/issues/762
    pub subsidy_slow_start_interval: u32,
    /// Block subsidy halving interval.
    ///
    /// Block subsidy is halved every `subsidy_halving_interval` blocks.
    /// There are 64 halving intervals in total.
    pub subsidy_halving_interval: u32,
}

impl ZcashDifficultyParams {
    fn _new() -> Self {
        Self {
            bip16_time: 0,
            bip34_height: 1,
            bip65_height: 0,
            bip66_height: 0,
            rule_change_activation_threshold: 1916, // 95%
            miner_confirmation_window: 2016,
            overwinter_height: 347500,
            sapling_height: 419200,
            pow_max_adjust_down: 32,
            pow_max_adjust_up: 16,
            pow_averaging_window: 17,
            pow_target_spacing: (2.5 * 60.0) as u32,
            pow_allow_min_difficulty_after_height: None,
            subsidy_slow_start_interval: 20_000,
            subsidy_halving_interval: 840_000,
        }
    }
}
