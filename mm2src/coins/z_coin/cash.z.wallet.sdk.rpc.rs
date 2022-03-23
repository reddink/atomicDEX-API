/// CompactBlock is a packaging of ONLY the data from a block that's needed to:
///   1. Detect a payment to your shielded Sapling address
///   2. Detect a spend of your shielded Sapling notes
///   3. Update your witnesses to generate new Sapling spend proofs.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactBlock {
    /// the version of this wire format, for storage
    #[prost(uint32, tag="1")]
    pub proto_version: u32,
    /// the height of this block
    #[prost(uint64, tag="2")]
    pub height: u64,
    #[prost(bytes="vec", tag="3")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes="vec", tag="4")]
    pub prev_hash: ::prost::alloc::vec::Vec<u8>,
    #[prost(uint32, tag="5")]
    pub time: u32,
    /// (hash, prevHash, and time) OR (full header)
    #[prost(bytes="vec", tag="6")]
    pub header: ::prost::alloc::vec::Vec<u8>,
    /// compact transactions from this block
    #[prost(message, repeated, tag="7")]
    pub vtx: ::prost::alloc::vec::Vec<CompactTx>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactTx {
    /// Index and hash will allow the receiver to call out to chain
    /// explorers or other data structures to retrieve more information
    /// about this transaction.
    #[prost(uint64, tag="1")]
    pub index: u64,
    #[prost(bytes="vec", tag="2")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
    /// The transaction fee: present if server can provide. In the case of a
    /// stateless server and a transaction with transparent inputs, this will be
    /// unset because the calculation requires reference to prior transactions.
    /// in a pure-Sapling context, the fee will be calculable as:
    ///    valueBalance + (sum(vPubNew) - sum(vPubOld) - sum(tOut))
    #[prost(uint32, tag="3")]
    pub fee: u32,
    #[prost(message, repeated, tag="4")]
    pub spends: ::prost::alloc::vec::Vec<CompactSpend>,
    #[prost(message, repeated, tag="5")]
    pub outputs: ::prost::alloc::vec::Vec<CompactOutput>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactSpend {
    #[prost(bytes="vec", tag="1")]
    pub nf: ::prost::alloc::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompactOutput {
    #[prost(bytes="vec", tag="1")]
    pub cmu: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes="vec", tag="2")]
    pub epk: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes="vec", tag="3")]
    pub ciphertext: ::prost::alloc::vec::Vec<u8>,
}
/// A BlockID message contains identifiers to select a block: a height or a
/// hash. Specification by hash is not implemented, but may be in the future.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockId {
    #[prost(uint64, tag="1")]
    pub height: u64,
    #[prost(bytes="vec", tag="2")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
}
/// BlockRange specifies a series of blocks from start to end inclusive.
/// Both BlockIDs must be heights; specification by hash is not yet supported.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockRange {
    #[prost(message, optional, tag="1")]
    pub start: ::core::option::Option<BlockId>,
    #[prost(message, optional, tag="2")]
    pub end: ::core::option::Option<BlockId>,
}
/// A TxFilter contains the information needed to identify a particular
/// transaction: either a block and an index, or a direct transaction hash.
/// Currently, only specification by hash is supported.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TxFilter {
    /// block identifier, height or hash
    #[prost(message, optional, tag="1")]
    pub block: ::core::option::Option<BlockId>,
    /// index within the block
    #[prost(uint64, tag="2")]
    pub index: u64,
    /// transaction ID (hash, txid)
    #[prost(bytes="vec", tag="3")]
    pub hash: ::prost::alloc::vec::Vec<u8>,
}
/// RawTransaction contains the complete transaction data. It also optionally includes
/// the block height in which the transaction was included.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RawTransaction {
    /// exact data returned by Zcash 'getrawtransaction'
    #[prost(bytes="vec", tag="1")]
    pub data: ::prost::alloc::vec::Vec<u8>,
    /// height that the transaction was mined (or -1)
    #[prost(uint64, tag="2")]
    pub height: u64,
}
/// A SendResponse encodes an error code and a string. It is currently used
/// only by SendTransaction(). If error code is zero, the operation was
/// successful; if non-zero, it and the message specify the failure.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendResponse {
    #[prost(int32, tag="1")]
    pub error_code: i32,
    #[prost(string, tag="2")]
    pub error_message: ::prost::alloc::string::String,
}
/// Chainspec is a placeholder to allow specification of a particular chain fork.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ChainSpec {
}
/// Empty is for gRPCs that take no arguments, currently only GetLightdInfo.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Empty {
}
/// LightdInfo returns various information about this lightwalletd instance
/// and the state of the blockchain.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LightdInfo {
    #[prost(string, tag="1")]
    pub version: ::prost::alloc::string::String,
    #[prost(string, tag="2")]
    pub vendor: ::prost::alloc::string::String,
    /// true
    #[prost(bool, tag="3")]
    pub taddr_support: bool,
    /// either "main" or "test"
    #[prost(string, tag="4")]
    pub chain_name: ::prost::alloc::string::String,
    /// depends on mainnet or testnet
    #[prost(uint64, tag="5")]
    pub sapling_activation_height: u64,
    /// protocol identifier, see consensus/upgrades.cpp
    #[prost(string, tag="6")]
    pub consensus_branch_id: ::prost::alloc::string::String,
    /// latest block on the best chain
    #[prost(uint64, tag="7")]
    pub block_height: u64,
    #[prost(string, tag="8")]
    pub git_commit: ::prost::alloc::string::String,
    #[prost(string, tag="9")]
    pub branch: ::prost::alloc::string::String,
    #[prost(string, tag="10")]
    pub build_date: ::prost::alloc::string::String,
    #[prost(string, tag="11")]
    pub build_user: ::prost::alloc::string::String,
    /// less than tip height if zcashd is syncing
    #[prost(uint64, tag="12")]
    pub estimated_height: u64,
    /// example: "v4.1.1-877212414"
    #[prost(string, tag="13")]
    pub zcashd_build: ::prost::alloc::string::String,
    /// example: "/MagicBean:4.1.1/"
    #[prost(string, tag="14")]
    pub zcashd_subversion: ::prost::alloc::string::String,
}
/// TransparentAddressBlockFilter restricts the results to the given address
/// or block range.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransparentAddressBlockFilter {
    /// t-address
    #[prost(string, tag="1")]
    pub address: ::prost::alloc::string::String,
    /// start, end heights
    #[prost(message, optional, tag="2")]
    pub range: ::core::option::Option<BlockRange>,
}
/// Duration is currently used only for testing, so that the Ping rpc
/// can simulate a delay, to create many simultaneous connections. Units
/// are microseconds.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Duration {
    #[prost(int64, tag="1")]
    pub interval_us: i64,
}
/// PingResponse is used to indicate concurrency, how many Ping rpcs
/// are executing upon entry and upon exit (after the delay).
/// This rpc is used for testing only.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PingResponse {
    #[prost(int64, tag="1")]
    pub entry: i64,
    #[prost(int64, tag="2")]
    pub exit: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Address {
    #[prost(string, tag="1")]
    pub address: ::prost::alloc::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AddressList {
    #[prost(string, repeated, tag="1")]
    pub addresses: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Balance {
    #[prost(int64, tag="1")]
    pub value_zat: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Exclude {
    #[prost(bytes="vec", repeated, tag="1")]
    pub txid: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
}
/// The TreeState is derived from the Zcash z_gettreestate rpc.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TreeState {
    /// "main" or "test"
    #[prost(string, tag="1")]
    pub network: ::prost::alloc::string::String,
    #[prost(uint64, tag="2")]
    pub height: u64,
    /// block id
    #[prost(string, tag="3")]
    pub hash: ::prost::alloc::string::String,
    /// Unix epoch time when the block was mined
    #[prost(uint32, tag="4")]
    pub time: u32,
    /// sapling commitment tree state
    #[prost(string, tag="5")]
    pub tree: ::prost::alloc::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosArg {
    #[prost(string, tag="1")]
    pub address: ::prost::alloc::string::String,
    #[prost(uint64, tag="2")]
    pub start_height: u64,
    /// zero means unlimited
    #[prost(uint32, tag="3")]
    pub max_entries: u32,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosReply {
    #[prost(bytes="vec", tag="1")]
    pub txid: ::prost::alloc::vec::Vec<u8>,
    #[prost(int32, tag="2")]
    pub index: i32,
    #[prost(bytes="vec", tag="3")]
    pub script: ::prost::alloc::vec::Vec<u8>,
    #[prost(int64, tag="4")]
    pub value_zat: i64,
    #[prost(uint64, tag="5")]
    pub height: u64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetAddressUtxosReplyList {
    #[prost(message, repeated, tag="1")]
    pub address_utxos: ::prost::alloc::vec::Vec<GetAddressUtxosReply>,
}
