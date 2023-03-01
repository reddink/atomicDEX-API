use common::block_on;
use mm2_number::BigDecimal;
use mm2_test_helpers::for_tests::{enable_tendermint, my_balance, nucleus_iris_ibc_testnet_conf, nucleus_testnet_conf,
                                  orderbook, orderbook_v2, set_price, MarketMakerIt, Mm2TestConf};
use mm2_test_helpers::structs::{OrderbookAddress, OrderbookResponse, OrderbookV2Response, RpcV2Response,
                                TendermintActivationResult};
use serde_json::{self, json};

const IRIS_TESTNET_RPCS: &[&str] = &["http://65.109.231.159:26657"];
const NUCLEUS_TICKER: &str = "NUCLEUS-TEST";
const IRIS_IBC_TICKER: &str = "IRIS-IBC-NUCLEUS-TEST";
const ACTIVATION_SEED: &str = "test_nucleus_with_iris_ibc_activation_balance_orderbook";

#[test]
fn test_nucleus_with_iris_ibc_activation_balance_orderbook() {
    let coins = json!([nucleus_testnet_conf(), nucleus_iris_ibc_testnet_conf()]);

    let conf = Mm2TestConf::seednode(ACTIVATION_SEED, &coins);
    let mm = MarketMakerIt::start(conf.conf, conf.rpc_password, None).unwrap();

    let activation_result = block_on(enable_tendermint(
        &mm,
        NUCLEUS_TICKER,
        &[IRIS_IBC_TICKER],
        IRIS_TESTNET_RPCS,
        false,
    ));

    let response: RpcV2Response<TendermintActivationResult> = serde_json::from_value(activation_result).unwrap();

    let expected_address = "nuc1s9vjmdq3sy5708gnc2vzsmzqz476rvsm47up4e";
    assert_eq!(response.result.address, expected_address);

    let expected_nucleus_balance = BigDecimal::from(3);
    assert_eq!(response.result.balance.spendable, expected_nucleus_balance);

    let expected_iris_ibc_balance: BigDecimal = "0.5".parse().unwrap();

    let actual_iris_ibc_balance = response.result.tokens_balances.get(IRIS_IBC_TICKER).unwrap();
    assert_eq!(actual_iris_ibc_balance.spendable, expected_iris_ibc_balance);

    let actual_iris_ibc_balance = block_on(my_balance(&mm, IRIS_IBC_TICKER)).balance;
    assert_eq!(actual_iris_ibc_balance, expected_iris_ibc_balance);

    let set_price_res = block_on(set_price(&mm, IRIS_IBC_TICKER, NUCLEUS_TICKER, "1", "0.1", false));
    println!("{:?}", set_price_res);

    let set_price_res = block_on(set_price(&mm, NUCLEUS_TICKER, IRIS_IBC_TICKER, "1", "0.1", false));
    println!("{:?}", set_price_res);

    let orderbook = block_on(orderbook(&mm, IRIS_IBC_TICKER, NUCLEUS_TICKER));
    let orderbook: OrderbookResponse = serde_json::from_value(orderbook).unwrap();

    let first_ask = orderbook.asks.first().unwrap();
    assert_eq!(first_ask.address, expected_address);

    let first_bid = orderbook.bids.first().unwrap();
    assert_eq!(first_bid.address, expected_address);

    let orderbook_v2 = block_on(orderbook_v2(&mm, IRIS_IBC_TICKER, NUCLEUS_TICKER));
    let orderbook_v2: RpcV2Response<OrderbookV2Response> = serde_json::from_value(orderbook_v2).unwrap();

    let expected_address = OrderbookAddress::Transparent(expected_address.into());
    let first_ask = orderbook_v2.result.asks.first().unwrap();
    assert_eq!(first_ask.entry.address, expected_address);

    let first_bid = orderbook_v2.result.bids.first().unwrap();
    assert_eq!(first_bid.entry.address, expected_address);
}
