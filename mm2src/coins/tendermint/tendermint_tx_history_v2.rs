use std::collections::hash_map::Entry;
use std::collections::HashMap;

use super::iris::htlc_proto::QueryHtlcResponseProto;
use super::{rpc::*, type_urls::*, AllBalancesResult, TendermintCoin, TendermintCoinRpcError, TendermintToken};

use crate::my_tx_history_v2::{CoinWithTxHistoryV2, MyTxHistoryErrorV2, MyTxHistoryTarget, TxHistoryStorage};
use crate::tendermint::iris::htlc::{HTLC_STATE_COMPLETED, HTLC_STATE_OPEN, HTLC_STATE_REFUNDED};
use crate::tendermint::iris::htlc_proto::{ClaimHtlcProtoRep, CreateHtlcProtoRep};
use crate::tendermint::TendermintFeeDetails;
use crate::tx_history_storage::{GetTxHistoryFilters, WalletId};
use crate::utxo::utxo_common::big_decimal_from_sat_unsigned;
use crate::{HistorySyncState, MarketCoinOps, TxFeeDetails};
use async_trait::async_trait;
use common::executor::Timer;
use common::log;
use common::state_machine::prelude::*;
use cosmrs::proto::cosmos::bank::v1beta1::MsgSend;
use mm2_err_handle::prelude::{MmError, MmResult};
use mm2_metrics::MetricsArc;
use mm2_number::BigDecimal;
use primitives::hash::H256;
use prost::Message;
use rpc::v1::types::Bytes as BytesJson;

macro_rules! try_or_return_stopped_as_err {
    ($exp:expr, $reason: expr, $fmt:literal) => {
        match $exp {
            Ok(t) => t,
            Err(e) => {
                return Err(Stopped {
                    phantom: Default::default(),
                    stop_reason: $reason(format!("{}: {}", $fmt, e)),
                })
            },
        }
    };
}

#[async_trait]
pub trait TendermintTxHistoryOps: CoinWithTxHistoryV2 + MarketCoinOps + Send + Sync + 'static {
    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError>;

    async fn query_htlc(&self, id: String) -> MmResult<QueryHtlcResponseProto, TendermintCoinRpcError>;

    fn decimals(&self) -> u8;

    fn set_history_sync_state(&self, new_state: HistorySyncState);

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError>;

    fn all_wallet_ids(&self) -> Vec<WalletId>;

    // Returns Hashmap where key is denom and value is ticker
    fn get_denom_and_ticker_map(&self) -> HashMap<String, String>;
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

