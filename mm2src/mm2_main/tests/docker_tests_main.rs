#![feature(async_closure)]
#![feature(custom_test_frameworks)]
#![feature(test)]
#![test_runner(docker_tests_runner)]
#![feature(drain_filter)]
#![feature(hash_raw_entry)]
#![feature(map_first_last)]
#![recursion_limit = "512"]

#[cfg(test)]
#[macro_use]
extern crate common;
#[cfg(test)]
#[macro_use]
extern crate gstuff;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[macro_use]
extern crate lazy_static;
#[cfg(test)]
#[macro_use]
extern crate serde_json;
#[cfg(test)]
#[macro_use]
extern crate serde_derive;
#[cfg(test)]
#[macro_use]
extern crate ser_error_derive;
#[cfg(test)] extern crate test;

use chain::TransactionOutput;
use coins::eth::{eth_coin_from_conf_and_request, EthCoin};
use coins::utxo::bch::{bch_coin_from_conf_and_params, BchActivationRequest, BchCoin};
use coins::utxo::rpc_clients::UtxoRpcClientEnum;
use coins::utxo::slp::SlpToken;
use coins::utxo::slp::{slp_genesis_output, SlpOutput};
use coins::utxo::utxo_common::send_outputs_from_my_address;
use coins::utxo::utxo_standard::{utxo_standard_coin_with_priv_key, UtxoStandardCoin};
use coins::utxo::{dhash160, GetUtxoListOps, UtxoActivationParams, UtxoCommonOps};
use coins::{CoinProtocol, MarketCoinOps, Transaction};
use common::{block_on, now_ms};
use crypto::privkey::{key_pair_from_secret, key_pair_from_seed};
use futures01::Future;
use keys::{Address, NetworkPrefix as CashAddrPrefix};
use mm2_core::mm_ctx::{MmArc, MmCtxBuilder};
use script::Builder;
use secp256k1::SecretKey;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::sync::Mutex;
use test::{test_main, StaticBenchFn, StaticTestFn, TestDescAndFn};
use testcontainers::clients::Cli;

mod docker_tests;
use docker_tests::docker_tests_common::*;
use docker_tests::qrc20_tests::{qtum_docker_node, QtumDockerOps, QTUM_REGTEST_DOCKER_IMAGE};
mod integration_tests_common;

// AP: custom test runner is intended to initialize the required environment (e.g. coin daemons in the docker containers)
// and then gracefully clear it by dropping the RAII docker container handlers
// I've tried to use static for such singleton initialization but it turned out that despite
// rustc allows to use Drop as static the drop fn won't ever be called
// NB: https://github.com/rust-lang/rfcs/issues/1111
// the only preparation step required is Zcash params files downloading:
// Windows - https://github.com/KomodoPlatform/komodo/blob/master/zcutil/fetch-params.bat
// Linux and MacOS - https://github.com/KomodoPlatform/komodo/blob/master/zcutil/fetch-params.sh
pub fn docker_tests_runner(tests: &[&TestDescAndFn]) {
    // pretty_env_logger::try_init();
    let docker = Cli::default();
    let mut containers = vec![];
    // skip Docker containers initialization if we are intended to run test_mm_start only
    if std::env::var("_MM2_TEST_CONF").is_err() {
        pull_docker_image(UTXO_ASSET_DOCKER_IMAGE);
        pull_docker_image(QTUM_REGTEST_DOCKER_IMAGE);
        remove_docker_containers(UTXO_ASSET_DOCKER_IMAGE);
        remove_docker_containers(QTUM_REGTEST_DOCKER_IMAGE);

        let utxo_node = utxo_asset_docker_node(&docker, "MYCOIN", 7000);
        let utxo_node1 = utxo_asset_docker_node(&docker, "MYCOIN1", 8000);
        let qtum_node = qtum_docker_node(&docker, 9000);
        let for_slp_node = utxo_asset_docker_node(&docker, "FORSLP", 10000);

        let utxo_ops = UtxoAssetDockerOps::from_ticker("MYCOIN");
        let utxo_ops1 = UtxoAssetDockerOps::from_ticker("MYCOIN1");
        let qtum_ops = QtumDockerOps::new();
        let for_slp_ops = BchDockerOps::from_ticker("FORSLP");

        utxo_ops.wait_ready(4);
        utxo_ops1.wait_ready(4);
        qtum_ops.wait_ready(2);
        qtum_ops.initialize_contracts();
        for_slp_ops.wait_ready(4);
        for_slp_ops.initialize_slp();

        containers.push(utxo_node);
        containers.push(utxo_node1);
        containers.push(qtum_node);
        containers.push(for_slp_node);
    }
    // detect if docker is installed
    // skip the tests that use docker if not installed
    let owned_tests: Vec<_> = tests
        .iter()
        .map(|t| match t.testfn {
            StaticTestFn(f) => TestDescAndFn {
                testfn: StaticTestFn(f),
                desc: t.desc.clone(),
            },
            StaticBenchFn(f) => TestDescAndFn {
                testfn: StaticBenchFn(f),
                desc: t.desc.clone(),
            },
            _ => panic!("non-static tests passed to lp_coins test runner"),
        })
        .collect();
    let args: Vec<String> = std::env::args().collect();
    let _exit_code = test_main(&args, owned_tests, None);
}

