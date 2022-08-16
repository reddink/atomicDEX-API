use crate::{lp_coinfind_or_err, CoinsContext, MmCoinEnum, WithdrawError, ZRpcOps};
use crate::{TransactionDetails, WithdrawRequest};
use async_trait::async_trait;
use common::SuccessResponse;
use crypto::hw_rpc_task::{HwRpcTaskAwaitingStatus, HwRpcTaskUserAction, HwRpcTaskUserActionRequest};
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::*;
use rpc_task::rpc_common::{InitRpcTaskResponse, RpcTaskStatusError, RpcTaskStatusRequest, RpcTaskUserActionError};
use rpc_task::{RpcTask, RpcTaskHandle, RpcTaskManager, RpcTaskManagerShared, RpcTaskStatusAlias, RpcTaskTypes};

pub type WithdrawAwaitingStatus = HwRpcTaskAwaitingStatus;
pub type WithdrawUserAction = HwRpcTaskUserAction;
pub type WithdrawStatusError = RpcTaskStatusError;
pub type WithdrawUserActionError = RpcTaskUserActionError;
pub type InitWithdrawResponse = InitRpcTaskResponse;
pub type WithdrawStatusRequest = RpcTaskStatusRequest;
pub type WithdrawUserActionRequest = HwRpcTaskUserActionRequest;
pub type WithdrawTaskManager<T> = RpcTaskManager<WithdrawTask<T>, T>;
pub type WithdrawTaskManagerShared<T> = RpcTaskManagerShared<WithdrawTask<T>, T>;
pub type WithdrawTaskHandle<T> = RpcTaskHandle<WithdrawTask<T>>;
pub type WithdrawRpcStatus<T> = RpcTaskStatusAlias<WithdrawTask<T>>;
pub type WithdrawInitResult<T> = Result<T, MmError<WithdrawError>>;

#[async_trait]
pub trait CoinWithdrawInit<T: ZRpcOps + Send> {
    fn init_withdraw(
        ctx: MmArc,
        req: WithdrawRequest,
        rpc_task_handle: &WithdrawTaskHandle<T>,
    ) -> WithdrawInitResult<TransactionDetails>;
}

pub async fn init_withdraw(ctx: MmArc, request: WithdrawRequest) -> WithdrawInitResult<InitWithdrawResponse> {
    let coin = lp_coinfind_or_err(&ctx, &request.coin).await?;
    let task = WithdrawTask {
        ctx: ctx.clone(),
        coin,
        request,
    };
    let coins_ctx = CoinsContext::from_ctx(&ctx).map_to_mm(WithdrawError::InternalError)?;
    let task_id = WithdrawTaskManager::spawn_rpc_task(&coins_ctx.withdraw_task_manager, task)?;
    Ok(InitWithdrawResponse { task_id })
}

pub async fn withdraw_status<T: ZRpcOps + Send>(
    ctx: MmArc,
    req: WithdrawStatusRequest,
) -> Result<WithdrawRpcStatus<T>, MmError<WithdrawStatusError>> {
    let coins_ctx = CoinsContext::from_ctx(&ctx).map_to_mm(WithdrawStatusError::Internal)?;
    let mut task_manager = coins_ctx
        .withdraw_task_manager
        .lock()
        .map_to_mm(|e| WithdrawStatusError::Internal(e.to_string()))?;
    task_manager
        .task_status(req.task_id, req.forget_if_finished)
        .or_mm_err(|| WithdrawStatusError::NoSuchTask(req.task_id))
}

#[derive(Clone, Serialize)]
pub enum WithdrawInProgressStatus {
    Preparing,
    GeneratingTransaction,
    SigningTransaction,
    Finishing,
    /// The following statuses don't require the user to send `UserAction`,
    /// but they tell the user that he should confirm/decline the operation on his device.
    WaitingForTrezorToConnect,
    WaitingForUserToConfirmPubkey,
    WaitingForUserToConfirmSigning,
}

pub async fn withdraw_user_action(
    ctx: MmArc,
    req: WithdrawUserActionRequest,
) -> Result<SuccessResponse, MmError<WithdrawUserActionError>> {
    let coins_ctx = CoinsContext::from_ctx(&ctx).map_to_mm(WithdrawUserActionError::Internal)?;
    let mut task_manager = coins_ctx
        .withdraw_task_manager
        .lock()
        .map_to_mm(|e| WithdrawUserActionError::Internal(e.to_string()))?;
    task_manager.on_user_action(req.task_id, req.user_action)?;
    Ok(SuccessResponse::new())
}

#[async_trait]
pub trait InitWithdrawCoin<T: ZRpcOps + Send> {
    async fn init_withdraw(
        &self,
        ctx: MmArc,
        req: WithdrawRequest,
        task_handle: &WithdrawTaskHandle<T>,
    ) -> Result<TransactionDetails, MmError<WithdrawError>>;
}

pub struct WithdrawTask<T: ZRpcOps + Send> {
    ctx: MmArc,
    coin: MmCoinEnum<T>,
    request: WithdrawRequest,
}

impl<T: ZRpcOps + Send> RpcTaskTypes<T> for WithdrawTask<T> {
    type Item = TransactionDetails;
    type Error = WithdrawError;
    type InProgressStatus = WithdrawInProgressStatus;
    type AwaitingStatus = WithdrawAwaitingStatus;
    type UserAction = WithdrawUserAction;
}

#[async_trait]
impl<T: ZRpcOps + Send> RpcTask<T> for WithdrawTask<T> {
    fn initial_status(&self) -> Self::InProgressStatus { WithdrawInProgressStatus::Preparing }

    async fn run(self, task_handle: &WithdrawTaskHandle<T>) -> Result<Self::Item, MmError<Self::Error>> {
        match self.coin {
            MmCoinEnum::UtxoCoin(ref standard_utxo) => {
                standard_utxo.init_withdraw(self.ctx, self.request, task_handle).await
            },
            MmCoinEnum::QtumCoin(ref qtum) => qtum.init_withdraw(self.ctx, self.request, task_handle).await,
            #[cfg(not(target_arch = "wasm32"))]
            MmCoinEnum::ZCoin(ref z) => z.init_withdraw(self.ctx, self.request, task_handle).await,
            _ => MmError::err(WithdrawError::CoinDoesntSupportInitWithdraw {
                coin: self.coin.ticker().to_owned(),
            }),
        }
    }
}
