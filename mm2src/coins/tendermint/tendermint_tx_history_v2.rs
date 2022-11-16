use super::iris::htlc_proto::QueryHtlcResponseProto;
use super::{rpc::*, type_urls::*, AllBalancesResult, TendermintCoin, TendermintCoinRpcError, TendermintToken};

use crate::my_tx_history_v2::{CoinWithTxHistoryV2, MyTxHistoryErrorV2, MyTxHistoryTarget, TxHistoryStorage};
use crate::tendermint::iris::htlc::{HTLC_STATE_COMPLETED, HTLC_STATE_OPEN, HTLC_STATE_REFUNDED};
use crate::tendermint::iris::htlc_proto::{ClaimHtlcProtoRep, CreateHtlcProtoRep};
use crate::tendermint::TendermintFeeDetails;
use crate::tx_history_storage::{GetTxHistoryFilters, WalletId};
use crate::utxo::utxo_common::big_decimal_from_sat_unsigned;
use crate::utxo::utxo_tx_history_v2::StopReason;
use crate::{HistorySyncState, MarketCoinOps, TxFeeDetails};
use async_trait::async_trait;
use common::executor::Timer;
use common::log;
use common::state_machine::prelude::*;
use cosmrs::proto::cosmos::bank::v1beta1::MsgSend;
use mm2_err_handle::prelude::MmResult;
use mm2_metrics::MetricsArc;
use mm2_number::BigDecimal;
use primitives::hash::H256;
use prost::Message;

#[async_trait]
pub trait TendermintTxHistoryOps: CoinWithTxHistoryV2 + MarketCoinOps + Send + Sync + 'static {
    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError>;

    async fn query_htlc(&self, id: String) -> MmResult<QueryHtlcResponseProto, TendermintCoinRpcError>;

    fn decimals(&self) -> u8;

    fn set_history_sync_state(&self, new_state: HistorySyncState);

    fn my_denom(&self) -> cosmrs::Denom;

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError>;
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
    fn history_wallet_id(&self) -> WalletId { WalletId::new(self.ticker().replace('-', "_")) }

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
    #[allow(dead_code)]
    metrics: MetricsArc,
    balances: AllBalancesResult,
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

impl<Coin, Storage> TransitionFrom<WaitForHistoryUpdateTrigger<Coin, Storage>>
    for FetchingTransactionsData<Coin, Storage>
{
}

impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>>
    for WaitForHistoryUpdateTrigger<Coin, Storage>
{
}

#[async_trait]
impl<Coin, Storage> State for WaitForHistoryUpdateTrigger<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        let _wallet_id = ctx.coin.history_wallet_id();
        loop {
            Timer::sleep(30.).await;

            let ctx_balances = ctx.balances.clone();

            let balances = match ctx.coin.all_balances().await {
                Ok(balances) => balances,
                Err(e) => return Self::change_state(Stopped::unknown(e.to_string())),
            };

            if balances != ctx_balances {
                return Self::change_state(FetchingTransactionsData::new(
                    ctx.coin.my_address().expect("should not fail"),
                ));
            }
        }
    }
}

