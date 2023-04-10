use crate::nft_storage::CreateNftStorageError;
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
    pub fn new(ctx: &MmArc) -> MmResult<Self, CreateNftStorageError>
    where
        Self: Sized,
    {
        let coins_ctx = CoinsContext::from_ctx(ctx).map_to_mm(CreateNftStorageError::Internal)?;
        Ok(IndexedDbNftStorage {
            db: coins_ctx.nft_cache_db.clone(),
        })
    }
}
