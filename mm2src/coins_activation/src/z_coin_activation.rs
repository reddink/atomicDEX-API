use crate::context::CoinsActivationContext;
use crate::prelude::TryFromCoinProtocol;
use crate::standalone_coin::{InitStandaloneCoinActivationOps, InitStandaloneCoinError,
                             InitStandaloneCoinInitialStatus, InitStandaloneCoinTaskHandle,
                             InitStandaloneCoinTaskManagerShared};
use async_trait::async_trait;
use coins::coin_balance::EnableCoinBalance;
use coins::hd_pubkey::RpcTaskXPubExtractor;
use coins::utxo::rpc_clients::ElectrumRpcRequest;
use coins::utxo::utxo_builder::UtxoArcBuilder;
use coins::z_coin::ZCoin;
use coins::{lp_register_coin, CoinProtocol, MmCoinEnum, PrivKeyBuildPolicy, RegisterCoinParams};
use common::mm_ctx::MmArc;
use common::mm_error::prelude::*;
use crypto::hw_rpc_task::{HwRpcTaskAwaitingStatus, HwRpcTaskUserAction};
use derive_more::Display;
use ser_error_derive::SerializeErrorType;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as Json;

pub type ZcoinTaskManagerShared = InitStandaloneCoinTaskManagerShared<ZCoin>;
pub type ZcoinRpcTaskHandle = InitStandaloneCoinTaskHandle<ZCoin>;
pub type ZcoinAwaitingStatus = HwRpcTaskAwaitingStatus;
pub type ZcoinUserAction = HwRpcTaskUserAction;

#[derive(Clone, Serialize)]
pub struct ZcoinActivationResult {
    pub current_block: u64,
    pub wallet_balance: EnableCoinBalance,
}

#[derive(Clone, Serialize)]
pub enum ZcoinInProgressStatus {
    ActivatingCoin,
    ScanningBlocks,
    RequestingWalletBalance,
    Finishing,
    /// This status doesn't require the user to send `UserAction`,
    /// but it tells the user that he should confirm/decline an address on his device.
    WaitingForTrezorToConnect,
    WaitingForUserToConfirmPubkey,
}

impl InitStandaloneCoinInitialStatus for ZcoinInProgressStatus {
    fn initial_status() -> Self { ZcoinInProgressStatus::ActivatingCoin }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "rpc", content = "rpc_data")]
pub enum ZcoinRpcMode {
    Native,
    Light {
        electrum_servers: Vec<ElectrumRpcRequest>,
        light_wallet_d_servers: Vec<String>,
    },
}

#[derive(Deserialize)]
pub struct ZcoinActivationParams {
    pub mode: ZcoinRpcMode,
    pub required_confirmations: Option<u64>,
}

#[derive(Clone, Display, Serialize, SerializeErrorType)]
#[serde(tag = "error_type", content = "error_data")]
pub enum ZcoinInitError {}

impl From<ZcoinInitError> for InitStandaloneCoinError {
    fn from(_: ZcoinInitError) -> Self { todo!() }
}

pub struct ZcoinProtocolInfo;

impl TryFromCoinProtocol for ZcoinProtocolInfo {
    fn try_from_coin_protocol(proto: CoinProtocol) -> Result<Self, MmError<CoinProtocol>>
    where
        Self: Sized,
    {
        match proto {
            CoinProtocol::ZHTLC => Ok(ZcoinProtocolInfo),
            protocol => MmError::err(protocol),
        }
    }
}

#[async_trait]
impl InitStandaloneCoinActivationOps for ZCoin {
    type ActivationRequest = ZcoinActivationParams;
    type StandaloneProtocol = ZcoinProtocolInfo;
    type ActivationResult = ZcoinActivationResult;
    type ActivationError = ZcoinInitError;
    type InProgressStatus = ZcoinInProgressStatus;
    type AwaitingStatus = ZcoinAwaitingStatus;
    type UserAction = ZcoinUserAction;

    fn rpc_task_manager(activation_ctx: &CoinsActivationContext) -> &ZcoinTaskManagerShared {
        &activation_ctx.init_z_coin_task_manager
    }

    async fn init_standalone_coin(
        ctx: MmArc,
        ticker: String,
        coin_conf: Json,
        activation_request: Self::ActivationRequest,
        _protocol_info: Self::StandaloneProtocol,
        priv_key_policy: PrivKeyBuildPolicy<'_>,
        task_handle: &ZcoinRpcTaskHandle,
    ) -> MmResult<Self, ZcoinInitError> {
        /*
        // Construct an Xpub extractor without checking if the MarketMaker supports HD wallet ops.
        // If the coin builder tries to extract an extended public key despite HD wallet is not supported,
        // [`UtxoCoinBuilder::build`] fails with the [`UtxoCoinBuildError::IguanaPrivKeyNotAllowed`] error.
        let xpub_extractor = RpcTaskXPubExtractor::new_unchecked(&ctx, task_handle, xpub_extractor_rpc_statuses());
        let coin = UtxoArcBuilder::new(
            &ctx,
            &ticker,
            &coin_conf,
            &activation_request,
            priv_key_policy,
            xpub_extractor,
            ZCoin::from,
        )
        .build()
        .await
        .mm_err(|e| InitUtxoStandardError::from_build_err(e, ticker.clone()))?;
        lp_register_coin(&ctx, MmCoinEnum::from(coin.clone()), RegisterCoinParams {
            ticker: ticker.clone(),
            tx_history: true,
        })
        .await
        .mm_err(|e| InitUtxoStandardError::from_register_err(e, ticker))?;
        Ok(coin)

         */
        unimplemented!()
    }

    async fn get_activation_result(
        &self,
        task_handle: &ZcoinRpcTaskHandle,
    ) -> MmResult<Self::ActivationResult, ZcoinInitError> {
        unimplemented!()
        // get_activation_result(self, task_handle).await
    }
}
