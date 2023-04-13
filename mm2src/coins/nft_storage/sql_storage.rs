use crate::nft::nft_structs::{Chain, ConvertChain, Nft, NftList, NftTransferHistory, NftsTransferHistoryList};
use crate::nft_storage::{CreateNftStorageError, NftListStorageError, NftListStorageOps, NftTxHistoryStorageError,
                         NftTxHistoryStorageOps};
use async_trait::async_trait;
use db_common::sqlite::rusqlite::{Connection, Error as SqlError};
use mm2_core::mm_ctx::MmArc;
use mm2_err_handle::mm_error::{MmError, MmResult};
use std::sync::{Arc, Mutex};

fn nft_list_table_name(chain: &Chain) -> String { chain.to_ticker() + "_nft_list" }

fn nft_tx_history_table_name(chain: &Chain) -> String { chain.to_ticker() + "_nft_tx_history" }

fn create_nft_list_table_sql(chain: &Chain) -> MmResult<String, SqlError> {
    let table_name = nft_list_table_name(chain);
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (
    chain TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_id VARCHAR(256) NOT NULL,
    amount VARCHAR(256) NOT NULL,
    owner_of TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    block_number_minted INTEGER NOT NULL,
    block_number INTEGER NOT NULL,
    contract_type TEXT,
    name TEXT,
    symbol TEXT,
    token_uri TEXT,
    metadata BLOB,
    last_token_uri_sync TEXT,
    last_metadata_sync TEXT,
    minter_address TEXT,
    PRIMARY KEY (token_address, token_id)
        );",
        table_name
    );
    Ok(sql)
}

fn create_tx_history_table_sql(chain: &Chain) -> MmResult<String, SqlError> {
    let table_name = nft_tx_history_table_name(chain);
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (
    chain TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    block_timestamp TEXT NOT NULL,
    block_hash TEXT NOT NULL,
    transaction_hash VARCHAR(256) PRIMARY KEY,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    value VARCHAR(256) NOT NULL,
    contract_type TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_id VARCHAR(256) NOT NULL,
    from_address TEXT NOT NULL,
    to_address TEXT NOT NULL,
    amount VARCHAR(256) NOT NULL,
    verified INTEGER NOT NULL,
    operator TEXT
        );",
        table_name
    );
    Ok(sql)
}

impl NftListStorageError for SqlError {}
impl NftTxHistoryStorageError for SqlError {}

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
    type Error = SqlError;

    async fn init(&self, _chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn is_initialized_for(&self, _chain: Chain) -> MmResult<bool, Self::Error> { todo!() }

    async fn get_nft_list(&self, _chain: Chain) -> MmResult<NftList, Self::Error> { todo!() }

    async fn add_nfts_to_list<I>(&self, _chain: Chain, _nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = Nft> + Send + 'static,
        I::IntoIter: Send,
    {
        todo!()
    }

    async fn remove_nft_from_list(&self, _nft: Nft) -> MmResult<(), Self::Error> { todo!() }
}

#[async_trait]
impl NftTxHistoryStorageOps for SqliteNftStorage {
    type Error = SqlError;

    async fn init(&self, _chain: Chain) -> MmResult<(), Self::Error> { todo!() }

    async fn is_initialized_for(&self, _chain: Chain) -> MmResult<bool, Self::Error> { todo!() }

    async fn get_tx_history(&self, _chain: Chain) -> MmResult<NftsTransferHistoryList, Self::Error> { todo!() }

    async fn add_txs_to_history<I>(&self, _chain: Chain, _nfts: I) -> MmResult<(), Self::Error>
    where
        I: IntoIterator<Item = NftTransferHistory> + Send + 'static,
        I::IntoIter: Send,
    {
        todo!()
    }
}
