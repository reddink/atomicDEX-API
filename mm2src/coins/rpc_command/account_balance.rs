use crate::coin_balance::HDAddressBalance;
use crate::hd_wallet::HDWalletCoinOps;
use crate::rpc_command::account_balance_rpc_error::HDAccountBalanceRpcError;
use crate::{lp_coinfind_or_err, CoinBalance, CoinWithDerivationMethod, MmCoinEnum};
use async_trait::async_trait;
use crypto::{ RpcDerivationPath};
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::*;
use std::fmt;

#[derive(Deserialize)]
pub struct AccountBalanceRequest {
    coin: String,
    #[serde(flatten)]
    params: AccountBalanceParams,
}

#[derive(Deserialize)]
pub struct AccountBalanceParams {
    pub account_index: u32,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AccountBalanceResponse {
    pub account_index: u32,
    pub derivation_path: RpcDerivationPath,
    pub addresses: Vec<HDAddressBalance>,
    pub total_balance: CoinBalance,
}

#[async_trait]
pub trait AccountBalanceRpcOps {
    async fn account_balance_rpc(
        &self,
        params: AccountBalanceParams,
    ) -> MmResult<AccountBalanceResponse, HDAccountBalanceRpcError>;
}

pub async fn account_balance(
    ctx: MmArc,
    req: AccountBalanceRequest,
) -> MmResult<AccountBalanceResponse, HDAccountBalanceRpcError> {
    match lp_coinfind_or_err(&ctx, &req.coin).await? {
        MmCoinEnum::UtxoCoin(utxo) => utxo.account_balance_rpc(req.params).await,
        MmCoinEnum::QtumCoin(qtum) => qtum.account_balance_rpc(req.params).await,
        _ => MmError::err(HDAccountBalanceRpcError::CoinIsActivatedNotWithHDWallet),
    }
}

pub mod common_impl {
    use super::*;
    use crate::coin_balance::HDWalletBalanceOps;
    use crate::hd_wallet::{HDAccountOps, HDWalletOps};

    pub async fn account_balance_rpc<Coin>(
        coin: &Coin,
        params: AccountBalanceParams,
    ) -> MmResult<AccountBalanceResponse, HDAccountBalanceRpcError>
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

        let addresses = coin.account_balance(&hd_account).await?;
        let total_balance = addresses.iter().fold(CoinBalance::default(), |total, addr_balance| {
            total + addr_balance.balance.clone()
        });

        let result = AccountBalanceResponse {
            account_index: account_id,
            derivation_path: RpcDerivationPath(hd_account.account_derivation_path()),
            addresses,
            total_balance,
        };
        Ok(result)
    }
}
