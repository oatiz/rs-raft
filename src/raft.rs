use super::raft_log::{self, RaftLog};
use super::read_only::{ReadOnly, ReadOnlyOption, ReadState};
use super::storage::Storage;
use super::ProgressSet;
use eraftpb::{Entry, EntryType, HardState, Message, MessageType, Snapshot};
use fxhash::{FxHashMap, FxHashSet};

/// 节点状态
pub enum StateRole {
    /// 跟随者, (如果它接收不到leader的消息,那么它就要变成candidate)
    FOLLOWER,
    /// 候选人, (发起投票选举,获得大部分票行,会变为leader)
    Candidate,
    /// 领导人, (系统所有的更改都通过leader操作)
    Leader,
}

impl Default for StateRole {
    fn default() -> StateRole {
        StateRole::FOLLOWER
    }
}

#[derive(Default, PartialEq, Debug)]
pub struct SoftState {
    pub leader_id: u64,
    pub raft_state: StateRole,
}

pub struct Raft<T: Storage> {
    pub id: u64,

    pub state: StateRole,

    pub term: u64,

    pub vote: u64,

    pub read_state: Vec<ReadState>,

    pub log: RaftLog<T>,

    pub max_inflight: usize,

    pub max_msg_size: u64,

    prs: Option<ProgressSet>,

    pub is_learner: bool,

    pub votes: FxHashMap<u64, bool>,

    pub msgs: Vec<Message>,

    pub learner_id: u64,

    pub lead_transferee: Option<u64>,

    pub pending_conf_index: u64,

    pub read_only: ReadOnly,

    pub election_elapsed: usize,

    heartbeat_elapsed: usize,

    pub check_quorum: bool,

    pub pre_vote: bool,

    heartbeat_timeout: usize,

    election_timeout: usize,

    randomized_election_timeout: usize,

    min_election_timeout: usize,

    max_election_timeout: usize,

    tag: String,
}

//pub struct Raft {
//    // 所有服务器上持久存在的
//    /// 当前节点的ID
//    pub id: u64,
//
//    /// 当前节点的角色
//    pub state: StateRole,
//
//    /// 服务器最后一次知道的任期号 (初始化为 0,持续递增)
//    pub current_term: u64,
//
//    /// 候选人的Id (在当前任期获得选票)
//    pub vote_for: u64,
//
//    /// 日志集合 (每条日志包含一个用户状态机执行的指令和收到的任期号)
//    pub log: Vec![],
//
//    // 所有服务器上经常变的
//    /// 已知的最大的已经被提交的日志条目的索引值
//    pub commit_index: u64,
//
//    /// 最后被应用到状态机的日志条目索引值 (初始化为 0，持续递增)
//    pub last_applied: u64,
//
//    // 在领导人里经常改变的 (选举后重新初始化)
//    /// 在领导人里经常改变的 需要发送给每个服务器下一条日志的索引值 (初始化为领导人的最后索引值+1)
//    pub next_index: Vec![],
//
//    /// 每个服务器已给复制的日志最高索引值
//    pub match_index: Vec![],
//}

trait AssertSend: Send {}

impl<T: Storage + Send> AssertSend for Raft<T> {}

fn new_message(to: u64, field_type: MessageType, from: Option<u64>) -> Message {
    let mut msg = Message::new();
    msg.set_to(to);
    if let Some(id) = from {
        msg.set_from(id);
    }
    msg.set_msg_type(field_type);

    msg
}

pub fn vote_resp_msg_type(msg_type: MessageType) -> MessageType {
    match msg_type {
        MessageType::MsgRequestVote => MessageType::MsgRequestVoteResponse,
        MessageType::MsgRequestPreVote => MessageType::MsgRequestPreVoteResponse,
        _ => panic!("not a vote message: {:?}", msg_type),
    }
}

impl<T: Storage> Raft<T> {
    pub fn new (cfg: &Config)
}