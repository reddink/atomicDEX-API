// Module implementing Tendermint (Cosmos) integration
// Useful resources
// https://docs.cosmos.network/

mod iris;
mod rpc;
mod tendermint_coin;
mod tendermint_token;
pub mod tendermint_tx_history_v2;

pub use tendermint_coin::*;
pub use tendermint_token::*;

pub(crate) mod type_urls {
    pub(crate) const CREATE_HTLC_TYPE_URL: &str = "/irismod.htlc.MsgCreateHTLC";
    pub(crate) const CLAIM_HTLC_TYPE_URL: &str = "/irismod.htlc.MsgClaimHTLC";
}
