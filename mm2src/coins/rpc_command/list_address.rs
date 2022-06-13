use crate::coin_balance::HDAddressBalance;
use crate::hd_wallet::HDWalletCoinOps;
use crate::rpc_command::account_balance_rpc_error::HDAccountBalanceRpcError;
use crate::{lp_coinfind_or_err, CoinWithDerivationMethod, MmCoinEnum};
use async_trait::async_trait;
use common::PagingOptionsEnum;
use crypto::{Bip44Chain, RpcDerivationPath};
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::*;
use std::fmt;

#[derive(Deserialize)]
pub struct ListAddressRequest {
    coin: String,
    #[serde(flatten)]
    params: ListAddressParams,
}

#[derive(Deserialize)]
pub struct ListAddressParams {
    pub account_index: u32,
    pub chain: Bip44Chain,
    #[serde(default = "common::ten")]
    pub limit: usize,
    #[serde(default)]
    pub paging_options: PagingOptionsEnum<u32>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ListAddressResponse {
    pub account_index: u32,
    pub derivation_path: RpcDerivationPath,
    pub addresses: Vec<HDAddressBalance>,
    pub limit: usize,
    pub skipped: u32,
    pub paging_options: PagingOptionsEnum<u32>,
}

#[async_trait]
pub trait ListAddressRpcOps {
    async fn list_address_rpc(
        &self,
        params: ListAddressParams,
    ) -> MmResult<ListAddressResponse, HDAccountBalanceRpcError>;
}

pub async fn list_address(
    ctx: MmArc,
    req: ListAddressRequest,
) -> MmResult<ListAddressResponse, HDAccountBalanceRpcError> {
    match lp_coinfind_or_err(&ctx, &req.coin).await? {
        MmCoinEnum::UtxoCoin(utxo) => utxo.list_address_rpc(req.params).await,
        MmCoinEnum::QtumCoin(qtum) => qtum.list_address_rpc(req.params).await,
        _ => MmError::err(HDAccountBalanceRpcError::CoinIsActivatedNotWithHDWallet),
    }
}

pub mod common_impl {
    use super::*;
    use crate::coin_balance::HDWalletBalanceOps;
    use crate::hd_wallet::{HDAccountOps, HDAddressId, HDWalletOps};

    pub async fn account_balance_rpc<Coin>(
        coin: &Coin,
        params: ListAddressParams,
    ) -> MmResult<ListAddressResponse, HDAccountBalanceRpcError>
    where
        Coin: HDWalletBalanceOps + CoinWithDerivationMethod<HDWallet = <Coin as HDWalletCoinOps>::HDWallet> + Sync,
        <Coin as HDWalletCoinOps>::Address: fmt::Display + Clone,
    {
        let account_id = params.account_index;
        let hd_account = coin
            .derivation_method()
            .hd_wallet_or_err()?
            .get_account(account_id)
            .await
            .or_mm_err(|| HDAccountBalanceRpcError::UnknownAccount { account_id })?;

        let from_address_id = match params.paging_options {
            PagingOptionsEnum::FromId(from_address_id) => from_address_id + 1,
            PagingOptionsEnum::PageNumber(page_number) => ((page_number.get() - 1) * params.limit) as u32,
        };
        let to_address_id = from_address_id + params.limit as u32;

        let address_ids = (from_address_id..to_address_id)
            .into_iter()
            .map(|address_id| HDAddressId {
                chain: params.chain,
                address_id,
            });
        let addresses = coin.addresses_balances_with_ids(&hd_account, address_ids).await?;

        let result = ListAddressResponse {
            account_index: account_id,
            derivation_path: RpcDerivationPath(hd_account.account_derivation_path()),
            addresses,
            limit: params.limit,
            skipped: from_address_id,
            paging_options: params.paging_options,
        };

        Ok(result)
    }
}
