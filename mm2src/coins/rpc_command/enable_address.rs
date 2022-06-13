use crate::hd_wallet::{HDAddressId, HDAddressIds, HDWalletCoinOps, HDWalletRpcError};
use crate::{lp_coinfind_or_err, CoinBalance, CoinWithDerivationMethod, MmCoinEnum};
use async_trait::async_trait;
use common::collections::UniqueVec;
use crypto::{Bip44DerivationPath, RpcDerivationPath};
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::*;
use std::fmt;

#[derive(Deserialize)]
pub struct EnableAddressRequest {
    coin: String,
    #[serde(flatten)]
    params: EnableAddressParams,
}

#[derive(Deserialize)]
pub struct EnableAddressParams {
    pub account_index: u32,
    pub addresses: EnableAddressEnum,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum EnableAddressEnum {
    AddressIds(UniqueVec<HDAddressId>),
    DerivationPaths(UniqueVec<Bip44DerivationPath>),
}

#[derive(Debug, PartialEq, Serialize)]
pub struct EnableAddressResponse {
    pub account_index: u32,
    pub derivation_path: RpcDerivationPath,
    /// The [`EnableAddressResponse::addresses`] items are in the same order
    /// as they were specified in the [`EnableAddressRequest::addresses`] request.
    pub addresses: Vec<EnableAddressResponseItem>,
}

#[derive(Debug, PartialEq, Serialize)]
pub enum EnableHDAddressStatus {
    Success,
    EnabledAlready,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct EnableAddressResponseItem {
    status: EnableHDAddressStatus,
    address: String,
    derivation_path: RpcDerivationPath,
    balance: CoinBalance,
}

#[async_trait]
pub trait EnableAddressRpcOps {
    async fn enable_address_rpc(
        &self,
        params: EnableAddressParams,
    ) -> MmResult<EnableAddressResponse, HDWalletRpcError>;
}

pub async fn enable_address(
    ctx: MmArc,
    req: EnableAddressRequest,
) -> MmResult<EnableAddressResponse, HDWalletRpcError> {
    match lp_coinfind_or_err(&ctx, &req.coin).await? {
        MmCoinEnum::UtxoCoin(utxo) => utxo.enable_address_rpc(req.params).await,
        MmCoinEnum::QtumCoin(qtum) => qtum.enable_address_rpc(req.params).await,
        _ => MmError::err(HDWalletRpcError::CoinIsActivatedNotWithHDWallet),
    }
}

pub mod common_impl {
    use super::*;
    use crate::coin_balance::HDWalletBalanceOps;
    use crate::hd_wallet::{HDAccountOps, HDWalletOps};
    use crate::HDAddress;
    use std::ops::Deref;

    pub async fn enable_address_rpc<Coin>(
        coin: &Coin,
        params: EnableAddressParams,
    ) -> MmResult<EnableAddressResponse, HDWalletRpcError>
    where
        Coin: HDWalletBalanceOps + CoinWithDerivationMethod<HDWallet = <Coin as HDWalletCoinOps>::HDWallet> + Sync,
        <Coin as HDWalletCoinOps>::Address: fmt::Display + Clone,
    {
        let account_id = params.account_index;
        let hd_wallet = coin.derivation_method().hd_wallet_or_err()?;
        let mut hd_account = hd_wallet
            .get_account_mut(account_id)
            .await
            .or_mm_err(|| HDWalletRpcError::UnknownAccount { account_id })?;

        // `address_ids` is guaranteed to consist of unique `HDAddressId` items
        // For more info see `common::collections::UniqueVec`.
        let address_ids = match params.addresses {
            EnableAddressEnum::AddressIds(address_ids) => address_ids.into(),
            EnableAddressEnum::DerivationPaths(der_paths) => der_paths
                .into_iter()
                // Convert `Bip44DerivationPath` into `MmResult<HDAddressId, InvalidDerivationPath>`
                .map(|der_path| HDAddressId::try_from_derivation_path(hd_wallet, hd_account.deref(), &der_path))
                .collect::<Result<Vec<_>, _>>()?,
        };

        // Get already enabled addresses.
        let enabled_addresses = hd_account.enabled_addresses();

        let mut result = EnableAddressResponse {
            account_index: account_id,
            derivation_path: RpcDerivationPath(hd_account.account_derivation_path()),
            addresses: Vec::with_capacity(address_ids.len()),
        };

        let mut to_enable = HDAddressIds::new();

        for address_id in address_ids.iter() {
            let HDAddress {
                address,
                derivation_path,
                ..
            } = coin.derive_address(&hd_account, *address_id)?;

            let status = if enabled_addresses.contains(address_id) {
                to_enable.insert(*address_id);
                EnableHDAddressStatus::EnabledAlready
            } else {
                EnableHDAddressStatus::Success
            };

            // Push the address result into `result` container with a default balance.
            // The balance will be requested below.
            // Please note [`EnableAddressResponse::addresses`] must be in the same order
            // as they were specified in the [`EnableAddressRequest::addresses`] request.
            result.addresses.push(EnableAddressResponseItem {
                address: address.to_string(),
                derivation_path: derivation_path.into(),
                status,
                balance: CoinBalance::default(),
            });
        }

        coin.enable_addresses(hd_wallet, &mut hd_account, to_enable).await?;

        coin.addresses_balances_with_ids(&hd_account, address_ids)
            .await?
            .into_iter()
            // The `HDAddressBalance` items are guaranteed to be in the same order in which they were requested,
            // so we can zip them with `&mut EnableAddressResponseItem`.
            .zip(result.addresses.iter_mut())
            .for_each(|(addr_balance, address_res)| address_res.balance = addr_balance.balance);
        Ok(result)
    }
}
