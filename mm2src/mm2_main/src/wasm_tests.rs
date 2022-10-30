use super::lp_init;
use common::executor::{spawn, Timer};
use common::log::wasm_log::register_wasm_log;
use mm2_core::mm_ctx::MmArc;
use mm2_test_helpers::for_tests::{enable_electrum, morty_conf, rick_conf, start_swaps, MarketMakerIt, Mm2TestConf,
                                  MORTY, MORTY_ELECTRUMS_WS, RICK, RICK_ELECTRUMS_WS};
use mm2_test_helpers::get_passphrase;
use serde_json::json;
use wasm_bindgen_test::wasm_bindgen_test;

/// Starts the WASM version of MM.
fn wasm_start(ctx: MmArc) {
    spawn(async move {
        lp_init(ctx).await.unwrap();
    })
}

/// This function runs Alice and Bob nodes, activates coins, starts swaps,
/// and then immediately stops the nodes to check if `MmArc` is dropped in a short period.
async fn test_mm2_stops_impl(
    pairs: &[(&'static str, &'static str)],
    maker_price: i32,
    taker_price: i32,
    volume: f64,
    stop_timeout_ms: u64,
) {
    let coins = json!([rick_conf(), morty_conf()]);

    let bob_passphrase = get_passphrase!(".env.seed", "BOB_PASSPHRASE").unwrap();
    let alice_passphrase = get_passphrase!(".env.client", "ALICE_PASSPHRASE").unwrap();

    let bob_conf = Mm2TestConf::seednode(&bob_passphrase, &coins);
    let mut mm_bob = MarketMakerIt::start_async(bob_conf.conf, bob_conf.rpc_password, Some(wasm_start))
        .await
        .unwrap();
    let (_bob_dump_log, _bob_dump_dashboard) = mm_bob.mm_dump();
    Timer::sleep(1.).await;

    let alice_conf = Mm2TestConf::light_node(&alice_passphrase, &coins, &[&mm_bob.my_seed_addr()]);
    let mut mm_alice = MarketMakerIt::start_async(alice_conf.conf, alice_conf.rpc_password, Some(wasm_start))
        .await
        .unwrap();
    let (_alice_dump_log, _alice_dump_dashboard) = mm_alice.mm_dump();

    // Enable coins on Bob side. Print the replies in case we need the address.
    let rc = enable_electrum(&mm_bob, RICK, true, RICK_ELECTRUMS_WS).await;
    log!("enable RICK (bob): {:?}", rc);

    let rc = enable_electrum(&mm_bob, MORTY, true, MORTY_ELECTRUMS_WS).await;
    log!("enable MORTY (bob): {:?}", rc);

    // Enable coins on Alice side. Print the replies in case we need the address.
    let rc = enable_electrum(&mm_alice, RICK, true, RICK_ELECTRUMS_WS).await;
    log!("enable RICK (bob): {:?}", rc);

    let rc = enable_electrum(&mm_alice, MORTY, true, MORTY_ELECTRUMS_WS).await;
    log!("enable MORTY (bob): {:?}", rc);

    start_swaps(&mut mm_bob, &mut mm_alice, pairs, maker_price, taker_price, volume).await;

    mm_alice
        .stop_and_wait_for_ctx_is_dropped(stop_timeout_ms)
        .await
        .unwrap();
    mm_bob.stop_and_wait_for_ctx_is_dropped(stop_timeout_ms).await.unwrap();
}

#[wasm_bindgen_test]
async fn test_mm2_stops_immediately() {
    const STOP_TIMEOUT_MS: u64 = 1000;

    register_wasm_log();

    let pairs: &[_] = &[("RICK", "MORTY")];
    test_mm2_stops_impl(pairs, 1, 1, 0.0001, STOP_TIMEOUT_MS).await;
}

#[wasm_bindgen_test]
async fn trade_test_rick_and_morty() {
    // let pairs: &[_] = &[("RICK", "MORTY")];
    // trade_base_rel_electrum(pairs, 1, 1, 0.0001).await;
}
