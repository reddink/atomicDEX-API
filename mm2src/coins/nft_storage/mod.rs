use crate::nft::nft_structs::{Chain, Nft, NftTransferHistory};
use async_trait::async_trait;
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::mm_error::NotMmError;
use mm2_err_handle::prelude::MmResult;
use std::marker::PhantomData;

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

    async fn add_nfts_to_list<I>(&self, chain: Chain, nfts: I) -> MmResult<(), Self::Error>
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

pub enum CreateNftStorageError {
    Internal(String),
}

pub trait StorageFactory {
    fn create(ctx: &MmArc) -> MmResult<Self, CreateNftStorageError>
    where
        Self: Sized;
}

/// `NftStorageBuilder` is used to create an instance that implements the `NftListStorageOps`
/// and `NftTxHistoryStorageOps` traits.
#[allow(dead_code)]
pub struct NftStorageBuilder<'a, T> {
    ctx: &'a MmArc,
    _phantom: PhantomData<T>,
}

impl<'a, T> NftStorageBuilder<'a, T>
where
    T: NftListStorageOps + NftTxHistoryStorageOps + StorageFactory,
{
    #[inline]
    pub fn new(ctx: &MmArc) -> NftStorageBuilder<'_, T> {
        NftStorageBuilder {
            ctx,
            _phantom: Default::default(),
        }
    }

    #[inline]
    pub fn build(self) -> MmResult<T, CreateNftStorageError>
    where
        Self: Sized,
    {
        T::create(self.ctx)
    }
}
