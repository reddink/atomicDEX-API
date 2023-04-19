mod bch_and_slp_tests;
mod best_orders_tests;
mod eth_tests;
mod iris_swap;
mod lightning_tests;
mod lp_bot_tests;
mod mm2_tests_inner;
mod orderbook_sync_tests;
mod tendermint_ibc_asset_tests;
mod tendermint_tests;
mod z_coin_tests;

use mm2_test_helpers::for_tests::MarketMakerIt;
use mm2_test_helpers::structs::CoinActivationResult;

// dummy test helping IDE to recognize this as test module
#[test]
#[allow(clippy::assertions_on_constants)]
fn dummy() { assert!(true) }

#[cfg(all(feature = "zhtlc-native-tests", not(target_arch = "wasm32")))]
async fn enable_z_coin(mm: &MarketMakerIt, coin: &str) -> CoinActivationResult {
    use common::executor::Timer;
    use common::now_ms;
    use mm2_test_helpers::for_tests::{init_z_coin_native, init_z_coin_status};
    use mm2_test_helpers::structs::{InitTaskResult, InitZcoinStatus, RpcV2Response};

    let init = init_z_coin_native(mm, coin).await;
    let init: RpcV2Response<InitTaskResult> = serde_json::from_value(init).unwrap();
    let timeout = now_ms() + 120000;

    loop {
        if gstuff::now_ms() > timeout {
            panic!("{} initialization timed out", coin);
        }

        let status = init_z_coin_status(mm, init.result.task_id).await;
        let status: RpcV2Response<InitZcoinStatus> = serde_json::from_value(status).unwrap();
        match status.result {
            InitZcoinStatus::Ok(result) => break result,
            InitZcoinStatus::Error(e) => panic!("{} initialization error {:?}", coin, e),
            _ => Timer::sleep(1.).await,
        }
    }
}
