use super::iris::htlc_proto::QueryHtlcResponseProto;
use super::{rpc::*, AllBalancesResult, TendermintCoin, TendermintCoinRpcError, TendermintToken};

use crate::my_tx_history_v2::{CoinWithTxHistoryV2, MyTxHistoryErrorV2, MyTxHistoryTarget, TxHistoryStorage};
use crate::tendermint::TendermintFeeDetails;
use crate::tx_history_storage::{GetTxHistoryFilters, WalletId};
use crate::utxo::utxo_common::big_decimal_from_sat_unsigned;
use crate::{HistorySyncState, MarketCoinOps, TransactionDetails, TransactionType, TxFeeDetails};
use async_trait::async_trait;
use bitcrypto::sha256;
use common::executor::Timer;
use common::log;
use common::number_type_casting::SafeTypeCastingNumbers;
use common::state_machine::prelude::*;
use cosmrs::tendermint::abci::Event;
use cosmrs::tx::Fee;
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::MmResult;
use mm2_number::BigDecimal;
use primitives::hash::H256;
use rpc::v1::types::Bytes as BytesJson;
use serde_json::Value as Json;
use std::cmp;

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

macro_rules! try_or_continue {
    ($exp:expr, $fmt:literal) => {
        match $exp {
            Ok(t) => t,
            Err(e) => {
                log::debug!("{}: {}", $fmt, e);
                continue;
            },
        }
    };
}

#[async_trait]
pub trait TendermintTxHistoryOps: CoinWithTxHistoryV2 + MarketCoinOps + Send + Sync + 'static {
    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError>;

    async fn query_htlc(&self, id: String) -> MmResult<QueryHtlcResponseProto, TendermintCoinRpcError>;

    fn decimals(&self) -> u8;

    fn platform_denom(&self) -> String;

    fn set_history_sync_state(&self, new_state: HistorySyncState);

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError>;
}

#[async_trait]
impl CoinWithTxHistoryV2 for TendermintCoin {
    fn history_wallet_id(&self) -> WalletId { WalletId::new(self.ticker().into()) }

    async fn get_tx_history_filters(
        &self,
        _target: MyTxHistoryTarget,
    ) -> MmResult<GetTxHistoryFilters, MyTxHistoryErrorV2> {
        Ok(GetTxHistoryFilters::for_address(self.account_id.to_string()))
    }
}

#[async_trait]
impl CoinWithTxHistoryV2 for TendermintToken {
    fn history_wallet_id(&self) -> WalletId { WalletId::new(self.platform_ticker().into()) }

    async fn get_tx_history_filters(
        &self,
        _target: MyTxHistoryTarget,
    ) -> MmResult<GetTxHistoryFilters, MyTxHistoryErrorV2> {
        let denom_hash = sha256(self.denom.to_string().as_bytes());
        let id = H256::from(denom_hash.as_slice());

        Ok(GetTxHistoryFilters::for_address(self.platform_coin.account_id.to_string()).with_token_id(id.to_string()))
    }
}

struct TendermintTxHistoryCtx<Coin: TendermintTxHistoryOps, Storage: TxHistoryStorage> {
    coin: Coin,
    storage: Storage,
    mm_ctx: MmArc,
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
    StorageError(String),
    RpcClient(String),
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
}

