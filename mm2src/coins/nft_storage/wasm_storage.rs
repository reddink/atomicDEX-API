use crate::nft::nft_structs::{Chain, Nft, NftTransferHistory};
use crate::nft_storage::{CreateNftStorageError, NftListStorageOps, NftTxHistoryStorageOps};
use crate::CoinsContext;
use async_trait::async_trait;
use mm2_core::mm_ctx::MmArc;
pub use mm2_db::indexed_db::InitDbResult;
use mm2_db::indexed_db::{DbIdentifier, DbInstance, DbLocked, IndexedDb, IndexedDbBuilder, SharedDb};
use mm2_err_handle::map_to_mm::MapToMmResult;
use mm2_err_handle::prelude::MmResult;

const DB_NAME: &str = "nft_cache";
const DB_VERSION: u32 = 1;

pub type NftCacheDBLocked<'a> = DbLocked<'a, NftCacheDb>;

pub struct NftCacheDb {
    inner: IndexedDb,
}

#[async_trait]
impl DbInstance for NftCacheDb {
    fn db_name() -> &'static str { DB_NAME }

    async fn init(db_id: DbIdentifier) -> InitDbResult<Self> {
        // todo add tables for each chain?
        let inner = IndexedDbBuilder::new(db_id).with_version(DB_VERSION).build().await?;
        Ok(NftCacheDb { inner })
    }
}

impl NftCacheDb {
    fn get_inner(&self) -> &IndexedDb { &self.inner }
}

#[derive(Clone)]
pub struct IndexedDbNftStorage {
    db: SharedDb<NftCacheDb>,
}

impl IndexedDbNftStorage {
    pub fn new(ctx: &MmArc) -> MmResult<Self, CreateNftStorageError> {
        let coins_ctx = CoinsContext::from_ctx(ctx).map_to_mm(CreateNftStorageError::Internal)?;
        Ok(IndexedDbNftStorage {
            db: coins_ctx.nft_cache_db.clone(),
        })
    }
}

#[async_trait]
impl NftListStorageOps for IndexedDbNftStorage {
    type Error = ();

    async fn init(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn is_initialized_for(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn get_nft_list(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn add_nfts_to_list<I>(&self, chain: Chain, nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = Nft> + Send + 'static,
        I::IntoIter: Send,
    {
        todo!()
    }
}

#[async_trait]
impl NftTxHistoryStorageOps for IndexedDbNftStorage {
    type Error = ();

    async fn init(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn is_initialized_for(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn get_tx_history(&self, chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn add_txs_to_history<I>(&self, chain: Chain, nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = NftTransferHistory> + Send + 'static,
        I::IntoIter: Send,
    {
        todo!()
    }
}
