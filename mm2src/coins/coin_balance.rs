use crate::hd_pubkey::HDXPubExtractor;
use crate::hd_wallet::{HDAccountOps, HDAddressId, HDWalletCoinOps, HDWalletRpcError, NewAccountCreatingError};
use crate::{BalanceError, BalanceResult, CoinBalance, CoinWithDerivationMethod, DerivationMethod, HDAddress,
            MarketCoinOps};
use async_trait::async_trait;
use common::custom_iter::TryUnzip;
use crypto::RpcDerivationPath;
use derive_more::Display;
use futures::compat::Future01CompatExt;
use mm2_err_handle::prelude::*;
use std::fmt;
use std::ops::Range;

pub type AddressIdRange = Range<u32>;

#[derive(Display)]
pub enum EnableCoinBalanceError {
    NewAccountCreatingError(NewAccountCreatingError),
    BalanceError(BalanceError),
}

impl From<NewAccountCreatingError> for EnableCoinBalanceError {
    fn from(e: NewAccountCreatingError) -> Self { EnableCoinBalanceError::NewAccountCreatingError(e) }
}

impl From<BalanceError> for EnableCoinBalanceError {
    fn from(e: BalanceError) -> Self { EnableCoinBalanceError::BalanceError(e) }
}

impl From<EnableAccountError> for EnableCoinBalanceError {
    fn from(e: EnableAccountError) -> Self {
        match e {
            EnableAccountError::BalanceError(balance) => EnableCoinBalanceError::from(balance),
        }
    }
}

#[derive(Display)]
pub enum EnableAccountError {
    BalanceError(BalanceError),
}

impl From<BalanceError> for EnableAccountError {
    fn from(e: BalanceError) -> Self { EnableAccountError::BalanceError(e) }
}

