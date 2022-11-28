use common::block_on;
use mm2_number::BigDecimal;
use mm2_test_helpers::for_tests::{atom_testnet_conf, enable_tendermint, enable_tendermint_token,
                                  get_tendermint_my_tx_history, iris_nimda_testnet_conf, iris_testnet_conf,
                                  my_balance, send_raw_transaction, withdraw_v1, MarketMakerIt, Mm2TestConf};
use mm2_test_helpers::structs::{MyBalanceResponse, RpcV2Response, TendermintActivationResult, TransactionDetails,
                                TransactionType};
use serde_json::{self as json, json};
use std::mem::discriminant;
use std::str::FromStr;

const ATOM_TEST_BALANCE_SEED: &str = "atom test seed";
const ATOM_TEST_WITHDRAW_SEED: &str = "atom test withdraw seed";
const ATOM_TICKER: &str = "ATOM";
const ATOM_TENDERMINT_RPC_URLS: &[&str] = &["https://cosmos-testnet-rpc.allthatnode.com:26657"];

const IRIS_TESTNET_RPC_URLS: &[&str] = &["http://34.80.202.172:26657"];

#[test]
fn test_tendermint_activation_and_balance() {
    let coins = json!([atom_testnet_conf()]);
    let expected_address = "cosmos1svaw0aqc4584x825ju7ua03g5xtxwd0ahl86hz";

    let conf = Mm2TestConf::seednode(ATOM_TEST_BALANCE_SEED, &coins);
    let mm = MarketMakerIt::start(conf.conf, conf.rpc_password, None).unwrap();

    let activation_result = block_on(enable_tendermint(
        &mm,
        ATOM_TICKER,
        &[],
        ATOM_TENDERMINT_RPC_URLS,
        false,
    ));

    let result: RpcV2Response<TendermintActivationResult> = json::from_value(activation_result).unwrap();
    assert_eq!(result.result.address, expected_address);
    let expected_balance: BigDecimal = "0.0959".parse().unwrap();
    assert_eq!(result.result.balance.spendable, expected_balance);

    let my_balance_result = block_on(my_balance(&mm, ATOM_TICKER));
    let my_balance: MyBalanceResponse = json::from_value(my_balance_result).unwrap();

    assert_eq!(my_balance.balance, expected_balance);
    assert_eq!(my_balance.unspendable_balance, BigDecimal::default());
    assert_eq!(my_balance.address, expected_address);
    assert_eq!(my_balance.coin, ATOM_TICKER);
}

#[test]
fn test_tendermint_withdraw() {
    let coins = json!([atom_testnet_conf()]);

    let conf = Mm2TestConf::seednode(ATOM_TEST_WITHDRAW_SEED, &coins);
    let mm = MarketMakerIt::start(conf.conf, conf.rpc_password, None).unwrap();

    let activation_res = block_on(enable_tendermint(
        &mm,
        ATOM_TICKER,
        &[],
        ATOM_TENDERMINT_RPC_URLS,
        false,
    ));
    println!("Activation {}", json::to_string(&activation_res).unwrap());

    // just call withdraw without sending to check response correctness
    let withdraw_result = block_on(withdraw_v1(
        &mm,
        ATOM_TICKER,
        "cosmos1svaw0aqc4584x825ju7ua03g5xtxwd0ahl86hz",
        "0.1",
    ));
    println!("Withdraw to other {}", json::to_string(&withdraw_result).unwrap());
    let tx_details: TransactionDetails = json::from_value(withdraw_result).unwrap();
    // TODO how to check it if the fee is dynamic?
    /*
    let expected_total: BigDecimal = "0.15".parse().unwrap();
    assert_eq!(tx_details.total_amount, expected_total);
    assert_eq!(tx_details.spent_by_me, expected_total);
    assert_eq!(tx_details.my_balance_change, expected_total * BigDecimal::from(-1));
    */
    assert_eq!(tx_details.received_by_me, BigDecimal::default());
    assert_eq!(tx_details.to, vec![
        "cosmos1svaw0aqc4584x825ju7ua03g5xtxwd0ahl86hz".to_owned()
    ]);
    assert_eq!(tx_details.from, vec![
        "cosmos1w5h6wud7a8zpa539rc99ehgl9gwkad3wjsjq8v".to_owned()
    ]);

    // withdraw and send transaction to ourselves
    let withdraw_result = block_on(withdraw_v1(
        &mm,
        ATOM_TICKER,
        "cosmos1w5h6wud7a8zpa539rc99ehgl9gwkad3wjsjq8v",
        "0.1",
    ));
    println!("Withdraw to self {}", json::to_string(&withdraw_result).unwrap());

    let tx_details: TransactionDetails = json::from_value(withdraw_result).unwrap();
    // TODO how to check it if the fee is dynamic?
    /*
    let expected_total: BigDecimal = "0.15".parse().unwrap();
    let expected_balance_change: BigDecimal = "-0.05".parse().unwrap();
    assert_eq!(tx_details.total_amount, expected_total);
    assert_eq!(tx_details.spent_by_me, expected_total);
    assert_eq!(tx_details.my_balance_change, expected_balance_change);
     */
    let expected_received: BigDecimal = "0.1".parse().unwrap();
    assert_eq!(tx_details.received_by_me, expected_received);

    assert_eq!(tx_details.to, vec![
        "cosmos1w5h6wud7a8zpa539rc99ehgl9gwkad3wjsjq8v".to_owned()
    ]);
    assert_eq!(tx_details.from, vec![
        "cosmos1w5h6wud7a8zpa539rc99ehgl9gwkad3wjsjq8v".to_owned()
    ]);

    let send_raw_tx = block_on(send_raw_transaction(&mm, ATOM_TICKER, &tx_details.tx_hex));
    println!("Send raw tx {}", json::to_string(&send_raw_tx).unwrap());
}

