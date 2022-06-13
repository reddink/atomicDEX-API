use crate::hd_pubkey::HDXPubExtractor;
use crate::hd_wallet_storage::HDWalletStorageError;
use crate::{BalanceError, CoinFindError, UnexpectedDerivationMethod, WithdrawError};
use async_trait::async_trait;
use common::HttpStatusCode;
use crypto::{Bip32DerPathError, Bip32Error, Bip44Chain, Bip44DerPathError, Bip44DerivationPath, DerivationPath,
             HwError};
use derive_more::Display;
use http::StatusCode;
use mm2_err_handle::prelude::*;
use rpc_task::RpcTaskError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub use futures::lock::{MappedMutexGuard as AsyncMappedMutexGuard, Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

pub type HDAccountsMap<HDAccount> = BTreeMap<u32, HDAccount>;
pub type HDAccountsMutex<HDAccount> = AsyncMutex<HDAccountsMap<HDAccount>>;
pub type HDAccountsMut<'a, HDAccount> = AsyncMutexGuard<'a, HDAccountsMap<HDAccount>>;
pub type HDAccountMut<'a, HDAccount> = AsyncMappedMutexGuard<'a, HDAccountsMap<HDAccount>, HDAccount>;
pub type HDAddressIds = BTreeSet<HDAddressId>;

#[derive(Display)]
pub enum AddressDerivingError {
    #[display(fmt = "BIP32 address deriving error: {}", _0)]
    Bip32Error(Bip32Error),
}

impl From<Bip32Error> for AddressDerivingError {
    fn from(e: Bip32Error) -> Self { AddressDerivingError::Bip32Error(e) }
}

impl From<AddressDerivingError> for BalanceError {
    fn from(e: AddressDerivingError) -> Self {
        match e {
            AddressDerivingError::Bip32Error(bip32) => BalanceError::Internal(bip32.to_string()),
        }
    }
}

impl From<AddressDerivingError> for WithdrawError {
    fn from(e: AddressDerivingError) -> Self { WithdrawError::UnexpectedFromAddress(e.to_string()) }
}

#[derive(Display)]
pub enum InvalidDerivationPath {
    #[display(fmt = "Expected '{}' coin_type, found '{}'", expected, found)]
    UnexpectedCoinType { expected: u32, found: u32 },
    #[display(fmt = "Expected '{}' account_id, found '{}'", expected, found)]
    UnexpectedAccountId { expected: u32, found: u32 },
    #[display(fmt = "{}", _0)]
    InvalidBip44Chain(InvalidBip44ChainError),
}

#[derive(Display)]
pub enum NewAccountCreatingError {
    #[display(fmt = "Hardware Wallet context is not initialized")]
    HwContextNotInitialized,
    #[display(fmt = "HD wallet is unavailable")]
    HDWalletUnavailable,
    #[display(
        fmt = "Coin doesn't support Trezor hardware wallet. Please consider adding the 'trezor_coin' field to the coins config"
    )]
    CoinDoesntSupportTrezor,
    RpcTaskError(RpcTaskError),
    HardwareWalletError(HwError),
    #[display(fmt = "Accounts limit reached. Max number of accounts: {}", max_accounts_number)]
    AccountLimitReached {
        max_accounts_number: u32,
    },
    #[display(fmt = "Error saving HD account to storage: {}", _0)]
    ErrorSavingAccountToStorage(String),
    #[display(fmt = "Internal error: {}", _0)]
    Internal(String),
}

impl From<Bip32DerPathError> for NewAccountCreatingError {
    fn from(e: Bip32DerPathError) -> Self { NewAccountCreatingError::Internal(Bip44DerPathError::from(e).to_string()) }
}

impl From<HDWalletStorageError> for NewAccountCreatingError {
    fn from(e: HDWalletStorageError) -> Self {
        match e {
            HDWalletStorageError::ErrorSaving(e) | HDWalletStorageError::ErrorSerializing(e) => {
                NewAccountCreatingError::ErrorSavingAccountToStorage(e)
            },
            HDWalletStorageError::HDWalletUnavailable => NewAccountCreatingError::HDWalletUnavailable,
            HDWalletStorageError::Internal(internal) => NewAccountCreatingError::Internal(internal),
            other => NewAccountCreatingError::Internal(other.to_string()),
        }
    }
}

