use async_trait::async_trait;
pub use mm2_db::indexed_db::InitDbResult;
use mm2_db::indexed_db::{DbIdentifier, DbInstance, DbLocked, IndexedDb, IndexedDbBuilder};

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
