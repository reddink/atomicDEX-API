use crate::nft::nft_structs::{Chain, Nft, NftTransferHistory};
use crate::nft_storage::{CreateNftStorageError, NftListStorageOps, NftTxHistoryStorageOps};
use async_trait::async_trait;
use db_common::sqlite::rusqlite::Connection;
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::mm_error::{MmError, MmResult};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SqliteNftStorage(Arc<Mutex<Connection>>);

impl SqliteNftStorage {
    pub fn new(ctx: &MmArc) -> MmResult<Self, CreateNftStorageError> {
        let sqlite_connection = ctx
            .sqlite_connection
            .ok_or(MmError::new(CreateNftStorageError::Internal(
                "sqlite_connection is not initialized".to_owned(),
            )))?;
        Ok(SqliteNftStorage(sqlite_connection.clone()))
    }
}

#[async_trait]
impl NftListStorageOps for SqliteNftStorage {
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
impl NftTxHistoryStorageOps for SqliteNftStorage {
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