impl From<NewAccountCreatingError> for HDWalletRpcError {
    fn from(e: NewAccountCreatingError) -> Self {
        match e {
            NewAccountCreatingError::HwContextNotInitialized => HDWalletRpcError::HwContextNotInitialized,
            NewAccountCreatingError::HDWalletUnavailable => HDWalletRpcError::CoinIsActivatedNotWithHDWallet,
            NewAccountCreatingError::CoinDoesntSupportTrezor => HDWalletRpcError::CoinDoesntSupportTrezor,
            NewAccountCreatingError::RpcTaskError(rpc) => HDWalletRpcError::from(rpc),
            NewAccountCreatingError::HardwareWalletError(hw) => HDWalletRpcError::from(hw),
            NewAccountCreatingError::AccountLimitReached { max_accounts_number } => {
                HDWalletRpcError::AccountLimitReached { max_accounts_number }
            },
            NewAccountCreatingError::ErrorSavingAccountToStorage(e) => {
                let error = format!("Error uploading HD account info to the storage: {}", e);
                HDWalletRpcError::WalletStorageError(error)
            },
            NewAccountCreatingError::Internal(internal) => HDWalletRpcError::Internal(internal),
        }
    }
}

/// Currently, we suppose that ETH/ERC20/QRC20 don't have [`Bip44Chain::Internal`] addresses.
#[derive(Display)]
#[display(fmt = "Coin doesn't support the given BIP44 chain: {:?}", chain)]
pub struct InvalidBip44ChainError {
    pub chain: Bip44Chain,
}

#[derive(Display)]
pub enum AccountUpdatingError {
    WalletStorageError(HDWalletStorageError),
}

impl From<HDWalletStorageError> for AccountUpdatingError {
    fn from(e: HDWalletStorageError) -> Self { AccountUpdatingError::WalletStorageError(e) }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize, SerializeErrorType)]
#[serde(tag = "error_type", content = "error_data")]
pub enum HDWalletRpcError {
    /* ----------- Trezor device errors ----------- */
    #[display(fmt = "Trezor device disconnected")]
    TrezorDisconnected,
    #[display(fmt = "Trezor internal error: {}", _0)]
    HardwareWalletInternal(String),
    #[display(fmt = "No Trezor device available")]
    NoTrezorDeviceAvailable,
    #[display(fmt = "Unexpected Hardware Wallet device: {}", _0)]
    FoundUnexpectedDevice(String),
    #[display(
        fmt = "Coin doesn't support Trezor hardware wallet. Please consider adding the 'trezor_coin' field to the coins config"
    )]
    CoinDoesntSupportTrezor,
    /* ----------- HD Wallet RPC error ------------ */
    #[display(fmt = "Hardware Wallet context is not initialized")]
    HwContextNotInitialized,
    #[display(fmt = "No such coin {}", coin)]
    NoSuchCoin { coin: String },
    #[display(fmt = "RPC timed out {:?}", _0)]
    Timeout(Duration),
    #[display(fmt = "Coin is expected to be activated with the HD wallet derivation method")]
    CoinIsActivatedNotWithHDWallet,
    #[display(fmt = "HD account '{}' is not activated", account_id)]
    UnknownAccount { account_id: u32 },
    #[display(fmt = "Coin doesn't support the given BIP44 chain: {:?}", chain)]
    InvalidBip44Chain { chain: Bip44Chain },
    #[display(fmt = "Error deriving an address: {}", _0)]
    ErrorDerivingAddress(String),
    #[display(fmt = "Accounts limit reached. Max number of accounts: {}", max_accounts_number)]
    AccountLimitReached { max_accounts_number: u32 },
    #[display(fmt = "Electrum/Native RPC invalid response: {}", _0)]
    RpcInvalidResponse(String),
    #[display(fmt = "HD wallet storage error: {}", _0)]
    WalletStorageError(String),
    #[display(fmt = "Transport: {}", _0)]
    Transport(String),
    #[display(fmt = "Internal: {}", _0)]
    Internal(String),
}

impl From<CoinFindError> for HDWalletRpcError {
    fn from(e: CoinFindError) -> Self {
        match e {
            CoinFindError::NoSuchCoin { coin } => HDWalletRpcError::NoSuchCoin { coin },
        }
    }
}

impl From<UnexpectedDerivationMethod> for HDWalletRpcError {
    fn from(e: UnexpectedDerivationMethod) -> Self {
        match e {
            UnexpectedDerivationMethod::HDWalletUnavailable => HDWalletRpcError::CoinIsActivatedNotWithHDWallet,
            unexpected_error => HDWalletRpcError::Internal(unexpected_error.to_string()),
        }
    }
}

impl From<BalanceError> for HDWalletRpcError {
    fn from(e: BalanceError) -> Self {
        match e {
            BalanceError::Transport(transport) => HDWalletRpcError::Transport(transport),
            BalanceError::InvalidResponse(rpc) => HDWalletRpcError::RpcInvalidResponse(rpc),
            BalanceError::UnexpectedDerivationMethod(der_path) => HDWalletRpcError::from(der_path),
            BalanceError::WalletStorageError(storage) => HDWalletRpcError::WalletStorageError(storage),
            BalanceError::Internal(internal) => HDWalletRpcError::Internal(internal),
        }
    }
}

