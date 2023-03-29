use crate::lightning::ln_platform::ChannelOpenAmount;
use crate::utxo::utxo_common::big_decimal_from_sat;
use crate::{lp_coinfind_or_err, CoinFindError, GenerateTxError, MmCoinEnum};
use common::HttpStatusCode;
use http::StatusCode;
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::prelude::*;
use mm2_number::BigDecimal;

type ChannelOpeningFeeResult<T> = Result<T, MmError<ChannelOpeningFeeError>>;

#[derive(Debug, Deserialize, Display, Serialize, SerializeErrorType)]
#[serde(tag = "error_type", content = "error_data")]
pub enum ChannelOpeningFeeError {
    #[display(fmt = "Lightning network is not supported for {}", _0)]
    UnsupportedCoin(String),
    #[display(fmt = "No such coin {}", _0)]
    NoSuchCoin(String),
    #[display(fmt = "Generate Tx Error {}", _0)]
    GenerateTxErr(String),
}

impl HttpStatusCode for ChannelOpeningFeeError {
    fn status_code(&self) -> StatusCode {
        match self {
            ChannelOpeningFeeError::UnsupportedCoin(_) => StatusCode::BAD_REQUEST,
            ChannelOpeningFeeError::GenerateTxErr(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ChannelOpeningFeeError::NoSuchCoin(_) => StatusCode::NOT_FOUND,
        }
    }
}

impl From<CoinFindError> for ChannelOpeningFeeError {
    fn from(e: CoinFindError) -> Self {
        match e {
            CoinFindError::NoSuchCoin { coin } => ChannelOpeningFeeError::NoSuchCoin(coin),
        }
    }
}

impl From<GenerateTxError> for ChannelOpeningFeeError {
    fn from(e: GenerateTxError) -> Self { ChannelOpeningFeeError::GenerateTxErr(e.to_string()) }
}

#[derive(Deserialize)]
pub struct ChannelOpeningFeeRequest {
    pub coin: String,
    pub amount: ChannelOpenAmount,
}

#[derive(Serialize)]
pub struct ChannelOpeningFeeResponse {
    channel_amount: BigDecimal,
    fee_amount: BigDecimal,
}

/// Returns the current fee for opening a channel on the lightning network.
pub async fn channel_opening_fee(
    ctx: MmArc,
    req: ChannelOpeningFeeRequest,
) -> ChannelOpeningFeeResult<ChannelOpeningFeeResponse> {
    let ln_coin = match lp_coinfind_or_err(&ctx, &req.coin).await? {
        MmCoinEnum::LightningCoin(c) => c,
        e => return MmError::err(ChannelOpeningFeeError::UnsupportedCoin(e.ticker().to_string())),
    };

    let (_, data) = ln_coin.platform.generate_funding_tx_preimage(req.amount).await?;
    let decimals = ln_coin.platform_coin().as_ref().decimals;
    let channel_amount = big_decimal_from_sat(data.spent_by_me as i64, decimals);
    let fee_amount_sats = data.fee_amount + data.unused_change.unwrap_or_default();
    let fee_amount = big_decimal_from_sat(fee_amount_sats as i64, decimals);

    Ok(ChannelOpeningFeeResponse {
        channel_amount,
        fee_amount,
    })
}
