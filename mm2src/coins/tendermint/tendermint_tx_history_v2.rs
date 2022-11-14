#![allow(warnings)]

use super::{rpc::*, TendermintCoin, TendermintCoinRpcError, TendermintToken};

use crate::my_tx_history_v2::{CoinWithTxHistoryV2, MyTxHistoryErrorV2, MyTxHistoryTarget, TxHistoryStorage};
use crate::tx_history_storage::{GetTxHistoryFilters, WalletId};
use crate::utxo::utxo_tx_history_v2::StopReason;
use crate::{HistorySyncState, MarketCoinOps};
use async_trait::async_trait;
use common::log;
use common::state_machine::prelude::*;
use cosmrs::rpc::{Client, HttpClient};
use mm2_err_handle::prelude::MmResult;
use mm2_metrics::MetricsArc;
use mm2_number::BigDecimal;
use prost::Message;
use std::collections::HashMap;

#[async_trait]
pub trait TendermintTxHistoryOps: CoinWithTxHistoryV2 + MarketCoinOps + Send + Sync + 'static {
    // Returns addresses for those we need to request Transaction history.
    // fn my_address(&self) -> AccountId;

    async fn get_rpc_client(&self) -> MmResult<HttpClient, TendermintCoinRpcError>;

    // Returns Transaction details by hash using the coin RPC if required.
    // async fn tx_details_by_hash<T>(
    //     &self,
    //     params: UtxoTxDetailsParams<'_, T>,
    // ) -> MmResult<Vec<TransactionDetails>, UtxoTxDetailsError>
    // where
    //     T: TxHistoryStorage;

    // Loads transaction from `storage` or requests it using coin RPC.
    // async fn tx_from_storage_or_rpc<Storage: TxHistoryStorage>(
    //     &self,
    //     tx_hash: &H256Json,
    //     storage: &Storage,
    // ) -> MmResult<UtxoTx, UtxoTxDetailsError>;

    // Requests transaction history.
    // async fn request_tx_history(&self, metrics: MetricsArc, for_addresses: &HashSet<Address>)
    //     -> RequestTxHistoryResult;

    // Requests timestamp of the given block.
    // async fn get_block_timestamp(&self, height: u64) -> MmResult<u64, GetBlockHeaderError>;

    // Requests balances of all activated coin's addresses.
    // async fn my_addresses_balances(&self) -> BalanceResult<HashMap<String, BigDecimal>>;

    // fn address_from_str(&self, address: &str) -> MmResult<Address, AddrFromStrError>;

    /// Sets the history sync state.
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

////////////////////////////////
////////////////////////////////
////////////////////////////////
////////////////////////////////
////////////////////////////////
////////////////////////////////
////////////////////////////////
////////////////////////////////

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
// impl<Coin, Storage> TransitionFrom<WaitForHistoryUpdateTrigger<Coin, Storage>> for Stopped<Coin, Storage> {}
// impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>> for Stopped<Coin, Storage> {}
// impl<Coin, Storage> TransitionFrom<FetchingTransactionsData<Coin, Storage>>
//     for WaitForHistoryUpdateTrigger<Coin, Storage>
// {
// }

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

        async fn query_transactions(client: &HttpClient, query: String, page: u32) {
            let response = client
                .perform(TxSearchRequest::new(
                    query,
                    false,
                    page,
                    TX_PAGE_SIZE,
                    TendermintResultOrder::Ascending.into(),
                ))
                .await
                .unwrap();

            for tx in response.txs {
                let _height = tx.height;
                let _hash = tx.hash;

                let deserialized_tx = cosmrs::Tx::from_bytes(tx.tx.as_bytes()).unwrap();
                let msg = deserialized_tx
                    .body
                    .messages
                    .first()
                    .ok_or("Tx body couldn't be read.")
                    .unwrap();

                println!(
                    "MSG_TYPE: {} TX_HEIGHT: {} TX_HASH: {}",
                    msg.type_url, tx.height, tx.hash
                );
            }
        }

        let rpc_client = ctx.coin.get_rpc_client().await.unwrap();

        let address = "iaa1e0rx87mdj79zejewuc4jg7ql9ud2286g2us8f2".to_string();
        let q = format!("transfer.sender = '{}'", address);
        query_transactions(&rpc_client, q, 1).await;
        todo!()
        // let q = format!("transfer.recipient = '{}'", self.address.clone());
        // query_transactions(&rpc_client, q, 1);

        // Get tx as sender and receiver
        // Order By Asc
        // Iterate via pagination values
        // Insert if not exists in storage
        // If all complete, wait for trigger
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
        println!("MUST BE EXECUTED AFTER");
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
