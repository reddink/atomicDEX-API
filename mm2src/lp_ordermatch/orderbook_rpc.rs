use super::{subscribe_to_orderbook_topic, OrdermatchContext, RpcOrderbookEntry};
use crate::mm2::lp_ordermatch::{addr_format_from_protocol_info, RpcOrderbookEntryV2};
use coins::utxo::UtxoAddressFormat;
use coins::{address_by_coin_conf_and_pubkey_str, coin_conf, is_wallet_only_conf, CoinProtocol};
use common::log::warn;
use common::mm_error::prelude::{MapMmError, MapToMmResult};
use common::mm_error::MmError;
use common::mm_number::MmNumberMultiRepr;
use common::{mm_ctx::MmArc, mm_number::MmNumber, now_ms, HttpStatusCode};
use crypto::CryptoCtx;
use derive_more::Display;
use http::{Response, StatusCode};
use num_rational::BigRational;
use num_traits::Zero;
use serde_json::{self as json, Error, Value as Json};

#[derive(Deserialize)]
pub struct OrderbookReq {
    base: String,
    rel: String,
}

construct_detailed!(TotalAsksBaseVol, total_asks_base_vol);
construct_detailed!(TotalAsksRelVol, total_asks_rel_vol);
construct_detailed!(TotalBidsBaseVol, total_bids_base_vol);
construct_detailed!(TotalBidsRelVol, total_bids_rel_vol);
construct_detailed!(AggregatedBaseVol, base_max_volume_aggr);
construct_detailed!(AggregatedRelVol, rel_max_volume_aggr);

#[derive(Debug, Serialize)]
pub struct AggregatedOrderbookEntry {
    #[serde(flatten)]
    entry: RpcOrderbookEntry,
    #[serde(flatten)]
    base_max_volume_aggr: AggregatedBaseVol,
    #[serde(flatten)]
    rel_max_volume_aggr: AggregatedRelVol,
}

#[derive(Debug, Serialize)]
pub struct AggregatedOrderbookEntryV2 {
    #[serde(flatten)]
    entry: RpcOrderbookEntryV2,
    base_max_volume_aggr: MmNumberMultiRepr,
    rel_max_volume_aggr: MmNumberMultiRepr,
}

#[derive(Debug, Serialize)]
pub struct OrderbookResponse {
    #[serde(rename = "askdepth")]
    ask_depth: u32,
    asks: Vec<AggregatedOrderbookEntry>,
    base: String,
    #[serde(rename = "biddepth")]
    bid_depth: u32,
    bids: Vec<AggregatedOrderbookEntry>,
    netid: u16,
    #[serde(rename = "numasks")]
    num_asks: usize,
    #[serde(rename = "numbids")]
    num_bids: usize,
    rel: String,
    timestamp: u64,
    #[serde(flatten)]
    total_asks_base: TotalAsksBaseVol,
    #[serde(flatten)]
    total_asks_rel: TotalAsksRelVol,
    #[serde(flatten)]
    total_bids_base: TotalBidsBaseVol,
    #[serde(flatten)]
    total_bids_rel: TotalBidsRelVol,
}

fn build_aggregated_entries(entries: Vec<RpcOrderbookEntry>) -> (Vec<AggregatedOrderbookEntry>, MmNumber, MmNumber) {
    let mut total_base = BigRational::zero();
    let mut total_rel = BigRational::zero();
    let aggregated = entries
        .into_iter()
        .map(|entry| {
            total_base += entry.base_max_volume.as_ratio();
            total_rel += entry.rel_max_volume.as_ratio();
            AggregatedOrderbookEntry {
                entry,
                base_max_volume_aggr: MmNumber::from(total_base.clone()).into(),
                rel_max_volume_aggr: MmNumber::from(total_rel.clone()).into(),
            }
        })
        .collect();
    (aggregated, total_base.into(), total_rel.into())
}

fn build_aggregated_entries_v2(
    entries: Vec<RpcOrderbookEntryV2>,
) -> (Vec<AggregatedOrderbookEntryV2>, MmNumberMultiRepr, MmNumberMultiRepr) {
    let mut total_base = BigRational::zero();
    let mut total_rel = BigRational::zero();
    let aggregated = entries
        .into_iter()
        .map(|entry| {
            total_base += &entry.base_max_volume.rational;
            total_rel += &entry.rel_max_volume.rational;
            AggregatedOrderbookEntryV2 {
                entry,
                base_max_volume_aggr: MmNumber::from(total_base.clone()).into(),
                rel_max_volume_aggr: MmNumber::from(total_rel.clone()).into(),
            }
        })
        .collect();
    (aggregated, total_base.into(), total_rel.into())
}