struct WaitForHistoryUpdateTrigger<Coin, Storage> {
    address: String,
    last_height_state: u64,
    tendermint_assets_conf: Vec<Json>,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> WaitForHistoryUpdateTrigger<Coin, Storage> {
    fn new(address: String, last_height_state: u64, tendermint_assets_conf: Vec<Json>) -> Self {
        WaitForHistoryUpdateTrigger {
            address,
            last_height_state,
            tendermint_assets_conf,
            phantom: Default::default(),
        }
    }
}

struct OnIoErrorCooldown<Coin, Storage> {
    address: String,
    last_block_height: u64,
    tendermint_assets_conf: Vec<Json>,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> OnIoErrorCooldown<Coin, Storage> {
    fn new(address: String, last_block_height: u64, tendermint_assets_conf: Vec<Json>) -> Self {
        OnIoErrorCooldown {
            address,
            last_block_height,
            tendermint_assets_conf,
            phantom: Default::default(),
        }
    }
}

impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>> for OnIoErrorCooldown<Coin, Storage> {}

#[async_trait]
impl<Coin, Storage> State for OnIoErrorCooldown<Coin, Storage>
where
    Coin: TendermintTxHistoryOps,
    Storage: TxHistoryStorage,
{
    type Ctx = TendermintTxHistoryCtx<Coin, Storage>;
    type Result = ();

    async fn on_changed(mut self: Box<Self>, _ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        Timer::sleep(30.).await;

        // retry history fetching process from last saved block
        return Self::change_state(FetchingTransactionsData::new(
            self.address,
            self.last_block_height,
            self.tendermint_assets_conf,
        ));
    }
}

struct FetchingTransactionsData<Coin, Storage> {
    /// The list of addresses for those we have requested [`UpdatingUnconfirmedTxes::all_tx_ids_with_height`] TX hashses
    /// at the `FetchingTxHashes` state.
    address: String,
    from_block_height: u64,
    tendermint_assets_conf: Vec<Json>,
    phantom: std::marker::PhantomData<(Coin, Storage)>,
}

impl<Coin, Storage> FetchingTransactionsData<Coin, Storage> {
    fn new(address: String, from_block_height: u64, tendermint_assets_conf: Vec<Json>) -> Self {
        FetchingTransactionsData {
            address,
            phantom: Default::default(),
            tendermint_assets_conf,
            from_block_height,
        }
    }
}

impl<Coin, Storage> TransitionFrom<TendermintInit<Coin, Storage>> for Stopped<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<TendermintInit<Coin, Storage>> for FetchingTransactionsData<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<OnIoErrorCooldown<Coin, Storage>> for FetchingTransactionsData<Coin, Storage> {}
impl<Coin, Storage> TransitionFrom<WaitForHistoryUpdateTrigger<Coin, Storage>> for OnIoErrorCooldown<Coin, Storage> {}
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
                Err(_) => {
                    return Self::change_state(OnIoErrorCooldown::new(
                        self.address.clone(),
                        self.last_height_state,
                        self.tendermint_assets_conf,
                    ));
                },
            };