#[test]
fn test_tendermint_token_activation_and_withdraw() {
    const TEST_SEED: &str = "iris test seed";
    let coins = json!([iris_testnet_conf(), iris_nimda_testnet_conf()]);
    let platform_coin = coins[0]["coin"].as_str().unwrap();
    let token = coins[1]["coin"].as_str().unwrap();

    let conf = Mm2TestConf::seednode(TEST_SEED, &coins);
    let mm = MarketMakerIt::start(conf.conf, conf.rpc_password, None).unwrap();

    let activation_res = block_on(enable_tendermint(&mm, platform_coin, &[], IRIS_TESTNET_RPC_URLS, false));
    println!("Activation with assets {}", json::to_string(&activation_res).unwrap());

    let activation_res = block_on(enable_tendermint_token(&mm, token));
    println!("Token activation {}", json::to_string(&activation_res).unwrap());

    // just call withdraw without sending to check response correctness
    let withdraw_result = block_on(withdraw_v1(
        &mm,
        token,
        "iaa1llp0f6qxemgh4g4m5ewk0ew0hxj76avuz8kwd5",
        "0.1",
    ));

    println!("Withdraw to other {}", json::to_string(&withdraw_result).unwrap());
    let tx_details: TransactionDetails = json::from_value(withdraw_result).unwrap();

    let expected_total: BigDecimal = "0.1".parse().unwrap();
    assert_eq!(tx_details.total_amount, expected_total);

    // TODO How to check it if the fee is dynamic?
    /*
    let expected_fee: BigDecimal = "0.05".parse().unwrap();
    let actual_fee: BigDecimal = tx_details.fee_details["amount"].as_str().unwrap().parse().unwrap();
    assert_eq!(actual_fee, expected_fee);
    */

    assert_eq!(tx_details.spent_by_me, expected_total);
    assert_eq!(tx_details.my_balance_change, expected_total * BigDecimal::from(-1));
    assert_eq!(tx_details.received_by_me, BigDecimal::default());
    assert_eq!(tx_details.to, vec![
        "iaa1llp0f6qxemgh4g4m5ewk0ew0hxj76avuz8kwd5".to_owned()
    ]);
    assert_eq!(tx_details.from, vec![
        "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2".to_owned()
    ]);

    // withdraw and send transaction to ourselves
    let withdraw_result = block_on(withdraw_v1(
        &mm,
        token,
        "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2",
        "0.1",
    ));
    println!("Withdraw to self {}", json::to_string(&withdraw_result).unwrap());

    let tx_details: TransactionDetails = json::from_value(withdraw_result).unwrap();
    let expected_total: BigDecimal = "0.1".parse().unwrap();
    let expected_received: BigDecimal = "0.1".parse().unwrap();

    assert_eq!(tx_details.total_amount, expected_total);

    // TODO How to check it if the fee is dynamic?
    /*
    let expected_fee: BigDecimal = "0.05".parse().unwrap();
    let actual_fee: BigDecimal = tx_details.fee_details["amount"].as_str().unwrap().parse().unwrap();
    assert_eq!(actual_fee, expected_fee);
    */

    assert_eq!(tx_details.spent_by_me, expected_total);
    assert_eq!(tx_details.received_by_me, expected_received);
    assert_eq!(tx_details.to, vec![
        "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2".to_owned()
    ]);
    assert_eq!(tx_details.from, vec![
        "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2".to_owned()
    ]);

    let send_raw_tx = block_on(send_raw_transaction(&mm, token, &tx_details.tx_hex));
    println!("Send raw tx {}", json::to_string(&send_raw_tx).unwrap());
}