pub async fn orderbook_rpc(ctx: MmArc, req: Json) -> Result<Response<Vec<u8>>, String> {
    let req: OrderbookReq = try_s!(json::from_value(req));
    if req.base == req.rel {
        return ERR!("Base and rel must be different coins");
    }
    let base_coin_conf = coin_conf(&ctx, &req.base);
    if base_coin_conf.is_null() {
        return ERR!("Coin {} is not found in config", req.base);
    }
    if is_wallet_only_conf(&base_coin_conf) {
        return ERR!("Base Coin {} is wallet only", req.base);
    }
    let rel_coin_conf = coin_conf(&ctx, &req.rel);
    if rel_coin_conf.is_null() {
        return ERR!("Coin {} is not found in config", req.rel);
    }
    if is_wallet_only_conf(&rel_coin_conf) {
        return ERR!("Base Coin {} is wallet only", req.rel);
    }
    let ordermatch_ctx = try_s!(OrdermatchContext::from_ctx(&ctx));
    let request_orderbook = true;
    let base_ticker = ordermatch_ctx.orderbook_ticker_bypass(&req.base);
    let rel_ticker = ordermatch_ctx.orderbook_ticker_bypass(&req.rel);
    if base_ticker == rel_ticker && base_coin_conf["protocol"] == rel_coin_conf["protocol"] {
        return ERR!("Base and rel coins have the same orderbook tickers and protocols.");
    }

    try_s!(subscribe_to_orderbook_topic(&ctx, &base_ticker, &rel_ticker, request_orderbook).await);
    let orderbook = ordermatch_ctx.orderbook.lock();
    let my_pubsecp = try_s!(CryptoCtx::from_ctx(&ctx)).secp256k1_pubkey_hex();

    let mut asks = match orderbook.unordered.get(&(base_ticker.clone(), rel_ticker.clone())) {
        Some(uuids) => {
            let mut orderbook_entries = Vec::new();
            for uuid in uuids {
                let ask = orderbook.order_set.get(uuid).ok_or(ERRL!(
                    "Orderbook::unordered contains {:?} uuid that is not in Orderbook::order_set",
                    uuid
                ))?;
                let address_format = addr_format_from_protocol_info(&ask.base_protocol_info);
                let address = try_s!(address_by_coin_conf_and_pubkey_str(
                    &ctx,
                    &req.base,
                    &base_coin_conf,
                    &ask.pubkey,
                    address_format,
                ));
                let is_mine = my_pubsecp == ask.pubkey;
                orderbook_entries.push(ask.as_rpc_entry_ask(address, is_mine));
            }
            orderbook_entries
        },
        None => Vec::new(),
    };
    asks.sort_unstable_by(|ask1, ask2| ask1.price_rat.cmp(&ask2.price_rat));
    let (mut asks, total_asks_base_vol, total_asks_rel_vol) = build_aggregated_entries(asks);
    asks.reverse();

    let mut bids = match orderbook.unordered.get(&(rel_ticker, base_ticker)) {
        Some(uuids) => {
            let mut orderbook_entries = vec![];
            for uuid in uuids {
                let bid = orderbook.order_set.get(uuid).ok_or(ERRL!(
                    "Orderbook::unordered contains {:?} uuid that is not in Orderbook::order_set",
                    uuid
                ))?;
                let address_format = addr_format_from_protocol_info(&bid.base_protocol_info);
                let address = try_s!(address_by_coin_conf_and_pubkey_str(
                    &ctx,
                    &req.rel,
                    &rel_coin_conf,
                    &bid.pubkey,
                    address_format,
                ));
                let is_mine = my_pubsecp == bid.pubkey;
                orderbook_entries.push(bid.as_rpc_entry_bid(address, is_mine));
            }
            orderbook_entries
        },
        None => vec![],
    };
    bids.sort_unstable_by(|bid1, bid2| bid2.price_rat.cmp(&bid1.price_rat));
    let (bids, total_bids_base_vol, total_bids_rel_vol) = build_aggregated_entries(bids);

    let response = OrderbookResponse {
        num_asks: asks.len(),
        num_bids: bids.len(),
        ask_depth: 0,
        asks,
        base: req.base,
        bid_depth: 0,
        bids,
        netid: ctx.netid(),
        rel: req.rel,
        timestamp: now_ms() / 1000,
        total_asks_base: total_asks_base_vol.into(),
        total_asks_rel: total_asks_rel_vol.into(),
        total_bids_base: total_bids_base_vol.into(),
        total_bids_rel: total_bids_rel_vol.into(),
    };
    let response = try_s!(json::to_vec(&response));
    Ok(try_s!(Response::builder().body(response)))
}

