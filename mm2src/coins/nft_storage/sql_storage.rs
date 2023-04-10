use crate::nft_storage::CreateNftStorageError;
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