            if balances != ctx_balances {
                // Update balances
                ctx.balances = balances;

                return Self::change_state(FetchingTransactionsData::new(
                    self.address.clone(),
                    self.last_height_state,
                    self.tendermint_assets_conf,
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

    async fn on_changed(self: Box<Self>, ctx: &mut Self::Ctx) -> StateResult<Self::Ctx, Self::Result> {
        const TX_PAGE_SIZE: u8 = 50;
        const DEFAULT_DECIMALS: u8 = 6;
        const DEFAULT_TRANSFER_EVENT_COUNT: usize = 2;
        const AMOUNT_TAG_KEY: &str = "amount";
        const RECEIVER_TAG_KEY: &str = "receiver";
        const SPENDER_TAG_KEY: &str = "spender";
        const COIN_RECEIVED_EVENT: &str = "coin_received";
        const COIN_SPENT_EVENT: &str = "coin_spent";

        struct TxAmounts {
            total: BigDecimal,
            spent_by_me: BigDecimal,
            received_by_me: BigDecimal,
            my_balance_change: BigDecimal,
        }

        fn get_decimals_by_denom(tendermint_assets_conf: &[Json], denom: &str) -> Option<u8> {
            let coin = tendermint_assets_conf.iter().find(|coin| {
                coin["protocol"]["protocol_data"]["denom"]
                    .as_str()
                    // just to be sure for case sensivity
                    .map(|t| t.to_lowercase())
                    == Some(denom.to_lowercase())
            })?;

            coin["protocol"]["protocol_data"]["decimals"]
                .as_u64()
                .map(|d| d.into_or(DEFAULT_DECIMALS))
        }

        fn get_tx_amounts(amount: u64, decimals: u8, sent_by_me: bool, is_self_transfer: bool) -> TxAmounts {
            let amount = big_decimal_from_sat_unsigned(amount, decimals);

            let spent_by_me = if sent_by_me && !is_self_transfer {
                amount.clone()
            } else {
                BigDecimal::default()
            };

            let received_by_me = if !sent_by_me || is_self_transfer {
                amount.clone()
            } else {
                BigDecimal::default()
            };

            TxAmounts {
                total: amount,
                my_balance_change: received_by_me.clone() - spent_by_me.clone(),
                spent_by_me,
                received_by_me,
            }
        }

        fn get_fee_details<Coin>(fee: Fee, coin: &Coin) -> Result<TendermintFeeDetails, String>
        where
            Coin: TendermintTxHistoryOps,
        {
            let fee_coin = fee
                .amount
                .first()
                .ok_or_else(|| "fee coin can't be empty".to_string())?;
            let fee_amount: u64 = fee_coin.amount.to_string().parse().map_err(|e| format!("{:?}", e))?;

            Ok(TendermintFeeDetails {
                coin: coin.platform_ticker().to_string(),
                amount: big_decimal_from_sat_unsigned(fee_amount, coin.decimals()),
                gas_limit: fee.gas_limit.value(),
            })
        }

        struct TransferDetails {
            from: String,
            to: String,
            denom: String,
            amount: u64,
        }

        fn parse_transfer_values_from_events(tx_events: Vec<&Event>) -> Option<TransferDetails> {
            let mut from: Option<String> = None;
            let mut to: Option<String> = None;
            let mut denom: Option<String> = None;
            let mut amount: Option<u64> = None;

            for event in tx_events {
                if amount.is_none() && denom.is_none() {
                    let amount_with_denom = event
                        .attributes
                        .iter()
                        .find(|tag| tag.key.to_string() == AMOUNT_TAG_KEY)?
                        .value
                        .to_string();

                    let extracted_amount: String = amount_with_denom.chars().take_while(|c| c.is_numeric()).collect();

                    denom = Some(amount_with_denom.split(&extracted_amount).collect());
                    amount = Some(extracted_amount.parse().ok()?);
                }

                match event.type_str.as_str() {
                    COIN_SPENT_EVENT if from.is_none() => {
                        from = Some(
                            event
                                .attributes
                                .iter()
                                .find(|tag| tag.key.to_string() == SPENDER_TAG_KEY)?
                                .value
                                .to_string(),
                        );
                    },
                    COIN_RECEIVED_EVENT if to.is_none() => {
                        to = Some(
                            event
                                .attributes
                                .iter()
                                .find(|tag| tag.key.to_string() == RECEIVER_TAG_KEY)?
                                .value
                                .to_string(),
                        );
                    },
                    _ => {},
                }
            }

            Some(TransferDetails {
                from: from?,
                to: to?,
                denom: denom?,
                amount: amount?,
            })
        }

        fn get_transfer_details(tx_events: Vec<Event>, fee_amount_with_denom: String) -> Option<TransferDetails> {
            // Filter out irrelevant events
            let mut events: Vec<&Event> = tx_events
                .iter()
                .filter(|event| [COIN_SPENT_EVENT, COIN_RECEIVED_EVENT].contains(&event.type_str.as_str()))
                .collect();

            if events.len() > DEFAULT_TRANSFER_EVENT_COUNT {
                // Retain fee related events
                events.retain(|event| {
                    let amount_with_denom = event
                        .attributes
                        .iter()
                        .find(|tag| tag.key.to_string() == AMOUNT_TAG_KEY)
                        .map(|t| t.value.to_string());

                    amount_with_denom != Some(fee_amount_with_denom.clone())
                });
            }

            parse_transfer_values_from_events(events)
        }

        async fn fetch_and_insert_txs<Coin, Storage>(
            tendermint_assets_conf: &[Json],
            address: String,
            coin: &Coin,
            storage: &Storage,
            query: String,
            from_height: u64,
        ) -> Result<u64, Stopped<Coin, Storage>>
        where
            Coin: TendermintTxHistoryOps,
            Storage: TxHistoryStorage,
        {
            let mut page = 1;
            let mut iterate_more = true;
            let mut highest_height = from_height;

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

                let mut tx_details = vec![];
                for tx in response.txs {
                    let internal_id = H256::from(tx.hash.as_bytes()).reversed().to_vec().into();
                    let tx_hash = tx.hash.to_string();

                    if let Ok(Some(_)) = storage
                        .get_tx_from_history(&coin.history_wallet_id(), &internal_id)
                        .await
                    {
                        log::debug!("Tx '{}' already exists in tx_history. Skipping it.", &tx_hash);
                        continue;
                    }

                    highest_height = cmp::max(highest_height, tx.height.into());

                    let deserialized_tx = try_or_continue!(
                        cosmrs::Tx::from_bytes(tx.tx.as_bytes()),
                        "Could not deserialize transaction"
                    );

                    let fee_data = match deserialized_tx.auth_info.fee.amount.first() {
                        Some(data) => data,
                        None => {
                            log::debug!("Could not read transaction fee for tx '{}', skipping it", &tx_hash);
                            continue;
                        },
                    };

                    let fee_amount_with_denom = format!("{}{}", fee_data.amount, fee_data.denom);

                    let transfer_details = match get_transfer_details(tx.tx_result.events, fee_amount_with_denom) {
                        Some(td) => td,
                        None => {
                            log::debug!(
                                "Could not parse transfer details from events for tx '{}', skipping it",
                                &tx_hash
                            );
                            continue;
                        },
                    };

                    // TODO
                    // push platform coin fee as tx for each token txs
                    // if transfer_details.denom != coin.platform_denom() {}

                    let decimals = match get_decimals_by_denom(tendermint_assets_conf, &transfer_details.denom) {
                        Some(d) => d,
                        None => {
                            log::debug!(
                                "Denom '{}' is not supported in current coins configuration, skipping it",
                                &transfer_details.denom
                            );
                            continue;
                        },
                    };

                    let tx_amounts = get_tx_amounts(
                        transfer_details.amount,
                        decimals,
                        address.clone() == transfer_details.from,
                        transfer_details.to == transfer_details.from,
                    );

                    let fee_details = try_or_continue!(
                        get_fee_details(deserialized_tx.auth_info.fee, coin),
                        "get_fee_details failed"
                    );

                    let transaction_type = if transfer_details.denom == coin.platform_denom() {
                        TransactionType::StandardTransfer
                    } else {
                        let denom_hash = sha256(transfer_details.denom.clone().as_bytes());
                        let token_id: BytesJson = H256::from(denom_hash.as_slice()).to_vec().into();
                        TransactionType::TokenTransfer(token_id)
                    };

                    let msg = try_or_continue!(
                        deserialized_tx.body.messages.first().ok_or("Tx body couldn't be read."),
                        "Tx body messages is empty"
                    );

                    let details = TransactionDetails {
                        from: vec![transfer_details.from],
                        to: vec![transfer_details.to],
                        total_amount: tx_amounts.total,
                        spent_by_me: tx_amounts.spent_by_me,
                        received_by_me: tx_amounts.received_by_me,
                        my_balance_change: tx_amounts.my_balance_change,
                        tx_hash,
                        tx_hex: msg.value.as_slice().into(),
                        fee_details: Some(TxFeeDetails::Tendermint(fee_details)),
                        block_height: tx.height.into(),
                        coin: transfer_details.denom,
                        internal_id,
                        timestamp: common::now_ms() / 1000,
                        kmd_rewards: None,
                        transaction_type,
                    };

                    tx_details.push(details);
                    log::debug!("Tx '{}' successfuly parsed.", tx.hash);
                }

                try_or_return_stopped_as_err!(
                    storage
                        .add_transactions_to_history(&coin.history_wallet_id(), tx_details)
                        .await
                        .map_err(|e| format!("{:?}", e)),
                    StopReason::StorageError,
                    "add_transactions_to_history failed"
                );

                iterate_more = (TX_PAGE_SIZE as u32 * page) < response.total_count;
                page += 1;
            }

            Ok(highest_height)
        }

        let q = format!(
            "coin_spent.spender = '{}' AND tx.height > {}",
            self.address.clone(),
            self.from_block_height
        );
        let highest_send_tx_height = match fetch_and_insert_txs(
            &self.tendermint_assets_conf,
            self.address.clone(),
            &ctx.coin,
            &ctx.storage,
            q,
            self.from_block_height,
        )
        .await
        {
            Ok(block) => block,
            Err(stopped) => {
                if let StopReason::RpcClient(e) = &stopped.stop_reason {
                    log::error!("Sent tx history process turned into cooldown mode due to rpc error: {e}");
                    return Self::change_state(OnIoErrorCooldown::new(
                        self.address.clone(),
                        self.from_block_height,
                        self.tendermint_assets_conf,
                    ));
                }

                return Self::change_state(stopped);
            },
        };

        let q = format!(
            "coin_received.receiver = '{}' AND tx.height > {}",
            self.address.clone(),
            self.from_block_height
        );
        let highest_recieved_tx_height = match fetch_and_insert_txs(
            &self.tendermint_assets_conf,
            self.address.clone(),
            &ctx.coin,
            &ctx.storage,
            q,
            self.from_block_height,
        )
        .await
        {
            Ok(block) => block,
            Err(stopped) => {
                if let StopReason::RpcClient(e) = &stopped.stop_reason {
                    log::error!("Received tx history process turned into cooldown mode due to rpc error: {e}");
                    return Self::change_state(OnIoErrorCooldown::new(
                        self.address.clone(),
                        self.from_block_height,
                        self.tendermint_assets_conf,
                    ));
                }

                return Self::change_state(stopped);
            },
        };

        let last_fetched_block = cmp::max(highest_send_tx_height, highest_recieved_tx_height);

        log::info!(
            "Tx history fetching finished for tendermint. Last fetched block {}",
            last_fetched_block
        );

        ctx.coin.set_history_sync_state(HistorySyncState::Finished);
        Self::change_state(WaitForHistoryUpdateTrigger::new(
            self.address.clone(),
            last_fetched_block,
            self.tendermint_assets_conf,
        ))
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
        const INITIAL_SEARCH_HEIGHT: u64 = 0;
        const COIN_PROTOCOL_TYPE: &str = "TENDERMINT";
        const TOKEN_PROTOCOL_TYPE: &str = "TENDERMINTTOKEN";

        ctx.coin.set_history_sync_state(HistorySyncState::NotStarted);

        if let Err(e) = ctx.storage.init(&ctx.coin.history_wallet_id()).await {
            return Self::change_state(Stopped::storage_error(e));
        }

        let tendermint_assets: Vec<Json> = ctx.mm_ctx.conf["coins"]
            .as_array()
            .expect("couldn't read coins context")
            .iter()
            .map(|coin| coin.to_owned())
            .filter(|coin| {
                matches!(
                    coin["protocol"]["type"].as_str(),
                    Some(COIN_PROTOCOL_TYPE) | Some(TOKEN_PROTOCOL_TYPE)
                )
            })
            .collect();

        Self::change_state(FetchingTransactionsData::new(
            ctx.coin.my_address().expect("my_address can't fail"),
            INITIAL_SEARCH_HEIGHT,
            tendermint_assets,
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

        let new_state_json = json!({
            "message": format!("{:?}", self.stop_reason),
        });

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

    fn platform_denom(&self) -> String { self.denom.to_string() }

    fn set_history_sync_state(&self, new_state: HistorySyncState) {
        *self.history_sync_state.lock().unwrap() = new_state;
    }

    async fn all_balances(&self) -> MmResult<AllBalancesResult, TendermintCoinRpcError> { self.all_balances().await }
}

pub async fn tendermint_history_loop(
    coin: TendermintCoin,
    storage: impl TxHistoryStorage,
    ctx: MmArc,
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
        mm_ctx: ctx,
        balances,
    };

    let state_machine: StateMachine<_, ()> = StateMachine::from_ctx(ctx);
    state_machine.run(TendermintInit::new()).await;
}