#[derive(Debug, Display, Serialize, SerializeErrorType)]
#[serde(tag = "error_type", content = "error_data")]
pub enum OrderbookRpcError {
    BaseRelSame,
    BaseRelSameOrderbookTickersAndProtocols,
    CoinConfigNotFound(String),
    CoinIsWalletOnly(String),
    P2PSubscribeError(String),
}

impl HttpStatusCode for OrderbookRpcError {
    fn status_code(&self) -> StatusCode {
        match self {
            OrderbookRpcError::BaseRelSame
            | OrderbookRpcError::BaseRelSameOrderbookTickersAndProtocols
            | OrderbookRpcError::CoinConfigNotFound(_)
            | OrderbookRpcError::CoinIsWalletOnly(_) => StatusCode::BAD_REQUEST,
            OrderbookRpcError::P2PSubscribeError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
pub struct OrderbookV2Response {
    asks: Vec<AggregatedOrderbookEntryV2>,
    base: String,
    bids: Vec<AggregatedOrderbookEntryV2>,
    net_id: u16,
    num_asks: usize,
    num_bids: usize,
    rel: String,
    timestamp: u64,
    total_asks_base_vol: MmNumberMultiRepr,
    total_asks_rel_vol: MmNumberMultiRepr,
    total_bids_base_vol: MmNumberMultiRepr,
    total_bids_rel_vol: MmNumberMultiRepr,
}

pub async fn orderbook_rpc_v2(
    ctx: MmArc,
    req: OrderbookReq,
) -> Result<OrderbookV2Response, MmError<OrderbookRpcError>> {
    if req.base == req.rel {
        return MmError::err(OrderbookRpcError::BaseRelSame);
    }
    let base_coin_conf = coin_conf(&ctx, &req.base);
    if base_coin_conf.is_null() {
        return MmError::err(OrderbookRpcError::CoinConfigNotFound(req.base));
    }
    if is_wallet_only_conf(&base_coin_conf) {
        return MmError::err(OrderbookRpcError::CoinIsWalletOnly(req.base));
    }
    let rel_coin_conf = coin_conf(&ctx, &req.rel);
    if rel_coin_conf.is_null() {
        return MmError::err(OrderbookRpcError::CoinConfigNotFound(req.rel));
    }
    if is_wallet_only_conf(&rel_coin_conf) {
        return MmError::err(OrderbookRpcError::CoinIsWalletOnly(req.rel));
    }
    let ordermatch_ctx = OrdermatchContext::from_ctx(&ctx).expect("ctx is available");
    let base_ticker = ordermatch_ctx.orderbook_ticker_bypass(&req.base);
    let rel_ticker = ordermatch_ctx.orderbook_ticker_bypass(&req.rel);
    if base_ticker == rel_ticker && base_coin_conf["protocol"] == rel_coin_conf["protocol"] {
        return MmError::err(OrderbookRpcError::BaseRelSameOrderbookTickersAndProtocols);
    }

    let request_orderbook = true;
    subscribe_to_orderbook_topic(&ctx, &base_ticker, &rel_ticker, request_orderbook)
        .await
        .map_to_mm(OrderbookRpcError::P2PSubscribeError)?;

    let orderbook = ordermatch_ctx.orderbook.lock();
    let my_pubsecp = CryptoCtx::from_ctx(&ctx)
        .expect("ctx is available")
        .secp256k1_pubkey_hex();

    let mut asks = match orderbook.unordered.get(&(base_ticker.clone(), rel_ticker.clone())) {
        Some(uuids) => {
            let mut orderbook_entries = Vec::new();
            for uuid in uuids {
                let ask = match orderbook.order_set.get(uuid) {
                    Some(a) => a,
                    None => {
                        warn!("unordered contains {:?} uuid that is not in order_set", uuid);
                        continue;
                    },
                };
                let address_format = addr_format_from_protocol_info(&ask.base_protocol_info);
                let address = match orderbook_address(&ctx, &req.base, &base_coin_conf, &ask.pubkey, address_format) {
                    Ok(a) => a,
                    Err(e) => {
                        warn!("Error {} on getting address for order {}", e, ask.uuid);
                        continue;
                    },
                };
                let is_mine = my_pubsecp == ask.pubkey;
                orderbook_entries.push(ask.as_rpc_v2_entry_ask(address, is_mine));
            }
            orderbook_entries
        },
        None => Vec::new(),
    };
    asks.sort_unstable_by(|ask1, ask2| ask1.price.rational.cmp(&ask2.price.rational));
    let (mut asks, total_asks_base_vol, total_asks_rel_vol) = build_aggregated_entries_v2(asks);
    asks.reverse();

    let mut bids = match orderbook.unordered.get(&(rel_ticker, base_ticker)) {
        Some(uuids) => {
            let mut orderbook_entries = vec![];
            for uuid in uuids {
                let bid = match orderbook.order_set.get(uuid) {
                    Some(b) => b,
                    None => {
                        warn!("unordered contains {:?} uuid that is not in order_set", uuid);
                        continue;
                    },
                };
                let address_format = addr_format_from_protocol_info(&bid.base_protocol_info);
                let address = match orderbook_address(&ctx, &req.rel, &rel_coin_conf, &bid.pubkey, address_format) {
                    Ok(a) => a,
                    Err(e) => {
                        warn!("Error {} on getting address for order {}", e, bid.uuid);
                        continue;
                    },
                };
                let is_mine = my_pubsecp == bid.pubkey;
                orderbook_entries.push(bid.as_rpc_v2_entry_bid(address, is_mine));
            }
            orderbook_entries
        },
        None => vec![],
    };
    bids.sort_unstable_by(|bid1, bid2| bid2.price.rational.cmp(&bid1.price.rational));
    let (bids, total_bids_base_vol, total_bids_rel_vol) = build_aggregated_entries_v2(bids);

    Ok(OrderbookV2Response {
        num_asks: asks.len(),
        num_bids: bids.len(),
        asks,
        bids,
        net_id: ctx.netid(),
        base: req.base,
        rel: req.rel,
        timestamp: now_ms() / 1000,
        total_asks_base_vol,
        total_asks_rel_vol,
        total_bids_base_vol,
        total_bids_rel_vol,
    })
}

#[derive(Debug, Serialize)]
#[serde(tag = "address_type", content = "address_data")]
pub enum OrderbookAddress {
    Transparent(String),
    Shielded,
}

#[derive(Debug, Display)]
enum OrderbookAddrErr {
    AddrFromPubkeyError(String),
    CoinIsNotSupported(String),
    DeserializationError(json::Error),
    InvalidPlatformCoinProtocol(String),
    PlatformCoinConfIsNull(String),
}

impl From<json::Error> for OrderbookAddrErr {
    fn from(err: Error) -> Self { OrderbookAddrErr::DeserializationError(err) }
}

fn orderbook_address(
    ctx: &MmArc,
    coin: &str,
    conf: &Json,
    pubkey: &str,
    addr_format: UtxoAddressFormat,
) -> Result<OrderbookAddress, MmError<OrderbookAddrErr>> {
    let protocol: CoinProtocol = json::from_value(conf["protocol"].clone())?;
    match protocol {
        CoinProtocol::ERC20 { .. } | CoinProtocol::ETH => coins::eth::addr_from_pubkey_str(pubkey)
            .map(OrderbookAddress::Transparent)
            .map_to_mm(OrderbookAddrErr::AddrFromPubkeyError),
        CoinProtocol::UTXO | CoinProtocol::QTUM | CoinProtocol::QRC20 { .. } | CoinProtocol::BCH { .. } => {
            coins::utxo::address_by_conf_and_pubkey_str(coin, conf, pubkey, addr_format)
                .map(OrderbookAddress::Transparent)
                .map_to_mm(OrderbookAddrErr::AddrFromPubkeyError)
        },
        CoinProtocol::SLPTOKEN { platform, .. } => {
            let platform_conf = coin_conf(ctx, &platform);
            if platform_conf.is_null() {
                return MmError::err(OrderbookAddrErr::PlatformCoinConfIsNull(platform));
            }
            // TODO is there any way to make it better without duplicating the prefix in the SLP conf?
            let platform_protocol: CoinProtocol = json::from_value(platform_conf["protocol"].clone())?;
            match platform_protocol {
                CoinProtocol::BCH { slp_prefix } => coins::utxo::slp::slp_addr_from_pubkey_str(pubkey, &slp_prefix)
                    .map(OrderbookAddress::Transparent)
                    .mm_err(|e| OrderbookAddrErr::AddrFromPubkeyError(e.to_string())),
                _ => MmError::err(OrderbookAddrErr::InvalidPlatformCoinProtocol(platform)),
            }
        },
        #[cfg(not(target_arch = "wasm32"))]
        CoinProtocol::LIGHTNING { .. } => MmError::err(OrderbookAddrErr::CoinIsNotSupported(coin.to_owned())),
        #[cfg(all(not(target_arch = "wasm32"), feature = "zhtlc"))]
        CoinProtocol::ZHTLC => Ok(OrderbookAddress::Shielded),
    }
}
