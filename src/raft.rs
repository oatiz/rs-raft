use super::errors::{Error, Result, StorageError};
use super::progress::{CandidacyStatus, Progress, ProgressSet, ProgressState};
use super::raft_log::{self, RaftLog};
use super::read_only::{ReadOnly, ReadOnlyOption, ReadState};
use super::storage::Storage;
use super::Config;
use eraftpb::{Entry, EntryType, HardState, Message, MessageType, Snapshot};
use fxhash::{FxHashMap, FxHashSet};
use rand::{self, Rng};

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

pub const INVALID_ID: u64 = 0;
pub const INVALID_INDEX: u64 = 0;

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

    skip_bcast_commit: bool,

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
    pub fn new(cfg: &Config, storage: T) -> Result<Raft<T>> {
        cfg.validate()?;
        let rs = storage.initial_state()?;
        let conf_state = &rs.conf_state;
        let raft_log = RaftLog::new(storage, cfg.tag.clone());
        let mut peers: &[u64] = &cfg.peers;
        let mut learners: &[u64] = &cfg.learners;
        if !conf_state.get_nodes().is_empty() || !conf_state.get_learners().is_empty() {
            if !peers.is_empty() || !learners.is_empty() {
                panic!(
                    "{} cannot specify both new(peers/learners) and ConfState.(Nodes/Learners)",
                    cfg.tag
                );
            }
            peers = conf_state.get_nodes();
            learners = conf_state.get_learners();
        }

        let mut raft = Raft {
            id: cfg.id,
            state: StateRole::FOLLOWER,
            term: Default::default(),
            vote: Default::default(),
            read_state: Default::default(),
            log: raft_log,
            max_inflight: cfg.max_inflight_msgs,
            max_msg_size: cfg.max_size_per_msg,
            prs: Some(ProgressSet::with_capacity(peers.len(), learners.len())),
            is_learner: false,
            votes: Default::default(),
            msgs: Default::default(),
            learner_id: Default::default(),
            lead_transferee: None,
            pending_conf_index: Default::default(),
            read_only: ReadOnly::new(cfg.read_only_option),
            election_elapsed: Default::default(),
            heartbeat_elapsed: Default::default(),
            check_quorum: cfg.check_quorum,
            pre_vote: cfg.pre_vote,
            skip_bcast_commit: cfg.skip_bcast_commit,
            heartbeat_timeout: cfg.heartbeat_tick,
            election_timeout: cfg.election_tick,
            randomized_election_timeout: 0,
            min_election_timeout: cfg.min_election_tick,
            max_election_timeout: cfg.max_election_tick,
            tag: cfg.tag.to_owned(),
        };

        for peer in peers {
            let progress = Progress::new(1, raft.max_inflight);
            if let Err(e) = raft.mut_prs().insert_learner(*peer, progress) {
                panic!("{}", e);
            };
        }
        for learner in learners {
            let progress = Progress::new(1, raft.max_inflight);
            if let Err(e) = raft.mut_prs().insert_learner(*learner, progress) {
                panic!("{}", e);
            };
            if *learner == raft.id {
                raft.is_learner = true;
            }
        }

        if rs.hard_state != HardState::new() {
            raft.load_state(&rs.hard_state);
        }
        if cfg.applied > 0 {
            raft.log.applied_to(cfg.applied);
        }
        let term = raft.term;

        Ok(())
    }

    pub fn reset(&mut self, term: u64) {
        if self.term != term {
            self.term = term;
            self.vote = INVALID_ID;
        }
        self.learner_id = INVALID_ID;
        self.reset_randomized_election_timeout();
        self.election_elapsed = 0;
        self.heartbeat_elapsed = 0;

        //        self.
    }

    pub fn become_fowller(&mut self, term: u64, leader_id: u64) {
        self.re
    }

    pub fn mut_prs(&mut self) -> &mut ProgressSet {
        self.prs.as_mut().unwrap()
    }

    pub fn load_state(&mut self, hs: &HardState) {
        if hs.get_commit() < self.log.committed || hs.get_commit() > self.log.last_index() {
            panic!(
                "{} hs.commit {} is out of range [{}, {}]",
                self.tag,
                hs.get_commit(),
                self.log.committed,
                self.log.last_index(),
            );
        }
        self.log.committed = hs.get_commit();
        self.term = hs.get_term();
        self.vote = hs.get_vote();
    }

    pub fn reset_randomized_election_timeout(&mut self) {
        let prev_timeout = self.randomized_election_timeout;
        let timeout =
            rand::thread_rng().gen_range(self.min_election_timeout, self.max_election_timeout);
        debug!(
            "{} reset election timeout {} -> {} at {}",
            self.tag, prev_timeout, timeout, self.election_elapsed
        );
        self.randomized_election_timeout = timeout;
    }
}