impl From<InvalidBip44ChainError> for HDWalletRpcError {
    fn from(e: InvalidBip44ChainError) -> Self { HDWalletRpcError::InvalidBip44Chain { chain: e.chain } }
}

impl From<InvalidDerivationPath> for HDWalletRpcError {
    fn from(e: InvalidDerivationPath) -> Self {
        match e {
            InvalidDerivationPath::UnexpectedCoinType { .. } | InvalidDerivationPath::UnexpectedAccountId { .. } => {
                HDWalletRpcError::ErrorDerivingAddress(e.to_string())
            },
            InvalidDerivationPath::InvalidBip44Chain(chain_err) => {
                HDWalletRpcError::InvalidBip44Chain { chain: chain_err.chain }
            },
        }
    }
}

impl From<AddressDerivingError> for HDWalletRpcError {
    fn from(e: AddressDerivingError) -> Self {
        match e {
            AddressDerivingError::Bip32Error(bip32) => HDWalletRpcError::ErrorDerivingAddress(bip32.to_string()),
        }
    }
}

impl From<AccountUpdatingError> for HDWalletRpcError {
    fn from(e: AccountUpdatingError) -> Self {
        match e {
            AccountUpdatingError::WalletStorageError(storage) => {
                HDWalletRpcError::WalletStorageError(storage.to_string())
            },
        }
    }
}

impl From<RpcTaskError> for HDWalletRpcError {
    fn from(e: RpcTaskError) -> Self {
        let error = e.to_string();
        match e {
            RpcTaskError::Canceled => HDWalletRpcError::Internal("Canceled".to_owned()),
            RpcTaskError::Timeout(timeout) => HDWalletRpcError::Timeout(timeout),
            RpcTaskError::NoSuchTask(_) | RpcTaskError::UnexpectedTaskStatus { .. } => {
                HDWalletRpcError::Internal(error)
            },
            RpcTaskError::Internal(internal) => HDWalletRpcError::Internal(internal),
        }
    }
}

impl From<HwError> for HDWalletRpcError {
    fn from(e: HwError) -> Self {
        let error = e.to_string();
        match e {
            HwError::NoTrezorDeviceAvailable => HDWalletRpcError::NoTrezorDeviceAvailable,
            HwError::FoundUnexpectedDevice { .. } => HDWalletRpcError::FoundUnexpectedDevice(error),
            _ => HDWalletRpcError::HardwareWalletInternal(error),
        }
    }
}

