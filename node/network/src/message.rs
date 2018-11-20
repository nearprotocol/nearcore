use primitives::hash::CryptoHash;
use primitives::traits::Block;
use primitives::types::BlockId;

pub type RequestId = u64;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub enum MessageBody<T, B> {
    //TODO: add different types of messages here
    Transaction(T),
    Status(Status),
    BlockRequest(BlockRequest),
    BlockResponse(BlockResponse<B>),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Message<T, B> {
    pub body: MessageBody<T, B>,
}

impl<T, B: Block> Message<T, B> {
    pub fn new(body: MessageBody<T, B>) -> Message<T, B> {
        Message { body }
    }
}

/// status sent on connection
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    // protocol version
    pub version: u32,
    // best block number
    pub best_number: u64,
    // best block hash
    pub best_hash: CryptoHash,
    // genesis hash
    pub genesis_hash: CryptoHash,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            version: 1,
            best_number: 0,
            best_hash: CryptoHash::default(),
            genesis_hash: CryptoHash::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRequest {
    // request id
    pub id: RequestId,
    // starting from this id
    pub from: BlockId,
    // ending at this id,
    pub to: Option<BlockId>,
    // max number of blocks requested
    pub max: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockResponse<Block> {
    // request id that the response is responding to
    pub id: RequestId,
    // block data
    pub blocks: Vec<Block>,
}
