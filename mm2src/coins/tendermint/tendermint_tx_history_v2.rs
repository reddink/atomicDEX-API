#![allow(warnings)]

use super::iris::htlc_proto::QueryHtlcResponseProto;
use super::{rpc::*, type_urls::*, TendermintCoin, TendermintCoinRpcError, TendermintToken};

use crate::my_tx_history_v2::{CoinWithTxHistoryV2, MyTxHistoryErrorV2, MyTxHistoryTarget, TxHistoryStorage};
use crate::tendermint::iris::htlc::{HTLC_STATE_COMPLETED, HTLC_STATE_OPEN, HTLC_STATE_REFUNDED};
use crate::tendermint::iris::htlc_proto::{ClaimHtlcProtoRep, CreateHtlcProtoRep};
use crate::tx_history_storage::{GetTxHistoryFilters, WalletId};
use crate::utxo::utxo_common::big_decimal_from_sat_unsigned;
use crate::utxo::utxo_tx_history_v2::StopReason;
use crate::{HistorySyncState, MarketCoinOps};
use async_trait::async_trait;
use common::log;
use common::state_machine::prelude::*;
use cosmrs::proto::cosmos::bank::v1beta1::MsgSend;
use itertools::Itertools;
use mm2_err_handle::prelude::MmResult;
use mm2_metrics::MetricsArc;
use mm2_number::BigDecimal;
use primitives::hash::H256;
use prost::Message;
use std::collections::HashMap;
use std::str::FromStr;

#[async_trait]
pub trait TendermintTxHistoryOps: CoinWithTxHistoryV2 + MarketCoinOps + Send + Sync + 'static {
    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError>;

    async fn query_htlc(&self, id: String) -> MmResult<QueryHtlcResponseProto, TendermintCoinRpcError>;

    fn decimals(&self) -> u8;

    fn set_history_sync_state(&self, new_state: HistorySyncState);
}

#[async_trait]
impl CoinWithTxHistoryV2 for TendermintCoin {
    fn history_wallet_id(&self) -> WalletId { WalletId::new(self.ticker().replace('-', "_")) }

    async fn get_tx_history_filters(
        &self,
        _target: MyTxHistoryTarget,
    ) -> MmResult<GetTxHistoryFilters, MyTxHistoryErrorV2> {
        Ok(GetTxHistoryFilters::for_address(self.account_id.to_string()))
    }
}

#[async_trait]
impl CoinWithTxHistoryV2 for TendermintToken {
    fn history_wallet_id(&self) -> WalletId { WalletId::new(self.ticker().to_owned()) }

    async fn get_tx_history_filters(
        &self,
        _target: MyTxHistoryTarget,
    ) -> MmResult<GetTxHistoryFilters, MyTxHistoryErrorV2> {
        Ok(GetTxHistoryFilters::for_address(
            self.platform_coin.account_id.to_string(),
        ))
    }
}

struct TendermintTxHistoryCtx<Coin: TendermintTxHistoryOps, Storage: TxHistoryStorage> {
    coin: Coin,
    storage: Storage,
    metrics: MetricsArc,
    /// Last requested balances of the activated coin's addresses.
    /// TODO add a `CoinBalanceState` structure and replace [`HashMap<String, BigDecimal>`] everywhere.
    balances: HashMap<String, BigDecimal>,
}

struct TendermintInit<Coin, Storage> {
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> TendermintInit<Coin, Storage> {
    fn new() -> Self {
        TendermintInit {
            phantom: Default::default(),
        }
    }
}

struct Stopped<Coin, Storage> {
    phantom: std::marker::PhantomData<(Coin, Storage)>,
    stop_reason: StopReason,
}

impl<Coin, Storage> Stopped<Coin, Storage> {
    fn history_too_large() -> Self {
        Stopped {
            phantom: Default::default(),
            stop_reason: StopReason::HistoryTooLarge,
        }
    }

    fn storage_error<E>(e: E) -> Self
    where
        E: std::fmt::Debug,
    {
        Stopped {
            phantom: Default::default(),
            stop_reason: StopReason::StorageError(format!("{:?}", e)),
        }
    }