impl From<EnableAccountError> for HDWalletRpcError {
    fn from(e: EnableAccountError) -> Self {
        match e {
            EnableAccountError::BalanceError(balance) => HDWalletRpcError::from(balance),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "wallet_type")]
pub enum EnableCoinBalance {
    Iguana(IguanaWalletBalance),
    HD(HDWalletBalance),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IguanaWalletBalance {
    pub address: String,
    pub balance: CoinBalance,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HDWalletBalance {
    pub accounts: Vec<HDAccountBalance>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HDAccountBalance {
    pub account_index: u32,
    pub derivation_path: RpcDerivationPath,
    pub total_balance: CoinBalance,
    pub addresses: Vec<HDAddressBalance>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HDAddressBalance {
    pub address: String,
    pub derivation_path: RpcDerivationPath,
    pub balance: CoinBalance,
}

#[async_trait]
pub trait EnableCoinBalanceOps {
    async fn enable_coin_balance<XPubExtractor>(
        &self,
        xpub_extractor: &XPubExtractor,
    ) -> MmResult<EnableCoinBalance, EnableCoinBalanceError>
    where
        XPubExtractor: HDXPubExtractor + Sync;
}

#[async_trait]
impl<Coin> EnableCoinBalanceOps for Coin
where
    Coin: CoinWithDerivationMethod<HDWallet = <Coin as HDWalletCoinOps>::HDWallet>
        + HDWalletBalanceOps
        + MarketCoinOps
        + Sync,
    <Coin as CoinWithDerivationMethod>::Address: fmt::Display + Sync,
{
    async fn enable_coin_balance<XPubExtractor>(
        &self,
        xpub_extractor: &XPubExtractor,
    ) -> MmResult<EnableCoinBalance, EnableCoinBalanceError>
    where
        XPubExtractor: HDXPubExtractor + Sync,
    {
        match self.derivation_method() {
            DerivationMethod::Iguana(my_address) => self
                .my_balance()
                .compat()
                .await
                .map(|balance| {
                    EnableCoinBalance::Iguana(IguanaWalletBalance {
                        address: my_address.to_string(),
                        balance,
                    })
                })
                .mm_err(EnableCoinBalanceError::from),
            DerivationMethod::HDWallet(hd_wallet) => self
                .enable_hd_wallet(hd_wallet, xpub_extractor)
                .await
                .map(EnableCoinBalance::HD),
        }
    }
}

#[async_trait]
pub trait HDWalletBalanceOps: HDWalletCoinOps {
    async fn enable_hd_account(&self, hd_account: &Self::HDAccount) -> MmResult<HDAccountBalance, EnableAccountError>;

    /// Requests balances of already known addresses, and if it's prescribed by [`EnableCoinParams::scan_policy`],
    /// scans for new addresses of every HD account by using [`HDWalletBalanceOps::scan_for_new_addresses`].
    /// This method is used on coin initialization to index working addresses and to return the wallet balance to the user.
    async fn enable_hd_wallet<XPubExtractor>(
        &self,
        hd_wallet: &Self::HDWallet,
        xpub_extractor: &XPubExtractor,
    ) -> MmResult<HDWalletBalance, EnableCoinBalanceError>
    where
        XPubExtractor: HDXPubExtractor + Sync;

    /// Requests balances of every known addresses of the given `hd_account`.
    async fn account_balance(&self, hd_account: &Self::HDAccount) -> BalanceResult<Vec<HDAddressBalance>>
    where
        Self::Address: fmt::Display + Clone,
    {
        self.addresses_balances_with_ids(hd_account, hd_account.enabled_addresses().iter().copied())
            .await
    }

    /// Requests balances of known addresses of the given `address_ids` addresses at the specified `chain`.
    /// The `HDAddressBalance` items are guaranteed to be in the same order in which they were requested.
    async fn addresses_balances_with_ids<Ids>(
        &self,
        hd_account: &Self::HDAccount,
        address_ids: Ids,
    ) -> BalanceResult<Vec<HDAddressBalance>>
    where
        Self::Address: fmt::Display + Clone,
        Ids: IntoIterator<Item = HDAddressId> + Send,
    {
        let (addresses, der_paths) = address_ids
            .into_iter()
            .map(|address_id| -> BalanceResult<_> {
                let HDAddress {
                    address,
                    derivation_path,
                    ..
                } = self.derive_address(hd_account, address_id)?;
                Ok((address, derivation_path))
            })
            // Try to unzip `Result<(Address, DerivationPath)>` elements into `Result<(Vec<Address>, Vec<DerivationPath>)>`.
            .try_unzip::<Vec<_>, Vec<_>>()?;

        let balances = self
            .addresses_balances(addresses)
            .await?
            .into_iter()
            // [`HDWalletBalanceOps::known_addresses_balances`] returns pairs `(Address, CoinBalance)`
            // that are guaranteed to be in the same order in which they were requested.
            // So we can zip the derivation paths with the pairs `(Address, CoinBalance)`.
            .zip(der_paths)
            .map(|((address, balance), derivation_path)| HDAddressBalance {
                address: address.to_string(),
                derivation_path: RpcDerivationPath(derivation_path),
                balance,
            })
            .collect();
        Ok(balances)
    }

    /// Requests balance of the given `address`.
    /// This function is expected to be more efficient than ['HDWalletBalanceOps::is_address_used'] in most cases
    /// since many of RPC clients allow us to request the address balance without the history.
    /// TODO consider removing it
    async fn address_balance(&self, address: &Self::Address) -> BalanceResult<CoinBalance>;

    /// Requests balances of the given `addresses`.
    /// The pairs `(Address, CoinBalance)` are guaranteed to be in the same order in which they were requested.
    async fn addresses_balances(
        &self,
        addresses: Vec<Self::Address>,
    ) -> BalanceResult<Vec<(Self::Address, CoinBalance)>>;
}

pub mod common_impl {
    use super::*;
    use crate::hd_wallet::{HDAccountOps, HDWalletOps};
    use common::log::{debug, info};

    pub(crate) async fn enable_hd_account<Coin>(
        coin: &Coin,
        hd_account: &Coin::HDAccount,
    ) -> MmResult<HDAccountBalance, EnableAccountError>
    where
        Coin: HDWalletBalanceOps + Sync,
        Coin::Address: fmt::Display + Clone,
    {
        let addresses = coin.account_balance(hd_account).await?;
        let total_balance = addresses.iter().fold(CoinBalance::default(), |total, addr_balance| {
            total + addr_balance.balance.clone()
        });
        Ok(HDAccountBalance {
            account_index: hd_account.account_id(),
            derivation_path: RpcDerivationPath(hd_account.account_derivation_path()),
            total_balance,
            addresses,
        })
    }

    pub(crate) async fn enable_hd_wallet<Coin, XPubExtractor>(
        coin: &Coin,
        hd_wallet: &Coin::HDWallet,
        xpub_extractor: &XPubExtractor,
    ) -> MmResult<HDWalletBalance, EnableCoinBalanceError>
    where
        Coin: HDWalletBalanceOps + MarketCoinOps + Sync,
        XPubExtractor: HDXPubExtractor + Sync,
    {
        let accounts = hd_wallet.get_accounts_mut().await;
        let mut result = HDWalletBalance {
            accounts: Vec::with_capacity(accounts.len() + 1),
        };

        if accounts.is_empty() {
            // Is seems that we couldn't find any HD account in the HD wallet storage.
            drop(accounts);
            info!(
                "{} HD wallet hasn't been enabled before. Create default HD account",
                coin.ticker()
            );

            // Create new HD account.
            let new_account = coin.create_new_account(hd_wallet, xpub_extractor).await?;
            result.accounts.push(coin.enable_hd_account(&new_account).await?);
            return Ok(result);
        }

        debug!(
            "{} HD accounts were found on {} coin activation",
            accounts.len(),
            coin.ticker()
        );
        for (_account_id, hd_account) in accounts.iter() {
            result.accounts.push(coin.enable_hd_account(hd_account).await?);
        }

        Ok(result)
    }
}
