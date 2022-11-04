#[cfg(not(target_arch = "wasm32"))] mod tendermint_native_rpc;
#[cfg(not(target_arch = "wasm32"))]
pub use tendermint_native_rpc::*;

#[cfg(target_arch = "wasm32")] mod tendermint_wasm_rpc;
#[cfg(target_arch = "wasm32")] pub use tendermint_wasm_rpc::*;

pub enum TendermintResultOrder {
    Ascending,
    Descending,
}

impl From<TendermintResultOrder> for Order {
    fn from(order: TendermintResultOrder) -> Self {
        match order {
            TendermintResultOrder::Ascending => Self::Ascending,
            TendermintResultOrder::Descending => Self::Descending,
        }
    }
}

impl From<TendermintResultOrder> for i32 {
    fn from(order: TendermintResultOrder) -> Self {
        match order {
            TendermintResultOrder::Ascending => 0,
            TendermintResultOrder::Descending => 1,
        }
    }
}