#[test]
fn test_tendermint_tx_history() { block_on(tendermint_tx_history()); }

pub async fn tendermint_tx_history() {
    const TEST_SEED: &str = "iris test seed3";
    const TEST_ADDRESS: &str = "iaa17d8hndl72u2mzae8rymn003hxfpehcslnqcrcd";
    const TEST_ADDRESS2: &str = "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2";
    const TX_FINISHED_LOG: &str = "Tx history fetching finished for IRIS-TEST.";

    let coins = json!([iris_testnet_conf(), iris_nimda_testnet_conf()]);
    let platform_coin = coins[0]["coin"].as_str().unwrap();
    let token = coins[1]["coin"].as_str().unwrap();

    let conf = Mm2TestConf::seednode(TEST_SEED, &coins);
    let mut mm = MarketMakerIt::start_async(conf.conf, conf.rpc_password, None)
        .await
        .unwrap();

    let activation_res = enable_tendermint(&mm, platform_coin, &[token], IRIS_TESTNET_RPC_URLS, true).await;
    println!("Activation with assets {}", json::to_string(&activation_res).unwrap());

    if let Err(_) = mm.wait_for_log(900., |log| log.contains(TX_FINISHED_LOG)).await {
        println!("{}", mm.log_as_utf8().unwrap());
        assert!(false, "Tx history didn't finish which is not expected");
    }

    let first_tx_history_response = get_tendermint_my_tx_history(&mm, platform_coin, 1, 1).await;
    let first_total_txs = first_tx_history_response["result"]["total"].as_u64().unwrap();
    assert!(first_total_txs > 0);

    // self platform coin transfer
    let withdraw_self = withdraw_v1(&mm, platform_coin, TEST_ADDRESS, "0.01").await;
    let withdraw_self_tx_details: TransactionDetails = json::from_value(withdraw_self).unwrap();
    let raw_tx_self = send_raw_transaction(&mm, platform_coin, &withdraw_self_tx_details.tx_hex).await;
    let self_tx_hash = raw_tx_self["tx_hash"].as_str().unwrap();

    let expected_tx_hash_log = format!("Tx '{}' successfuly parsed.", self_tx_hash);
    if let Err(_) = mm.wait_for_log(45., |log| log.contains(&expected_tx_hash_log)).await {
        println!("{}", mm.log_as_utf8().unwrap());
        assert!(false, "Couldn't find history log for {}", self_tx_hash);
    }

    if let Err(_) = mm.wait_for_log(15., |log| log.contains(TX_FINISHED_LOG)).await {
        println!("{}", mm.log_as_utf8().unwrap());
        assert!(false, "Tx history didn't finish which is not expected");
    }

    // outgoing platform coin transfer
    let withdraw_out = withdraw_v1(&mm, platform_coin, TEST_ADDRESS2, "0.01").await;
    let withdraw_out_tx_details: TransactionDetails = json::from_value(withdraw_out).unwrap();
    let raw_tx_out = send_raw_transaction(&mm, platform_coin, &withdraw_out_tx_details.tx_hex).await;
    let out_tx_hash = raw_tx_out["tx_hash"].as_str().unwrap();

    // outgoing token transfer
    let withdraw_token_out = withdraw_v1(&mm, token, TEST_ADDRESS2, "0.01").await;
    let withdraw_token_out_tx_details: TransactionDetails = json::from_value(withdraw_token_out).unwrap();
    let raw_tx_token_out = send_raw_transaction(&mm, token, &withdraw_token_out_tx_details.tx_hex).await;
    let token_out_tx_hash = raw_tx_token_out["tx_hash"].as_str().unwrap();

    let expected_tx_hash_log = format!("Tx '{}' successfuly parsed.", out_tx_hash);
    if let Err(_) = mm.wait_for_log(45., |log| log.contains(&expected_tx_hash_log)).await {
        println!("{}", mm.log_as_utf8().unwrap());
        assert!(false, "Couldn't find history log for {}", out_tx_hash);
    }

    if let Err(_) = mm.wait_for_log(15., |log| log.contains(TX_FINISHED_LOG)).await {
        println!("{}", mm.log_as_utf8().unwrap());
        assert!(false, "Tx history didn't finish which is not expected");
    }

    let second_tx_history_response = get_tendermint_my_tx_history(&mm, platform_coin, 2, 1).await;
    let second_total_txs = second_tx_history_response["result"]["total"].as_u64().unwrap();
    assert_eq!(first_total_txs + 2, second_total_txs);

    let mut transactions = second_tx_history_response["result"]["transactions"].clone();
    // drop confirmations key since TransactionDetails since implements `deny_unknown_fields`
    transactions[0].as_object_mut().unwrap().remove("confirmations");
    transactions[1].as_object_mut().unwrap().remove("confirmations");

    let tx_amount = BigDecimal::from_str("0.01").unwrap();

    let out_tx_details: TransactionDetails = json::from_value(transactions[0].clone()).unwrap();
    let out_fee_amount = BigDecimal::from_str(out_tx_details.fee_details["amount"].as_str().unwrap()).unwrap();

    assert_eq!(out_tx_details.coin, platform_coin.to_string());
    assert_eq!(out_tx_details.tx_hash, out_tx_hash.to_string());
    assert_eq!(out_tx_details.from, vec![TEST_ADDRESS]);
    assert_eq!(out_tx_details.to, vec![TEST_ADDRESS2]);
    assert_eq!(out_tx_details.total_amount, &tx_amount + &out_fee_amount);
    assert_eq!(out_tx_details.spent_by_me, &tx_amount + &out_fee_amount);
    assert_eq!(out_tx_details.received_by_me, BigDecimal::default());
    assert_eq!(
        out_tx_details.my_balance_change,
        BigDecimal::default() - (&tx_amount + &out_fee_amount)
    );
    assert_eq!(out_tx_details.transaction_type, TransactionType::StandardTransfer);

    let self_tx_details: TransactionDetails = json::from_value(transactions[1].clone()).unwrap();

    assert_eq!(self_tx_details.coin, platform_coin.to_string());
    assert_eq!(self_tx_details.tx_hash, self_tx_hash.to_string());
    assert_eq!(self_tx_details.from, vec![TEST_ADDRESS]);
    assert_eq!(self_tx_details.to, vec![TEST_ADDRESS]);
    assert_eq!(self_tx_details.total_amount, tx_amount);
    assert_eq!(self_tx_details.spent_by_me, BigDecimal::default());
    assert_eq!(self_tx_details.received_by_me, tx_amount);
    assert_eq!(self_tx_details.my_balance_change, tx_amount);
    assert_eq!(self_tx_details.transaction_type, TransactionType::StandardTransfer);

    let token_tx_history_response = get_tendermint_my_tx_history(&mm, token, 2, 1).await;
    let mut transactions = token_tx_history_response["result"]["transactions"].clone();
    // drop confirmations key since TransactionDetails since implements `deny_unknown_fields`
    transactions[0].as_object_mut().unwrap().remove("confirmations");
    transactions[1].as_object_mut().unwrap().remove("confirmations");

    let (token_tx, platform_fee_tx_of_token) = {
        let tx1: TransactionDetails = json::from_value(transactions[0].clone()).unwrap();
        let tx2: TransactionDetails = json::from_value(transactions[1].clone()).unwrap();

        if tx1.coin == token.to_string() {
            (tx1, tx2)
        } else {
            (tx2, tx1)
        }
    };

    assert_eq!(token_tx.coin, token.to_string());
    assert_eq!(token_tx.tx_hash, token_out_tx_hash.to_string());
    assert_eq!(token_tx.from, vec![TEST_ADDRESS]);
    assert_eq!(token_tx.to, vec![TEST_ADDRESS2]);
    assert_eq!(token_tx.total_amount, tx_amount);
    assert_eq!(token_tx.spent_by_me, tx_amount);
    assert_eq!(token_tx.received_by_me, BigDecimal::default());
    assert_eq!(token_tx.my_balance_change, BigDecimal::default() - &tx_amount);
    assert_eq!(
        discriminant(&token_tx.transaction_type),
        discriminant(&TransactionType::TokenTransfer(String::default()))
    );

    let token_tx_fee_amount = BigDecimal::from_str(token_tx.fee_details["amount"].as_str().unwrap()).unwrap();

    assert_eq!(platform_fee_tx_of_token.coin, platform_coin.to_string());
    assert_eq!(platform_fee_tx_of_token.tx_hash, token_out_tx_hash.to_string());
    assert_eq!(platform_fee_tx_of_token.from, vec![TEST_ADDRESS]);
    assert!(platform_fee_tx_of_token.to.is_empty());
    assert!(platform_fee_tx_of_token.fee_details.is_null());
    assert_eq!(platform_fee_tx_of_token.total_amount, token_tx_fee_amount);
    assert_eq!(platform_fee_tx_of_token.spent_by_me, token_tx_fee_amount);
    assert_eq!(platform_fee_tx_of_token.received_by_me, BigDecimal::default());
    assert_eq!(
        platform_fee_tx_of_token.my_balance_change,
        BigDecimal::default() - &token_tx_fee_amount
    );
    assert_eq!(
        discriminant(&platform_fee_tx_of_token.transaction_type),
        discriminant(&TransactionType::Fee(String::default()))
    );

    mm.stop().await.unwrap();
}