    fn unknown(e: String) -> Self {
        Stopped {
            phantom: Default::default(),
            stop_reason: StopReason::UnknownError(e),
        }
    }
}

struct OnIoErrorCooldown<Coin, Storage> {
    address: String,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> OnIoErrorCooldown<Coin, Storage> {
    fn new(address: String) -> Self {
        OnIoErrorCooldown {
            address,
            phantom: Default::default(),
        }
    }
}

struct WaitForHistoryUpdateTrigger<Coin, Storage> {
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> WaitForHistoryUpdateTrigger<Coin, Storage> {
    fn new() -> Self {
        WaitForHistoryUpdateTrigger {
            phantom: Default::default(),
        }
    }
}

struct FetchingTransactionsData<Coin, Storage> {
    /// The list of addresses for those we have requested [`UpdatingUnconfirmedTxes::all_tx_ids_with_height`] TX hashses
    /// at the `FetchingTxHashes` state.
    address: String,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> FetchingTransactionsData<Coin, Storage> {
    fn new(address: String) -> Self {
        FetchingTransactionsData {
            address,
            phantom: Default::default(),
        }
    }
}

impl<Coin, Storage> TransitionFrom<TendermintInit<Coin, Storage>> for Stopped<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<TendermintInit<Coin, Storage>> for FetchingTransactionsData<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<WaitForHistoryUpdateTrigger<Coin, Storage>> for Stopped<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>> for Stopped<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>>
    for WaitForHistoryUpdateTrigger<Coin, Storage>
{
}

#[async_trait]
impl<Coin, Storage> State for FetchingTransactionsData<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        const TX_PAGE_SIZE: u8 = 50;

        async fn query_transactions<Coin>(coin: &Coin, query: String) -> Vec<crate::TransactionDetails>
        where
            Coin: TendermintTxHistoryOps,
        {
            // TODO
            // check if coin matches in the transactions before inserting them
            let mut page = 1;
            let mut iterate_more = true;

            let mut tx_details = vec![];

            let client = coin.get_rpc_client().await.unwrap();
            while iterate_more {
                let response = client
                    .perform(TxSearchRequest::new(
                        query.clone(),
                        false,
                        page,
                        TX_PAGE_SIZE,
                        TendermintResultOrder::Ascending.into(),
                    ))
                    .await
                    .unwrap();

                for tx in response.txs {
                    // TODO
                    // Ignore inserted ones
                    let deserialized_tx = cosmrs::Tx::from_bytes(tx.tx.as_bytes()).unwrap();
                    let msg = deserialized_tx
                        .body
                        .messages
                        .first()
                        .ok_or("Tx body couldn't be read.")
                        .unwrap();

                    match msg.type_url.as_str() {
                        SEND_TYPE_URL => {
                            let send_tx = MsgSend::decode(msg.value.as_slice()).unwrap();
                            let amount: u64 = send_tx.amount.first().unwrap().amount.parse().unwrap();
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if send_tx.from_address == coin.my_address().expect("my_address should never fail") {
                                    (amount.clone(), BigDecimal::default())
                                } else {
                                    (BigDecimal::default(), amount.clone())
                                };

                            let details = crate::TransactionDetails {
                                from: vec![send_tx.from_address],
                                to: vec![send_tx.to_address.clone()],
                                total_amount: amount,
                                spent_by_me: spent_by_me.clone(),
                                received_by_me: received_by_me.clone(),
                                my_balance_change: received_by_me - spent_by_me,
                                tx_hash: tx.hash.to_string(),
                                tx_hex: msg.value.as_slice().into(),
                                // TODO
                                fee_details: None, // Some(fee_details.into()),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id: H256::from(tx.hash.as_bytes()).reversed().to_vec().into(),
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };
                            tx_details.push(details);
                        },
                        CLAIM_HTLC_TYPE_URL => {
                            let htlc_tx = ClaimHtlcProtoRep::decode(msg.value.as_slice()).unwrap();
                            let htlc_response = coin.query_htlc(htlc_tx.id).await.unwrap();

                            let htlc_data = match htlc_response.htlc {
                                Some(htlc) => htlc,
                                None => continue,
                            };

                            match htlc_data.state {
                                HTLC_STATE_OPEN | HTLC_STATE_COMPLETED | HTLC_STATE_REFUNDED => {},
                                unexpected_state => continue,
                            };

                            let amount: u64 = htlc_data.amount.first().unwrap().amount.parse().unwrap();
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if htlc_data.sender == coin.my_address().expect("my_address should never fail") {
                                    (amount.clone(), BigDecimal::default())
                                } else {
                                    (BigDecimal::default(), amount.clone())
                                };

                            let details = crate::TransactionDetails {
                                from: vec![htlc_data.sender],
                                to: vec![htlc_data.to.clone()],
                                total_amount: amount,
                                spent_by_me: spent_by_me.clone(),
                                received_by_me: received_by_me.clone(),
                                my_balance_change: received_by_me - spent_by_me,
                                tx_hash: tx.hash.to_string(),
                                tx_hex: msg.value.as_slice().into(),
                                // TODO
                                fee_details: None, // Some(fee_details.into()),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id: H256::from(tx.hash.as_bytes()).reversed().to_vec().into(),
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };

                            tx_details.push(details);
                        },
                        CREATE_HTLC_TYPE_URL => {
                            let htlc_tx = CreateHtlcProtoRep::decode(msg.value.as_slice()).unwrap();
                            let amount: u64 = htlc_tx.amount.first().unwrap().amount.parse().unwrap();

                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if htlc_tx.sender == coin.my_address().expect("my_address should never fail") {
                                    (amount.clone(), BigDecimal::default())
                                } else {
                                    (BigDecimal::default(), amount.clone())
                                };

                            let details = crate::TransactionDetails {
                                from: vec![htlc_tx.sender],
                                to: vec![htlc_tx.to.clone()],
                                total_amount: amount,
                                spent_by_me: spent_by_me.clone(),
                                received_by_me: received_by_me.clone(),
                                my_balance_change: received_by_me - spent_by_me,
                                tx_hash: tx.hash.to_string(),
                                tx_hex: msg.value.as_slice().into(),
                                // TODO
                                fee_details: None, // Some(fee_details.into()),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id: H256::from(tx.hash.as_bytes()).reversed().to_vec().into(),
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };
                            tx_details.push(details);
                        },
                        _ => {},
                    }
                }

                iterate_more = (TX_PAGE_SIZE as u32 * page) < response.total_count;
                page += 1;
            }

            tx_details
        }