impl HttpStatusCode for HDWalletRpcError {
    fn status_code(&self) -> StatusCode {
        match self {
            HDWalletRpcError::CoinDoesntSupportTrezor
            | HDWalletRpcError::HwContextNotInitialized
            | HDWalletRpcError::NoSuchCoin { .. }
            | HDWalletRpcError::CoinIsActivatedNotWithHDWallet
            | HDWalletRpcError::UnknownAccount { .. }
            | HDWalletRpcError::InvalidBip44Chain { .. }
            | HDWalletRpcError::ErrorDerivingAddress(_)
            | HDWalletRpcError::AccountLimitReached { .. } => StatusCode::BAD_REQUEST,
            HDWalletRpcError::TrezorDisconnected
            | HDWalletRpcError::HardwareWalletInternal(_)
            | HDWalletRpcError::NoTrezorDeviceAvailable
            | HDWalletRpcError::FoundUnexpectedDevice(_)
            | HDWalletRpcError::Timeout(_)
            | HDWalletRpcError::Transport(_)
            | HDWalletRpcError::RpcInvalidResponse(_)
            | HDWalletRpcError::WalletStorageError(_)
            | HDWalletRpcError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub struct HDAddress<Address, Pubkey> {
    pub address: Address,
    pub pubkey: Pubkey,
    pub derivation_path: DerivationPath,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct HDAddressId {
    pub chain: Bip44Chain,
    pub address_id: u32,
}

impl HDAddressId {
    /// Tries to convert `der_path` into `HDAddressId` by validating its `coin_type`, `chain` and `account_id`.
    pub fn try_from_derivation_path(
        hd_wallet: &impl HDWalletOps,
        hd_account: &impl HDAccountOps,
        der_path: &Bip44DerivationPath,
    ) -> MmResult<HDAddressId, InvalidDerivationPath> {
        let HDAccountAddressId { account_id, chain, .. } =
            HDAccountAddressId::try_from_derivation_path(hd_wallet, der_path)?;
        if hd_account.account_id() != account_id {
            return MmError::err(InvalidDerivationPath::UnexpectedAccountId {
                expected: hd_account.account_id(),
                found: account_id,
            });
        }

        Ok(HDAddressId {
            chain,
            address_id: der_path.address_id(),
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct HDAccountAddressId {
    pub account_id: u32,
    pub chain: Bip44Chain,
    pub address_id: u32,
}

impl HDAccountAddressId {
    /// Tries to convert `der_path` into `HDAddressId` by validating its `coin_type` and `chain`.
    pub fn try_from_derivation_path(
        hd_wallet: &impl HDWalletOps,
        der_path: &Bip44DerivationPath,
    ) -> MmResult<HDAccountAddressId, InvalidDerivationPath> {
        if hd_wallet.coin_type() != der_path.coin_type() {
            return MmError::err(InvalidDerivationPath::UnexpectedCoinType {
                expected: hd_wallet.coin_type(),
                found: der_path.coin_type(),
            });
        }

        let chain = der_path.chain();
        if !hd_wallet.supports_chain(&chain) {
            return MmError::err(InvalidDerivationPath::InvalidBip44Chain(InvalidBip44ChainError {
                chain,
            }));
        }

        Ok(HDAccountAddressId {
            account_id: der_path.account_id(),
            chain,
            address_id: der_path.address_id(),
        })
    }

    pub fn to_address_id(&self) -> HDAddressId {
        HDAddressId {
            chain: self.chain,
            address_id: self.address_id,
        }
    }
}

#[async_trait]
pub trait HDWalletCoinOps {
    type Address: Send + Sync;
    type Pubkey: Send;
    type HDWallet: HDWalletOps<HDAccount = Self::HDAccount>;
    type HDAccount: HDAccountOps;

    /// Derives an address from the given info.
    fn derive_address(
        &self,
        hd_account: &Self::HDAccount,
        address_id: HDAddressId,
    ) -> MmResult<HDAddress<Self::Address, Self::Pubkey>, AddressDerivingError>;

    /// Creates a new HD account, registers it within the given `hd_wallet`
    /// and returns a mutable reference to the registered account.
    async fn create_new_account<'a, XPubExtractor>(
        &self,
        hd_wallet: &'a Self::HDWallet,
        xpub_extractor: &XPubExtractor,
    ) -> MmResult<HDAccountMut<'a, Self::HDAccount>, NewAccountCreatingError>
    where
        XPubExtractor: HDXPubExtractor + Sync;

    async fn enable_addresses(
        &self,
        hd_wallet: &Self::HDWallet,
        hd_account: &mut Self::HDAccount,
        addresses: HDAddressIds,
    ) -> MmResult<(), AccountUpdatingError>;
}

#[async_trait]
pub trait HDWalletOps: Send + Sync {
    type HDAccount: HDAccountOps + Clone + Send;

    fn coin_type(&self) -> u32;

    fn supports_chain(&self, chain: &Bip44Chain) -> bool;

    fn get_accounts_mutex(&self) -> &HDAccountsMutex<Self::HDAccount>;

    /// Returns a copy of an account by the given `account_id` if it's activated.
    async fn get_account(&self, account_id: u32) -> Option<Self::HDAccount> {
        let accounts = self.get_accounts_mutex().lock().await;
        accounts.get(&account_id).cloned()
    }

    /// Returns a mutable reference to an account by the given `account_id` if it's activated.
    async fn get_account_mut(&self, account_id: u32) -> Option<HDAccountMut<'_, Self::HDAccount>> {
        let accounts = self.get_accounts_mutex().lock().await;
        if !accounts.contains_key(&account_id) {
            return None;
        }

        Some(AsyncMutexGuard::map(accounts, |accounts| {
            accounts
                .get_mut(&account_id)
                .expect("getting an element should never fail due to the checks above")
        }))
    }

    /// Returns copies of all activated accounts.
    async fn get_accounts(&self) -> HDAccountsMap<Self::HDAccount> { self.get_accounts_mutex().lock().await.clone() }

    /// Returns a mutable reference to all activated accounts.
    async fn get_accounts_mut(&self) -> HDAccountsMut<'_, Self::HDAccount> { self.get_accounts_mutex().lock().await }
}

pub trait HDAccountOps: Send + Sync {
    /// Returns a derivation path of this account.
    fn account_derivation_path(&self) -> DerivationPath;

    /// Returns an index of this account.
    fn account_id(&self) -> u32;

    /// Returns the enabled HD addresses.
    fn enabled_addresses(&self) -> &HDAddressIds;

    /// Returns true if the given address is enabled.
    fn is_address_enabled(&self, address_id: &HDAddressId) -> bool { self.enabled_addresses().contains(address_id) }
}