#[derive(Debug)]
enum StopReason {
    #[allow(dead_code)]
    HistoryTooLarge,
    StorageError(String),
    UnknownError(String),
    RpcClient(String),
    Marshaling(String),
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
    /// Denom - Ticker
    active_assets: HashMap<String, String>,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> FetchingTransactionsData<Coin, Storage> {
    fn new(address: String, active_assets: HashMap<String, String>) -> Self {
        FetchingTransactionsData {
            address,
            active_assets,
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
        loop {
            Timer::sleep(30.).await;

            let ctx_balances = ctx.balances.clone();

            let balances = match ctx.coin.all_balances().await {
                Ok(balances) => balances,
                Err(e) => return Self::change_state(Stopped::unknown(e.to_string())),
            };

            if balances != ctx_balances {
                if let Err(e) = ensure_storage_init(&ctx.coin.all_wallet_ids(), &ctx.storage).await {
                    return Self::change_state(Stopped::storage_error(e));
                }
                // Update balances
                ctx.balances = balances;

                return Self::change_state(FetchingTransactionsData::new(
                    ctx.coin.my_address().expect("my_address can't fail"),
                    ctx.coin.get_denom_and_ticker_map(),
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
    // - claim/create htlcs will display twice
    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        const TX_PAGE_SIZE: u8 = 50;

        async fn is_tx_exists<Storage>(storage: &Storage, wallet_id: &WalletId, internal_id: &BytesJson) -> bool
        where
            Storage: TxHistoryStorage,
        {
            matches!(storage.get_tx_from_history(wallet_id, internal_id).await, Ok(Some(_)))
        }

        async fn fetch_and_insert_txs<Coin, Storage>(
            coin: &Coin,
            storage: &Storage,
            query: String,
            active_assets: &HashMap<String, String>,
        ) -> Result<(), Stopped<Coin, Storage>>
        where
            Coin: TendermintTxHistoryOps,
            Storage: TxHistoryStorage,
        {
            let mut page = 1;
            let mut iterate_more = true;

            let client = try_or_return_stopped_as_err!(
                coin.get_rpc_client().await,
                StopReason::RpcClient,
                "could not get rpc client"
            );
            while iterate_more {
                let response = try_or_return_stopped_as_err!(
                    client
                        .perform(TxSearchRequest::new(
                            query.clone(),
                            false,
                            page,
                            TX_PAGE_SIZE,
                            TendermintResultOrder::Ascending.into(),
                        ))
                        .await,
                    StopReason::RpcClient,
                    "tx search rpc call failed"
                );

                let mut tx_details: HashMap<String, Vec<crate::TransactionDetails>> = HashMap::new();
                for tx in response.txs {
                    let internal_id = H256::from(tx.hash.as_bytes()).reversed().to_vec().into();

                    let deserialized_tx = try_or_return_stopped_as_err!(
                        cosmrs::Tx::from_bytes(tx.tx.as_bytes()),
                        StopReason::Marshaling,
                        "Could not deserialize transaction"
                    );

                    let fee = deserialized_tx.auth_info.fee;

                    let fee_coin = try_or_return_stopped_as_err!(
                        fee.amount.first().ok_or("fee coin can't be empty"),
                        StopReason::Marshaling,
                        "Fee coin is empty"
                    );

                    let fee_amount = try_or_return_stopped_as_err!(
                        fee_coin.amount.to_string().parse(),
                        StopReason::Marshaling,
                        "Fee amount parsing into u64 failed"
                    );

                    let fee_details = TxFeeDetails::Tendermint(TendermintFeeDetails {
                        coin: coin.platform_ticker().to_string(),
                        amount: big_decimal_from_sat_unsigned(fee_amount, coin.decimals()),
                        gas_limit: fee.gas_limit.value(),
                    });

                    let msg = try_or_return_stopped_as_err!(
                        deserialized_tx.body.messages.first().ok_or("Tx body couldn't be read."),
                        StopReason::Marshaling,
                        "Tx body messages is empty"
                    );

                    match msg.type_url.as_str() {
                        SEND_TYPE_URL => {
                            let sent_tx = try_or_return_stopped_as_err!(
                                MsgSend::decode(msg.value.as_slice()),
                                StopReason::Marshaling,
                                "Decoding MsgSend failed"
                            );
                            let coin_amount = try_or_return_stopped_as_err!(
                                sent_tx.amount.first().ok_or("amount can't be empty"),
                                StopReason::Marshaling,
                                "MsgSend amount is empty"
                            );

                            let denom = coin_amount.denom.to_lowercase();
                            let tx_coin_ticker = match active_assets.get(&denom) {
                                Some(t) => t,
                                None => continue,
                            };

                            if is_tx_exists(storage, &WalletId::new(tx_coin_ticker.replace('-', "_")), &internal_id)
                                .await
                            {
                                log::debug!(
                                    "Tx '{}' already exists in tx_history. Skipping it.",
                                    tx.hash.to_string()
                                );
                                continue;
                            } else {
                                log::debug!("Adding tx: '{}' to tx_history.", tx.hash.to_string());
                            }

                            let amount: u64 = try_or_return_stopped_as_err!(
                                coin_amount.amount.parse(),
                                StopReason::Marshaling,
                                "Amount parsing into u64 failed"
                            );
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if sent_tx.from_address == coin.my_address().expect("my_address can't fail") {
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
                                coin: tx_coin_ticker.to_string(),
                                internal_id,
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };

                            match tx_details.entry(tx_coin_ticker.to_string()) {
                                Entry::Vacant(e) => {
                                    e.insert(vec![details]);
                                },
                                Entry::Occupied(mut e) => {
                                    e.get_mut().push(details);
                                },
                            };
                        },
                        CLAIM_HTLC_TYPE_URL => {
                            let htlc_tx = try_or_return_stopped_as_err!(
                                ClaimHtlcProtoRep::decode(msg.value.as_slice()),
                                StopReason::Marshaling,
                                "Decoding ClaimHtlcProtoRep failed"
                            );
                            let htlc_response = try_or_return_stopped_as_err!(
                                coin.query_htlc(htlc_tx.id).await,
                                StopReason::RpcClient,
                                "Htlc query rpc call failed"
                            );

                            let htlc_data = match htlc_response.htlc {
                                Some(htlc) => htlc,
                                None => continue,
                            };

                            match htlc_data.state {
                                HTLC_STATE_OPEN | HTLC_STATE_COMPLETED | HTLC_STATE_REFUNDED => {},
                                _unexpected_state => continue,
                            };

                            let coin_amount = try_or_return_stopped_as_err!(
                                htlc_data.amount.first().ok_or("amount can't be empty"),
                                StopReason::Marshaling,
                                "Htlc amount is empty"
                            );

                            let denom = coin_amount.denom.to_lowercase();
                            let tx_coin_ticker = match active_assets.get(&denom) {
                                Some(t) => t,
                                None => continue,
                            };

                            if is_tx_exists(storage, &WalletId::new(tx_coin_ticker.replace('-', "_")), &internal_id)
                                .await
                            {
                                log::debug!(
                                    "Tx '{}' already exists in tx_history. Skipping it.",
                                    tx.hash.to_string()
                                );
                                continue;
                            } else {
                                log::debug!("Adding tx: '{}' to tx_history.", tx.hash.to_string());
                            }

                            let amount: u64 = try_or_return_stopped_as_err!(
                                coin_amount.amount.parse(),
                                StopReason::Marshaling,
                                "Amount parsing into u64 failed"
                            );
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if htlc_data.sender == coin.my_address().expect("my_address can't fail") {
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
                                coin: tx_coin_ticker.to_string(),
                                internal_id,
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };

                            match tx_details.entry(tx_coin_ticker.to_string()) {
                                Entry::Vacant(e) => {
                                    e.insert(vec![details]);
                                },
                                Entry::Occupied(mut e) => {
                                    e.get_mut().push(details);
                                },
                            };
                        },
                        CREATE_HTLC_TYPE_URL => {
                            let htlc_tx = try_or_return_stopped_as_err!(
                                CreateHtlcProtoRep::decode(msg.value.as_slice()),
                                StopReason::Marshaling,
                                "Decoding CreateHtlcProtoRep failed"
                            );

                            let coin_amount = try_or_return_stopped_as_err!(
                                htlc_tx.amount.first().ok_or("amount can't be empty"),
                                StopReason::Marshaling,
                                "Htlc amount is empty"
                            );

                            let denom = coin_amount.denom.to_lowercase();
                            let tx_coin_ticker = match active_assets.get(&denom) {
                                Some(t) => t,
                                None => continue,
                            };

                            if is_tx_exists(storage, &WalletId::new(tx_coin_ticker.replace('-', "_")), &internal_id)
                                .await
                            {
                                log::debug!(
                                    "Tx '{}' already exists in tx_history. Skipping it.",
                                    tx.hash.to_string()
                                );
                                continue;
                            } else {
                                log::debug!("Adding tx: '{}' to tx_history.", tx.hash.to_string());
                            }

                            let amount: u64 = try_or_return_stopped_as_err!(
                                coin_amount.amount.parse(),
                                StopReason::Marshaling,
                                "Amount parsing into u64 failed"
                            );
                            let amount = big_decimal_from_sat_unsigned(amount, coin.decimals());

                            let (spent_by_me, received_by_me) =
                                if htlc_tx.sender == coin.my_address().expect("my_address can't fail") {
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
                                coin: tx_coin_ticker.to_string(),
                                internal_id,
                                timestamp: common::now_ms() / 1000,
                                kmd_rewards: None,
                                transaction_type: Default::default(),
                            };

                            match tx_details.entry(tx_coin_ticker.to_string()) {
                                Entry::Vacant(e) => {
                                    e.insert(vec![details]);
                                },
                                Entry::Occupied(mut e) => {
                                    e.get_mut().push(details);
                                },
                            };
                        },
                        _ => {},
                    }
                }

                for (ticker, txs) in tx_details.into_iter() {
                    let id = WalletId::new(ticker.replace('-', "_"));

                    try_or_return_stopped_as_err!(
                        storage
                            .add_transactions_to_history(&id, txs)
                            .await
                            .map_err(|e| format!("{:?}", e)),
                        StopReason::StorageError,
                        "add_transactions_to_history failed"
                    );
                }

                iterate_more = (TX_PAGE_SIZE as u32 * page) < response.total_count;
                page += 1;
            }

            Ok(())
        }

        let address = ctx.coin.my_address().expect("my_address can't fail");

        let q = format!("transfer.sender = '{}'", address.clone());
        if let Err(stopped) = fetch_and_insert_txs(&ctx.coin, &ctx.storage, q, &self.active_assets).await {
            return Self::change_state(stopped);
        };

        let q = format!("transfer.recipient = '{}'", self.address.clone());
        if let Err(stopped) = fetch_and_insert_txs(&ctx.coin, &ctx.storage, q, &self.active_assets).await {
            return Self::change_state(stopped);
        };

        log::info!("Tx history fetching finished for tendermint");
        ctx.coin.set_history_sync_state(HistorySyncState::Finished);
        Self::change_state(WaitForHistoryUpdateTrigger::new())
    }
}

async fn ensure_storage_init<Storage>(wallet_ids: &[WalletId], storage: &Storage) -> MmResult<(), String>
where
    Storage: TxHistoryStorage,
{
    for wallet_id in wallet_ids.iter() {
        if let Err(e) = storage.init(wallet_id).await {
            return MmError::err(format!("{:?}", e));
        }
    }

    Ok(())
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

        if let Err(e) = ensure_storage_init(&ctx.coin.all_wallet_ids(), &ctx.storage).await {
            return Self::change_state(Stopped::storage_error(e));
        }

        Self::change_state(FetchingTransactionsData::new(
            ctx.coin.my_address().expect("my_address can't fail"),
            ctx.coin.get_denom_and_ticker_map(),
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
                "code": crate::utxo::utxo_common::HISTORY_TOO_LARGE_ERR_CODE,
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

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError> { self.all_balances().await }

    fn all_wallet_ids(&self) -> Vec<WalletId> {
        let tokens = self.tokens_info.lock();
        let mut ids = vec![];

        ids.push(WalletId::new(self.ticker().replace('-', "_")));

        for (ticker, _info) in tokens.iter() {
            ids.push(WalletId::new(ticker.replace('-', "_")));
        }

        ids
    }

    fn get_denom_and_ticker_map(&self) -> HashMap<String, String> {
        let tokens = self.tokens_info.lock();

        let mut map = HashMap::with_capacity(tokens.len() + 1);
        map.insert(
            self.denom.to_string().to_lowercase(),
            self.ticker().to_string().to_uppercase(),
        );

        for (ticker, info) in tokens.iter() {
            map.insert(info.denom.to_string().to_lowercase(), ticker.to_uppercase());
        }

        map
    }
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