        let rpc_client = ctx.coin.get_rpc_client().await.unwrap();

        let address = ctx.coin.my_address().expect("should not fail");

        // TODO
        // do the iteration here to avoid pressuring memory
        let q = format!("transfer.sender = '{}'", address.clone());
        let mut tx_details = vec![];
        tx_details.extend_from_slice(&query_transactions(&ctx.coin, q).await);

        let q = format!("transfer.recipient = '{}'", self.address.clone());
        tx_details.extend_from_slice(&query_transactions(&ctx.coin, q).await);

        let tx_details: Vec<crate::TransactionDetails> =
            tx_details.into_iter().unique_by(|t| t.tx_hash.clone()).collect();

        if let Err(e) = ctx
            .storage
            .add_transactions_to_history(&ctx.coin.history_wallet_id(), tx_details)
            .await
        {
            return Self::change_state(Stopped::storage_error(e));
        }

        log::info!("Tx history fetching finished for {}", ctx.coin.ticker());
        ctx.coin.set_history_sync_state(HistorySyncState::Finished);
        // Self::change_state(WaitForHistoryUpdateTrigger::new())

        todo!()
    }
}

#[async_trait]
impl<Coin, Storage> State for TendermintInit<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        ctx.coin.set_history_sync_state(HistorySyncState::NotStarted);

        if let Err(e) = ctx.storage.init(&ctx.coin.history_wallet_id()).await {
            return Self::change_state(Stopped::storage_error(e));
        }

        Self::change_state(FetchingTransactionsData::new(
            ctx.coin.my_address().expect("should not fail"),
        ))
    }
}

#[async_trait]
impl<Coin, Storage> LastState for Stopped<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> Self::Result {
        log::info!(
            "Stopping tx history fetching for {}. Reason: {:?}",
            ctx.coin.ticker(),
            self.stop_reason
        );
        let new_state_json = match self.stop_reason {
            StopReason::HistoryTooLarge => json!({
                // "code": crate::utxo::utxo_common::HISTORY_TOO_LARGE_ERR_CODE,
                "code": -1_i64,
                "message": "Got `history too large` error from Electrum server. History is not available",
            }),
            reason => json!({
                "message": format!("{:?}", reason),
            }),
        };
        ctx.coin.set_history_sync_state(HistorySyncState::Error(new_state_json));
    }
}

#[async_trait]
impl TendermintTxHistoryOps for TendermintCoin {
    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError> { self.rpc_client().await }

    async fn query_htlc(&self, id: String) -> MmResult<QueryHtlcResponseProto, TendermintCoinRpcError> {
        self.query_htlc(id).await
    }

    fn decimals(&self) -> u8 { self.decimals }

    fn set_history_sync_state(&self, new_state: HistorySyncState) {
        *self.history_sync_state.lock().unwrap() = new_state;
    }
}

pub async fn tendermint_history_loop(
    coin: TendermintCoin,
    storage: impl TxHistoryStorage,
    metrics: MetricsArc,
    current_balance: BigDecimal,
) {
    let my_address = match coin.my_address() {
        Ok(my_address) => my_address,
        Err(e) => {
            log::error!("{}", e);
            return;
        },
    };
    let mut balances = HashMap::new();
    balances.insert(my_address, current_balance);
    drop_mutability!(balances);

    let ctx = TendermintTxHistoryCtx {
        coin,
        storage,
        metrics,
        balances,
    };

    let state_machine: StateMachine<_, ()> = StateMachine::from_ctx(ctx);
    state_machine.run(TendermintInit::new()).await;
}