#[cfg(all(test, target_arch = "wasm32"))]
mod docker_tests {
    use test::{test_main, StaticBenchFn, StaticTestFn, TestDescAndFn};

    pub fn docker_tests_runner(tests: &[&TestDescAndFn]) {
        let owned_tests: Vec<_> = tests
            .iter()
            .map(|t| match t.testfn {
                StaticTestFn(f) => TestDescAndFn {
                    testfn: StaticTestFn(f),
                    desc: t.desc.clone(),
                },
                StaticBenchFn(f) => TestDescAndFn {
                    testfn: StaticBenchFn(f),
                    desc: t.desc.clone(),
                },
                _ => panic!("non-static tests passed to lp_coins test runner"),
            })
            .collect();
        let args: Vec<String> = std::env::args().collect();
        let _exit_code = test_main(&args, owned_tests, None);
    }
}

fn pull_docker_image(name: &str) {
    Command::new("docker")
        .arg("pull")
        .arg(name)
        .status()
        .expect("Failed to execute docker command");
}

fn remove_docker_containers(name: &str) {
    let stdout = Command::new("docker")
        .arg("ps")
        .arg("-f")
        .arg(format!("ancestor={}", name))
        .arg("-q")
        .output()
        .expect("Failed to execute docker command");

    let reader = BufReader::new(stdout.stdout.as_slice());
    let ids: Vec<_> = reader.lines().map(|line| line.unwrap()).collect();
    if !ids.is_empty() {
        Command::new("docker")
            .arg("rm")
            .arg("-f")
            .args(ids)
            .status()
            .expect("Failed to execute docker command");
    }
}

struct UtxoAssetDockerOps {
    #[allow(dead_code)]
    ctx: MmArc,
    coin: UtxoStandardCoin,
}

impl CoinDockerOps for UtxoAssetDockerOps {
    fn rpc_client(&self) -> &UtxoRpcClientEnum { &self.coin.as_ref().rpc_client }
}

impl UtxoAssetDockerOps {
    fn from_ticker(ticker: &str) -> UtxoAssetDockerOps {
        let conf = json!({"asset": ticker, "txfee": 1000, "network": "regtest"});
        let req = json!({"method":"enable"});
        let priv_key = hex::decode("809465b17d0a4ddb3e4c69e8f23c2cabad868f51f8bed5c765ad1d6516c3306f").unwrap();
        let ctx = MmCtxBuilder::new().into_mm_arc();
        let params = UtxoActivationParams::from_legacy_req(&req).unwrap();

        let coin = block_on(utxo_standard_coin_with_priv_key(
            &ctx, ticker, &conf, &params, &priv_key,
        ))
        .unwrap();
        UtxoAssetDockerOps { ctx, coin }
    }
}

struct BchDockerOps {
    #[allow(dead_code)]
    ctx: MmArc,
    coin: BchCoin,
}

// builds the EthCoin using the external dev Parity/OpenEthereum node
// the address belonging to the default passphrase has million of ETH that it can distribute to
// random privkeys generated in tests
fn eth_distributor() -> EthCoin {
    let conf = json!({"coin":"ETH","name":"ethereum","protocol":{"type":"ETH"}});
    let req = json!({
        "method": "enable",
        "coin": "ETH",
        "urls": ["http://195.201.0.6:8565"],
        "swap_contract_address": "0xa09ad3cd7e96586ebd05a2607ee56b56fb2db8fd",
    });
    let keypair =
        key_pair_from_seed("spice describe gravity federal blast come thank unfair canal monkey style afraid").unwrap();
    block_on(eth_coin_from_conf_and_request(
        &MM_CTX,
        "ETH",
        &conf,
        &req,
        &*keypair.private().secret,
        CoinProtocol::ETH,
    ))
    .unwrap()
}

