#[cfg(not(target_arch = "wasm32"))] mod tendermint_native_rpc;
#[cfg(target_arch = "wasm32")] mod tendermint_wasm_rpc;

#[cfg(not(target_arch = "wasm32"))]
pub use tendermint_native_rpc::*;

#[cfg(target_arch = "wasm32")]
pub use super::tendermint_wasm_rpc::*;
