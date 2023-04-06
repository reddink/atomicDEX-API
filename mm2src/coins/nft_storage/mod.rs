use crate::nft::nft_structs::{Chain, Nft, NftTransferHistory};
use async_trait::async_trait;
use mm2_err_handle::mm_error::NotMmError;
use mm2_err_handle::prelude::MmResult;

#[cfg(not(target_arch = "wasm32"))] pub mod sql_storage;
#[cfg(target_arch = "wasm32")] pub mod wasm_storage;

pub trait NftListStorageError: std::fmt::Debug + NotMmError + Send {}
pub trait NftTxHistoryStorageError: std::fmt::Debug + NotMmError + Send {}

#[async_trait]
pub trait NftListStorageOps {
    type Error: NftListStorageError;

    /// Initializes tables in storage for the specified chain type.
    async fn init(&self, chain: Chain) -> MmResult<(), Self::Error>;

    /// Whether tables are initialized for the specified chain.
    async fn is_initialized_for(&self, chain: Chain) -> MmResult<(), Self::Error>;

    async fn get_nft_list(&self, chain: Chain) -> MmResult<(), Self::Error>;

    async fn add_nfts<I>(&self, chain: Chain, nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = Nft> + Send + 'static,
        I::IntoIter: Send;
}

#[async_trait]
pub trait NftTxHistoryStorageOps {
    type Error: NftTxHistoryStorageError;

    /// Initializes tables in storage for the specified chain type.
    async fn init(&self, chain: Chain) -> MmResult<(), Self::Error>;

    /// Whether tables are initialized for the specified chain.
    async fn is_initialized_for(&self, chain: Chain) -> MmResult<(), Self::Error>;

    async fn get_tx_history(&self, chain: Chain) -> MmResult<(), Self::Error>;

    async fn add_txs_to_history<I>(&self, chain: Chain, nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = NftTransferHistory> + Send + 'static,
        I::IntoIter: Send;
}