// pass address without 0x prefix to this fn
fn fill_eth(to_addr: &str) {
    ETH_DISTRIBUTOR
        .send_to_address(to_addr.parse().unwrap(), 1_000_000_000_000_000_000u64.into())
        .wait()
        .unwrap();
}

lazy_static! {
    static ref COINS_LOCK: Mutex<()> = Mutex::new(());
    static ref ETH_DISTRIBUTOR: EthCoin = eth_distributor();
    static ref MM_CTX: MmArc = MmCtxBuilder::new().into_mm_arc();
}

impl BchDockerOps {
    fn from_ticker(ticker: &str) -> BchDockerOps {
        let conf = json!({"asset": ticker,"txfee":1000,"network": "regtest","txversion":4,"overwintered":1});
        let req = json!({"method":"enable", "bchd_urls": [], "allow_slp_unsafe_conf": true});
        let priv_key = hex::decode("809465b17d0a4ddb3e4c69e8f23c2cabad868f51f8bed5c765ad1d6516c3306f").unwrap();
        let ctx = MmCtxBuilder::new().into_mm_arc();
        let params = BchActivationRequest::from_legacy_req(&req).unwrap();

        let coin = block_on(bch_coin_from_conf_and_params(
            &ctx,
            ticker,
            &conf,
            params,
            CashAddrPrefix::SlpTest,
            &priv_key,
        ))
        .unwrap();
        BchDockerOps { ctx, coin }
    }

    fn initialize_slp(&self) {
        fill_address(&self.coin, &self.coin.my_address().unwrap(), 100000.into(), 30);
        let mut slp_privkeys = vec![];

        let slp_genesis_op_ret = slp_genesis_output("ADEXSLP", "ADEXSLP", None, None, 8, None, 1000000_00000000);
        let slp_genesis = TransactionOutput {
            value: self.coin.as_ref().dust_amount,
            script_pubkey: Builder::build_p2pkh(&self.coin.my_public_key().unwrap().address_hash().into()).to_bytes(),
        };

        let mut bch_outputs = vec![slp_genesis_op_ret, slp_genesis];
        let mut slp_outputs = vec![];

        for _ in 0..18 {
            let priv_key = SecretKey::new(&mut rand6::thread_rng());
            let key_pair = key_pair_from_secret(priv_key.as_ref()).unwrap();
            let address_hash = key_pair.public().address_hash();
            let address = Address {
                prefix: self.coin.as_ref().conf.pub_addr_prefix,
                t_addr_prefix: self.coin.as_ref().conf.pub_t_addr_prefix,
                hrp: None,
                hash: address_hash.into(),
                checksum_type: Default::default(),
                addr_format: Default::default(),
            };

            self.native_client()
                .import_address(&address.to_string(), &address.to_string(), false)
                .wait()
                .unwrap();

            let script_pubkey = Builder::build_p2pkh(&address_hash.into());

            bch_outputs.push(TransactionOutput {
                value: 1000_00000000,
                script_pubkey: script_pubkey.to_bytes(),
            });

            slp_outputs.push(SlpOutput {
                amount: 1000_00000000,
                script_pubkey: script_pubkey.to_bytes(),
            });
            slp_privkeys.push(*priv_key.as_ref());
        }

        let slp_genesis_tx = send_outputs_from_my_address(self.coin.clone(), bch_outputs)
            .wait()
            .unwrap();
        self.coin
            .wait_for_confirmations(&slp_genesis_tx.tx_hex(), 1, false, now_ms() / 1000 + 30, 1)
            .wait()
            .unwrap();

        let adex_slp = SlpToken::new(
            8,
            "ADEXSLP".into(),
            slp_genesis_tx.tx_hash().as_slice().into(),
            self.coin.clone(),
            1,
        );

        let tx = block_on(adex_slp.send_slp_outputs(slp_outputs)).unwrap();
        self.coin
            .wait_for_confirmations(&tx.tx_hex(), 1, false, now_ms() / 1000 + 30, 1)
            .wait()
            .unwrap();
        *SLP_TOKEN_OWNERS.lock().unwrap() = slp_privkeys;
        *SLP_TOKEN_ID.lock().unwrap() = slp_genesis_tx.tx_hash().as_slice().into();
    }
}

impl CoinDockerOps for BchDockerOps {
    fn rpc_client(&self) -> &UtxoRpcClientEnum { &self.coin.as_ref().rpc_client }
}