#[async_trait]
impl<Coin, Storage> State for FetchingTransactionsData<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    // TODO
    // - storage should be coin specific
    // - claim/create htlcs will display twice
    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        const TX_PAGE_SIZE: u8 = 50;

        async fn query_transactions<Coin, Storage>(coin: &Coin, storage: &Storage, query: String) -> Result<(), String>
        where
            Coin: TendermintTxHistoryOps,
            Storage: TxHistoryStorage,
        {
            let mut page = 1;
            let mut iterate_more = true;

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

                let mut tx_details = vec![];
                for tx in response.txs {
                    let internal_id = H256::from(tx.hash.as_bytes()).reversed().to_vec().into();
                    match storage
                        .get_tx_from_history(&coin.history_wallet_id(), &internal_id)
                        .await
                    {
                        Ok(Some(_)) => {
                            log::debug!(
                                "Tx '{}' already exists in tx_history. Skipping it.",
                                tx.hash.to_string()
                            );
                            continue;
                        },
                        _ => {
                            log::debug!("Adding tx: '{}' to tx_history.", tx.hash.to_string());
                        },
                    }

                    let deserialized_tx = cosmrs::Tx::from_bytes(tx.tx.as_bytes()).unwrap();

                    let fee = deserialized_tx.auth_info.fee;
                    let fee_amount: u64 = fee
                        .amount
                        .first()
                        .expect("Fee amount can't be empty")
                        .amount
                        .to_string()
                        .parse()
                        .expect("fee_amount parsing can't fail");

                    let fee_details = TxFeeDetails::Tendermint(TendermintFeeDetails {
                        coin: coin.platform_ticker().to_string(),
                        amount: big_decimal_from_sat_unsigned(fee_amount, coin.decimals()),
                        gas_limit: fee.gas_limit.value(),
                    });

                    let msg = deserialized_tx
                        .body
                        .messages
                        .first()
                        .ok_or("Tx body couldn't be read.")
                        .unwrap();

                    match msg.type_url.as_str() {
                        SEND_TYPE_URL => {
                            let sent_tx = MsgSend::decode(msg.value.as_slice()).unwrap();
                            let coin_amount = sent_tx.amount.first().unwrap();

                            let amount: u64 = coin_amount.amount.parse().unwrap();
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if sent_tx.from_address == coin.my_address().expect("my_address should never fail") {
                                    (amount.clone(), BigDecimal::default())
                                } else {
                                    (BigDecimal::default(), amount.clone())
                                };

                            let details = crate::TransactionDetails {
                                from: vec![sent_tx.from_address],
                                to: vec![sent_tx.to_address.clone()],
                                total_amount: amount,
                                spent_by_me: spent_by_me.clone(),
                                received_by_me: received_by_me.clone(),
                                my_balance_change: received_by_me - spent_by_me,
                                tx_hash: tx.hash.to_string(),
                                tx_hex: msg.value.as_slice().into(),
                                fee_details: Some(fee_details),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id,
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
                                _unexpected_state => continue,
                            };

                            let coin_amount = htlc_data.amount.first().unwrap();

                            let amount: u64 = coin_amount.amount.parse().unwrap();
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
                                fee_details: Some(fee_details),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id,
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };

                            tx_details.push(details);
                        },
                        CREATE_HTLC_TYPE_URL => {
                            let htlc_tx = CreateHtlcProtoRep::decode(msg.value.as_slice()).unwrap();

                            let coin_amount = htlc_tx.amount.first().unwrap();

                            let amount: u64 = coin_amount.amount.parse().unwrap();
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
                                fee_details: Some(fee_details),
                                block_height: tx.height.into(),
                                coin: coin.ticker().to_string(),
                                internal_id,
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };
                            tx_details.push(details);
                        },
                        _ => {},
                    }
                }

                if let Err(e) = storage
                    .add_transactions_to_history(&coin.history_wallet_id(), tx_details)
                    .await
                {
                    return Err(format!("{:?}", e));
                }

                iterate_more = (TX_PAGE_SIZE as u32 * page) < response.total_count;
                page += 1;
            }

            Ok(())
        }

        let address = ctx.coin.my_address().expect("should not fail");

        let q = format!("transfer.sender = '{}'", address.clone());
        if let Err(e) = query_transactions(&ctx.coin, &ctx.storage, q).await {
            return Self::change_state(Stopped::unknown(e));
        };

        let q = format!("transfer.recipient = '{}'", self.address.clone());
        if let Err(e) = query_transactions(&ctx.coin, &ctx.storage, q).await {
            return Self::change_state(Stopped::unknown(e));
        };

        log::info!("Tx history fetching finished for {}", ctx.coin.ticker());
        ctx.coin.set_history_sync_state(HistorySyncState::Finished);
        Self::change_state(WaitForHistoryUpdateTrigger::new())
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

    fn my_denom(&self) -> cosmrs::Denom { self.denom.clone() }

    fn set_history_sync_state(&self, new_state: HistorySyncState) {
        *self.history_sync_state.lock().unwrap() = new_state;
    }

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError> { self.all_balances().await }
}

pub async fn tendermint_history_loop(
    coin: TendermintCoin,
    storage: impl TxHistoryStorage,
    metrics: MetricsArc,
    _current_balance: BigDecimal,
) {
    let balances = match coin.all_balances().await {
        Ok(balances) => balances,
        Err(e) => {
            log::error!("{}", e);
            return;
        },
    };

    let ctx = TendermintTxHistoryCtx {
        coin,
        storage,
        metrics,
        balances,
    };

    let state_machine: StateMachine<_, ()> = StateMachine::from_ctx(ctx);
    state_machine.run(TendermintInit::new()).await;
}
