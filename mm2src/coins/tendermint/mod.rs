// Module implementing Tendermint (Cosmos) integration
// Useful resources
// https://docs.cosmos.network/

#[path = "iris/htlc.rs"] mod htlc;
#[path = "iris/htlc_proto.rs"] mod htlc_proto;
mod rpc;
mod tendermint_coin;
mod tendermint_token;
pub use tendermint_coin::*;
pub use tendermint_token::*;
