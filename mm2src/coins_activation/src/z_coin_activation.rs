use crate::context::CoinsActivationContext;
use crate::prelude::*;
use crate::standalone_coin::{InitStandaloneCoinActivationOps, InitStandaloneCoinError,
                             InitStandaloneCoinInitialStatus, InitStandaloneCoinTaskHandle,
                             InitStandaloneCoinTaskManagerShared};
use async_trait::async_trait;
use coins::coin_balance::EnableCoinBalance;
use coins::utxo::rpc_clients::ElectrumRpcRequest;
use coins::utxo::{UtxoActivationParams, UtxoRpcMode};
use coins::z_coin::{z_coin_from_conf_and_params, ZCoin, ZCoinBuildError};
use coins::{CoinProtocol, PrivKeyBuildPolicy, RegisterCoinError};
use common::executor::Timer;
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
    Scanning,
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
    pub requires_notarization: Option<bool>,
}

impl TxHistoryEnabled for ZcoinActivationParams {
    fn tx_history_enabled(&self) -> bool { false }
}

#[derive(Clone, Display, Serialize, SerializeErrorType)]
#[serde(tag = "error_type", content = "error_data")]
pub enum ZcoinInitError {
    #[display(fmt = "Error on coin {} creation: {}", ticker, error)]
    CoinCreationError {
        ticker: String,
        error: String,
    },
    CoinIsAlreadyActivated {
        ticker: String,
    },
    HardwareWalletsAreNotSupportedYet,
    Internal(String),
}

impl FromRegisterErr for ZcoinInitError {
    fn from_register_err(reg_err: RegisterCoinError, ticker: String) -> ZcoinInitError {
        match reg_err {
            RegisterCoinError::CoinIsInitializedAlready { coin } => {
                ZcoinInitError::CoinIsAlreadyActivated { ticker: coin }
            },
            RegisterCoinError::ErrorGettingBlockCount(error) => ZcoinInitError::CoinCreationError { ticker, error },
            RegisterCoinError::Internal(internal) => ZcoinInitError::Internal(internal),
        }
    }
}

impl From<ZcoinInitError> for InitStandaloneCoinError {
    fn from(_: ZcoinInitError) -> Self { todo!() }
}

impl ZcoinInitError {
    pub fn from_build_err(build_err: ZCoinBuildError, ticker: String) -> Self {
        ZcoinInitError::CoinCreationError {
            ticker,
            error: build_err.to_string(),
        }
    }
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
        activation_request: &ZcoinActivationParams,
        _protocol_info: ZcoinProtocolInfo,
        priv_key_policy: PrivKeyBuildPolicy<'_>,
        task_handle: &ZcoinRpcTaskHandle,
    ) -> MmResult<Self, ZcoinInitError> {
        let priv_key = match priv_key_policy {
            PrivKeyBuildPolicy::IguanaPrivKey(key) => key,
            PrivKeyBuildPolicy::HardwareWallet => {
                return MmError::err(ZcoinInitError::HardwareWalletsAreNotSupportedYet)
            },
        };
        let utxo_mode = match &activation_request.mode {
            ZcoinRpcMode::Native => UtxoRpcMode::Native,
            ZcoinRpcMode::Light { electrum_servers, .. } => UtxoRpcMode::Electrum {
                servers: electrum_servers.clone(),
            },
        };
        let utxo_params = UtxoActivationParams {
            mode: utxo_mode,
            utxo_merge_params: None,
            tx_history: false,
            required_confirmations: activation_request.required_confirmations,
            requires_notarization: activation_request.requires_notarization,
            address_format: None,
            gap_limit: None,
            scan_policy: Default::default(),
            check_utxo_maturity: None,
        };
        let coin = z_coin_from_conf_and_params(&ctx, &ticker, &coin_conf, &utxo_params, &priv_key)
            .await
            .mm_err(|e| ZcoinInitError::from_build_err(e, ticker))?;

        task_handle.update_in_progress_status(ZcoinInProgressStatus::Scanning)?;
        while !coin.is_sapling_state_synced() {
            Timer::sleep(1.).await;
        }
        Ok(coin)
    }

    async fn get_activation_result(
        &self,
        ctx: MmArc,
        task_handle: &ZcoinRpcTaskHandle,
        activation_request: &Self::ActivationRequest,
    ) -> MmResult<Self::ActivationResult, ZcoinInitError> {
        task_handle.update_in_progress_status(ZcoinInProgressStatus::RequestingWalletBalance)?;
        unimplemented!()
        // get_activation_result(self, task_handle).await
    }
}
