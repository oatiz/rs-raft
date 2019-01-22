use super::errors::{Error, Result, StorageError};
use super::progress::{CandidacyStatus, Progress, ProgressSet, ProgressState};
use super::raft_log::{self, RaftLog};
use super::read_only::{ReadOnly, ReadOnlyOption, ReadState};
use super::storage::Storage;
use super::Config;
use core::cmp;
use eraftpb::{Entry, EntryType, HardState, Message, MessageType, Snapshot};
use fxhash::{FxHashMap, FxHashSet};
use protobuf::RepeatedField;
use rand::{self, Rng};

// CAMPAIGN_PRE_ELECTION represents the first phase of a normal election when
// Config.pre_vote is true.
const CAMPAIGN_PRE_ELECTION: &[u8] = b"CampaignPreElection";
// CAMPAIGN_ELECTION represents a normal (time-based) election (the second phase
// of the election when Config.pre_vote is true).
const CAMPAIGN_ELECTION: &[u8] = b"CampaignElection";
// CAMPAIGN_TRANSFER represents the type of leader transfer.
const CAMPAIGN_TRANSFER: &[u8] = b"CampaignTransfer";

/// 节点状态
pub enum StateRole {
    /// 跟随者, (如果它接收不到leader的消息,那么它就要变成candidate)
    FOLLOWER,
    /// 候选人, (发起投票选举,获得大部分票行,会变为leader)
    Candidate,
    /// 领导人, (系统所有的更改都通过leader操作)
    Leader,
    PreCandidate,
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

    pub raft_log: RaftLog<T>,

    pub max_inflight: usize,

    pub max_msg_size: u64,

    prs: Option<ProgressSet>,

    pub is_learner: bool,

    pub votes: FxHashMap<u64, bool>,

    pub msgs: Vec<Message>,

    pub leader_id: u64,

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
            raft_log,
            max_inflight: cfg.max_inflight_msgs,
            max_msg_size: cfg.max_size_per_msg,
            prs: Some(ProgressSet::with_capacity(peers.len(), learners.len())),
            is_learner: false,
            votes: Default::default(),
            msgs: Default::default(),
            leader_id: Default::default(),
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
            raft.raft_log.applied_to(cfg.applied);
        }
        let term = raft.term;
        raft.become_follower(term, INVALID_ID);
        info!(
            "{} new raft [peers: {:?}, term: {:?}, commit: {}, applied: {}, last_index: {}, last_term: {}]",
            raft.tag,
            raft.prs().voters().collect::<Vec<_>>(),
            raft.term,
            raft.raft_log.committed,
            raft.raft_log.get_applied(),
            raft.raft_log.last_index(),
            raft.raft_log.last_term()
        );

        Ok(())
    }

    #[inline]
    pub fn get_store(&self) -> &T {
        self.raft_log.get_store()
    }

    #[inline]
    pub fn mut_store(&mut self) -> &mut T {
        self.raft_log.mut_store()
    }

    #[inline]
    pub fn get_snap(&self) -> Option<&Snapshot> {
        self.raft_log.get_unstable().snapshot.as_ref()
    }

    #[inline]
    pub fn pending_read_count(&self) -> usize {
        self.read_only.pending_read_count()
    }

    #[inline]
    pub fn ready_read_count(&self) -> usize {
        self.read_state.len()
    }

    pub fn soft_state(&self) -> SoftState {
        SoftState {
            leader_id: self.leader_id,
            raft_state: self.state,
        }
    }

    pub fn hard_state(&self) -> HardState {
        let mut hs = HardState::new();
        hs.set_term(self.term);
        hs.set_vote(self.vote);
        hs.set_commit(self.raft_log.committed);

        hs
    }

    pub fn in_lease(&self) -> bool {
        self.state == StateRole::Leader && self.check_quorum
    }

    pub fn set_randomized_election_timeout(&mut self, timeout: usize) {
        assert!(self.min_election_timeout <= t && t < self.max_election_timeout);
        self.randomized_election_timeout = timeout;
    }

    pub fn get_election_timeout(&self) -> usize {
        self.election_timeout
    }

    pub fn get_heartbeat_timeout(&self) -> usize {
        self.heartbeat_timeout
    }

    pub fn get_randomized_timeout(&self) -> usize {
        self.randomized_election_timeout
    }

    #[inline]
    pub fn skip_bcast_commit(&mut self, skip: bool) {
        self.skip_bcast_commit = skip;
    }

    fn send(&mut self, mut msg: Message) {
        msg.set_from(self.id);
        if msg.get_msg_type() == MessageType::MsgRequestVote
            || msg.get_msg_type() == MessageType::MsgRequestPreVote
            || msg.get_msg_type() == MessageType::MsgRequestVoteResponse
            || msg.get_msg_type() == MessageType::MsgRequestPreVoteResponse
        {
            if msg.get_term() == 0 {
                panic!(
                    "{} term should be set when sending {:?}",
                    self.tag,
                    msg.get_msg_type()
                );
            }
        } else {
            if msg.get_term() == 0 {
                panic!(
                    "{} term should be set when sending {:?} (was {})",
                    self.tag,
                    msg.get_msg_type(),
                    msg.get_term()
                );
            }

            if msg.get_msg_type() != MessageType::MsgPropose
                && msg.get_msg_type() != MessageType::MsgReadIndex
            {
                msg.set_term(self.term);
            }
        }

        self.msgs.push(msg);
    }

    pub fn prepare_send_snapshot(&mut self, msg: &mut Message, pr: &mut Progress, to: u64) -> bool {
        if !pr.recent_active {
            debug!(
                "{} ignore sending snapshot to {} since it is not recently active",
                self.tag, to
            );
            return false;
        }

        msg.set_msg_type(MessageType::MsgSnapshot);
        let snapshot_r = self.raft_log.snapshot();
        if let Err(e) = snapshot_r {
            if e == Error::Store(StorageError::SnapshotTemporarilyUnavailable) {
                debug!(
                    "{} failed to send snapshot to {} because snapshot is temporarily unavailable",
                    self.tag, to
                );
                return false;
            }
            panic!("{} unexpected error: {:?}", self.tag, e);
        }

        let snapshot = snapshot_r.unwrap();
        if snapshot.get_metadata().get_index() == 0 {
            panic!("{} need non-empty snapshot", self.tag);
        }
        let (s_index, s_term) = (
            snapshot.get_metadata().get_index(),
            snapshot.get_metadata().get_term(),
        );
        msg.set_snapshot(snapshot);
        debug!(
            "{} [first_index: {}, commit: {}] send snapshot[index: {}, term: {}] to {} [{:?}]",
            self.tag,
            self.raft_log.first_index(),
            self.raft_log.committed,
            s_index,
            s_term,
            to,
            pr
        );

        pr.become_snapshot(s_index);
        debug!(
            "{} paused sending replication message to {} [{:?}]",
            self.tag, to, pr
        );

        true
    }

    fn prepare_send_entries(
        &mut self,
        msg: &mut Message,
        pr: &mut Progress,
        term: u64,
        ents: Vec<Entry>,
    ) {
        msg.set_msg_type(MessageType::MsgAppend);
        msg.set_index(pr.next_idx - 1);
        msg.set_log_term(term);
        msg.set_entries(RepeatedField::from_vec(ents));
        msg.set_commit(self.raft_log.committed);
        if !msg.get_entries().is_empty() {
            match pr.state {
                ProgressState::Replicate => {
                    let last = msg.get_entries().last().unwrap().get_index();
                    pr.optimistic_update(last);
                    pr.ins.add(last);
                }
                ProgressState::Probe => pr.pause(),
                _ => panic!(
                    "{} is sending append in unhandle state {:?}",
                    self.tag, pr.state
                ),
            }
        }
    }

    pub fn send_append(&mut self, to: u64, pr: &mut Progress) {
        if pr.is_paused() {
            return;
        }
        let term = self.raft_log.term(pr.next_idx - 1);
        let ents = self.raft_log.entries(pr.next_idx, self.max_msg_size);
        let mut msg = Message::new();
        msg.set_to(to);
        if term.is_err() || ents.is_err() {
            if !self.prepare_send_snapshot(&mut msg, pr, to) {
                return;
            }
        } else {
            self.prepare_send_entries(&mut msg, pr, term.unwrap(), ents.unwrap());
        }
        self.send(msg);
    }

    fn send_heartbeat(&mut self, to: u64, pr: &Progress, ctx: Option<Vec<u8>>) {
        let mut msg = Message::new();
        msg.set_to(to);
        msg.set_msg_type(MessageType::MsgHeartbeat);
        let commit = cmp::min(pr.matched, self.raft_log.committed);
        msg.set_commit(commit);
        if let Some(context) = ctx {
            msg.set_context(context);
        }
        self.send(msg);
    }

    pub fn bcast_append(&mut self) {
        let self_id = self.id;
        let mut prs = self.take_prs();
        prs.iter_mut()
            .filter(|&(id, _)| *id != self_id)
            .for_each(|(id, pr)| self.send_append(*id, pr));
        self.set_prs(prs);
    }

    pub fn bcast_heartbeat(&mut self) {
        let ctx = self.read_only.last_pending_request_ctx();
        self.bcast_heartbeat_with_ctx(ctx);
    }

    fn bcast_heartbeat_with_ctx(&mut self, ctx: Option<Vec<u8>>) {
        let self_id = self.id;
        let mut prs = self.take_prs();
        prs.iter_mut()
            .filter(|(&id, _)| *id != self_id)
            .for_each(|(id, pr)| self.send_heartbeat(*id, pr, ctx.clone()));
        self.set_prs(prs);
    }

    pub fn maybe_commit(&mut self) -> bool {
        let mci = self.prs().maximal_committed_index();
        self.raft_log.maybe_commit(mci, self.term)
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

        self.abort_leader_transfer();

        self.votes.clear();

        self.pending_conf_index = 0;
        self.read_only = ReadOnly::new(self.read_only.option);

        let last_index = self.raft_log.last_index();
        let self_id = self.id;
        for (&id, progress) in self.mut_prs().iter_mut() {
            progress.reset(last_index + 1);
            if id == self_id {
                progress.matched = last_index;
            }
        }
    }

    pub fn append_entry(&mut self, ents: &mut [Entry]) {
        let mut li = self.raft_log.last_index();
        for (i, e) in ents.iter_mut().enumerate() {
            e.set_term(self.term);
            e.set_index(li + 1 + i as u64);
        }
        li = self.raft_log.append(ents);

        let self_id = self.id;
        self.mut_prs().get_mut(self_id).unwrap().maybe_update(li);

        self.maybe_commit();
    }

    pub fn tick(&mut self) -> bool {
        match self.state {
            StateRole::FOLLOWER | StateRole::Candidate => self.to,
        }
    }

    pub fn tick_election(&mut self) -> bool {
        self.election_elapsed += 1;
        if !self.pass_election_timeout() || !self.promotable() {
            return false;
        }

        self.election_elapsed = 0;
        let msg = new_message(INVALID_ID, MessageType::MsgHup, Some(self.id));
        //        self.step();
        true
    }

    pub fn campaign(&mut self, campaign_type: &[u8]) {
        unimplemented!()
    }

    pub fn become_follower(&mut self, term: u64, leader_id: u64) {
        self.reset(term);
        self.leader_id = leader_id;
        self.state = StateRole::FOLLOWER;
        info!("{} become follower at term {}", self.tag, self.term);
    }

    pub fn become_leader(&mut self) {
        unimplemented!();
    }

    fn num_pending_conf(&self, ents: &[Entry]) -> usize {
        ents.into_iter()
            .filter(|e| e.get_entry_type() == EntryType::EntryConfChange)
            .count()
    }

    fn register_vote(&mut self, id: u64, vote: bool) {
        self.votes.entry(id).or_insert(vote);
    }

    pub fn step(&mut self, msg: Message) -> Result<()> {
        if msg.get_term() == 0 {
            // local message
        } else if msg.get_term() > self.term {
            if msg.get_msg_type() == MessageType::MsgRequestVote
                || msg.get_msg_type() == MessageType::MsgRequestPreVote
            {
                let force = msg.get_context() == CAMPAIGN_TRANSFER;
                let in_lease = self.check_quorum
                    && self.leader_id != INVALID_ID
                    && self.election_elapsed < self.election_timeout;
                if !force && in_lease {
                    info!(
                        "{} [log_term: {}, index: {}, vote: {}] ignored {:?} vote from \
                         {} [log_term: {}, index: {}] at term {}: lease is not expired \
                         (remaining ticks: {})",
                        self.tag,
                        self.raft_log.last_term(),
                        self.raft_log.last_index(),
                        self.vote,
                        msg.get_msg_type(),
                        msg.get_from(),
                        msg.get_log_term(),
                        msg.get_index(),
                        self.term,
                        self.election_timeout - self.election_elapsed
                    );
                }

                return Ok(());
            }

            if msg.get_msg_type() == MessageType::MsgRequestPreVote
                || (msg.get_msg_type() == MessageType::MsgRequestPreVoteResponse
                    && !msg.get_reject())
            {
                // do not support this
            } else {
                info!(
                    "{} [term: {}] received a {:?} message with higher term form {} [term: {}]",
                    self.tag,
                    self.term,
                    msg.get_msg_type(),
                    msg.get_from(),
                    msg.get_term()
                );
                if msg.get_msg_type() == MessageType::MsgAppend
                    || msg.get_msg_type() == MessageType::MsgHeartbeat
                    || msg.get_msg_type() == MessageType::MsgSnapshot
                {
                    self.become_follower(msg.get_term(), msg.get_from());
                } else {
                    self.become_follower(msg.get_term(), INVALID_ID);
                }
            }
        } else if msg.get_term() < self.term {
            if (self.check_quorum || self.pre_vote)
                && (msg.get_msg_type() == MessageType::MsgHeartbeat
                    || msg.get_msg_type() == MessageType::MsgAppend)
            {
                let to_send = new_message(msg.get_from(), MessageType::MsgAppendResponse, None);
                self.send(to_send);
            } else if msg.get_msg_type() == MessageType::MsgRequestPreVote {
                info!(
                    "{} [log_term: {}, index: {}, vote: {}] rejected {:?} from {} [log_term: {}, index: {}] at term {}",
                    self.id,
                    self.raft_log.last_term(),
                    self.raft_log.last_index(),
                    self.vote,
                    msg.get_msg_type(),
                    msg.get_from(),
                    msg.get_log_term(),
                    msg.get_index(),
                    self.term,
                );

                let mut to_send =
                    new_message(msg.get_from(), MessageType::MsgRequestPreVoteResponse, None);
                to_send.set_term(self.term);
                to_send.set_reject(true);
                self.send(to_send);
            } else {
                info!(
                    "{} [term: {}] ignored a {:?} message with lower term from {} [term: {}]",
                    self.tag,
                    self.term,
                    msg.get_msg_type(),
                    msg.get_from(),
                    msg.get_term(),
                );
            }
            return Ok(());
        }

        #[cfg(feature = "failpoint")]
        fail_point!("before_step");

        match msg.get_msg_type() {
            MessageType::MsgHup => {
                if self.state != StateRole::Leader {
                    let ents = self
                        .raft_log
                        .slice(
                            self.raft_log.applied + 1,
                            self.raft_log.committed + 1,
                            raft_log::NO_LIMIT,
                        )
                        .expect("unexpected error getting un-applied entries");

                    let n = self.num_pending_conf(&ents);
                    if n != 0 && self.raft_log.committed > self.raft_log.applied {
                        warn!(
                            "{} cannot campaign at term {} since there are still {} pending configuration changes to apply",
                            self.tag, self.term, n,
                        );
                        return Ok(());
                    }
                    info!(
                        "{} is starting a new election at term {}",
                        self.tag, self.term,
                    );
                    if self.pre_vote {
                        self.campaign(CAMPAIGN_PRE_ELECTION);
                    } else {
                        self.campaign(CAMPAIGN_ELECTION);
                    }
                } else {
                    debug!("{} ignoring MsgHup because already leader", self.tag);
                }
            }

            MessageType::MsgRequestVote | MessageType::MsgRequestPreVote => {
                let can_vote = (self.vote == msg.get_from())
                    || (self.vote == INVALID_ID && self.leader_id == INVALID_ID)
                    || (msg.get_msg_type() == MessageType::MsgRequestPreVote
                        && msg.get_term() > self.term);

                if can_vote
                    && self
                        .raft_log
                        .is_up_to_date(msg.get_index(), msg.get_log_term())
                {
                    self.log_vote_approve(&msg);
                    let mut to_send =
                        new_message(msg.get_from(), vote_resp_msg_type(msg.get_msg_type()), None);
                    to_send.set_reject(false);
                    to_send.set_term(msg.get_term());
                    self.send(to_send);
                    if msg.get_msg_type() == MessageType::MsgRequestVote {
                        self.election_elapsed = 0;
                        self.vote = msg.get_from();
                    }
                } else {
                    self.log_vote_reject(&msg);
                    let mut to_send =
                        new_message(msg.get_from(), vote_resp_msg_type(msg.get_msg_type()), None);
                    to_send.set_reject(true);
                    to_send.set_term(self.term);
                    self.send(to_send);
                }
            }

            _ => match self.state {
                StateRole::PreCandidate | StateRole::Candidate => self.step_candidate(msg)?,
                StateRole::FOLLOWER => self.step_follower(msg)?,
                StateRole::Leader => self.step_leader(msg)?,
            },
        }

        Ok(())
    }

    fn log_vote_approve(&self, msg: &Message) {
        info!(
            "{} [log_term: {}, index: {}, vote: {}] cast {:?} for {} [log_term: {}, index: {}] at term {}",
            self.tag,
            self.raft_log.last_term(),
            self.raft_log.last_index(),
            self.vote,
            msg.get_msg_type(),
            msg.get_from(),
            msg.get_log_term(),
            msg.get_index(),
            self.term
        );
    }

    fn log_vote_reject(&self, msg: &Message) {
        info!(
            "{} [log_term: {}, index: {}, vote: {}] rejected {:?} from {} [log_term: {}, index: {}] at term {}",
            self.tag,
            self.raft_log.last_term(),
            self.raft_log.last_index(),
            self.vote,
            msg.get_msg_type(),
            msg.get_from(),
            msg.get_log_term(),
            msg.get_index(),
            self.term
        );
    }

    fn step_leader(&mut self, mut msg: Message) -> Result<()> {
        unimplemented!()
    }

    fn step_candidate(&mut self, msg: Message) -> Result<()> {
        match msg.get_msg_type() {
            MessageType::MsgPropose => {
                info!(
                    "{} no leader at term {}; dropping proposal",
                    self.tag, self.term
                );
                return Err(Error::ProposalDropped);
            }
            MessageType::MsgAppend => {
                debug_assert_eq!(self.term, msg.get_term());
                self.become_follower(msg.get_term(), msg.get_from());
                self.handler_append_entries(&msg);
            }
            MessageType::MsgHeartbeat => {
                debug_assert_eq!(self.term, msg.get_term());
                self.become_follower(msg.get_term(), msg.get_from());
                self.handler_heartbeat(msg);
            }
            MessageType::MsgSnapshot => {
                debug_assert_eq!(self.term, msg.get_term());
                self.become_follower(self.term, msg.get_term());
                self.handler_snapshot(msg);
            }
            MessageType::MsgRequestPreVoteResponse | MessageType::MsgRequestVoteResponse => {
                if (self.state == StateRole::PreCandidate
                    && msg.get_msg_type() != MessageType::MsgRequestPreVoteResponse)
                    || (self.state == StateRole::Candidate
                        && msg.get_msg_type() != MessageType::MsgRequestVoteResponse)
                {
                    return Ok(());
                }

                let acceptance = !msg.get_reject();
                let msg_type = msg.get_msg_type();
                let from_id = msg.get_from();
                info!(
                    "{} received {:?}{} from {} at term {}",
                    self.id,
                    msg_type,
                    if !acceptance { " rejection " } else { "" },
                    from_id,
                    self.term,
                );
                self.register_vote(from_id, acceptance);
                match self.prs().candidacy_status(self.id, &self.votes) {
                    CandidacyStatus::Elected => {
                        if self.state == StateRole::PreCandidate {
                            self.campaign(CAMPAIGN_ELECTION);
                        } else {
                            self.become_leader();
                            self.bcast_append();
                        }
                    }
                    CandidacyStatus::Ineligible => {
                        let term = self.term;
                        self.become_follower(term, INVALID_ID);
                    }
                    CandidacyStatus::Eligible => {}
                }
            }
            MessageType::MsgTimeoutNow => debug!(
                "{} [term {} state {:?}] ignored MsgTimeoutNow from {}",
                self.tag,
                self.term,
                self.state,
                msg.get_from(),
            ),
            _ => {}
        }

        Ok(())
    }

    fn step_follower(&mut self, mut msg: Message) -> Result<()> {
        unimplemented!()
    }

    pub fn handler_append_entries(&mut self, msg: &Message) {
        if msg.get_index() < self.raft_log.committed {
            let mut to_send = Message::new();
            to_send.set_to(msg.get_from());
            to_send.set_msg_type(MessageType::MsgAppendResponse);
            to_send.set_index(self.raft_log.committed);
            self.send(to_send);
            return;
        }

        let mut to_send = Message::new();
        to_send.set_to(msg.get_from());
        to_send.set_msg_type(MessageType::MsgAppendResponse);
        match self.raft_log.maybe_append(
            msg.get_index(),
            msg.get_term(),
            msg.get_commit(),
            msg.get_entries(),
        ) {
            Some(msg_last_index) => {
                to_send.set_index(msg_last_index);
                self.send(to_send);
            }
            None => {
                debug!(
                    "{} [log_term: {}, index: {}] rejected msgApp [log_term: {}, index: {}] from {}",
                    self.tag,
                    self.raft_log.term(m.get_index()).unwrap_or(0),
                    m.get_index(),
                    m.get_log_term(),
                    m.get_index(),
                    m.get_from()
                );
                to_send.set_index(m.get_index());
                to_send.set_reject(true);
                to_send.set_reject_hint(self.raft_log.last_index());
                self.send(to_send);
            }
        }
    }

    pub fn handler_heartbeat(&mut self, mut msg: Message) {
        self.raft_log.commit_to(msg.get_commit());
        let mut to_send = Message::new();
        to_send.set_to(msg.get_from());
        to_send.set_msg_type(MessageType::MsgHeartbeatResponse);
        to_send.set_context(msg.take_context());
        self.send(to_send);
    }

    fn handler_snapshot(&mut self, mut msg: Message) {
        let (s_index, s_term) = (
            msg.get_snapshot().get_metadata().get_index(),
            msg.get_snapshot().get_metadata().get_term(),
        );
        if self.restore(msg.take_snapshot()) {
            info!(
                "{} [commit: {}] restored snapshot [index: {}, term: {}]",
                self.tag, self.raft_log.committed, s_index, s_term,
            );
            let mut to_send = Message::new();
            to_send.set_to(msg.get_from());
            to_send.set_msg_type(MessageType::MsgAppendResponse);
            to_send.set_index(self.raft_log.last_index());
            self.send(to_send);
        } else {
            info!(
                "{} [commit: {}] ignored snapshot [index: {}, term: {}]",
                self.tag, self.raft_log.committed, s_index, s_term,
            );
            let mut to_send = Message::new();
            to_send.set_to(msg.get_from());
            to_send.set_msg_type(MessageType::MsgAppendResponse);
            to_send.set_index(self.raft_log.committed);
            self.send(to_send);
        }
    }

    pub fn restore_raft(&mut self, snap: &Snapshot) -> Option<bool> {
        let meta = snap.get_metadata();
        if self.raft_log.match_term(meta.get_index(), meta.get_term()) {
            info!(
                "{} [commit: {}, last_index: {}, last_term: {}] fast-forwarded commit to snapshot [index: {}, term: {}]",
                self.tag,
                self.raft_log.committed,
                self.raft_log.last_index(),
                self.raft_log.last_term(),
                meta.get_index(),
                meta.get_term()
            );
            self.raft_log.commit_to(meta.get_index());
            return Some(false);
        }

        if self.prs().iter().len() != 0 && !self.is_learner {
            for &id in meta.get_conf_state().get_learners() {
                if id == self.id {
                    error!(
                        "{} can not become learner when restores snapshot [index: {}, term: {}]",
                        self.tag,
                        meta.get_index(),
                        meta.get_term(),
                    );
                    return Some(false);
                }
            }
        }

        info!(
            "{} [commit: {}, last_index: {}, last_term: {}] starts to restore snapshot [index: {}, term: {}]",
            self.tag,
            self.raft_log.committed,
            self.raft_log.last_index(),
            self.raft_log.last_term(),
            meta.get_index(),
            meta.get_term()
        );

        let nodes = meta.get_conf_state().get_nodes();
        let learners = meta.get_conf_state().get_learners();
        self.prs = Some(ProgressSet::with_capacity(nodes.len(), learners.len()));

        for &(is_learner, nodes) in &[(false, nodes), (true, learners)] {
            for &n in nodes {
                let next_index = self.raft_log.last_index() + 1;
                let mut matched = 0;
                if n == self.id {
                    matched = next_index - 1;
                    self.is_learner = is_learner;
                }
                self.set_progress(n, matched, next_index, is_learner);
                info!(
                    "{} restored progress of {} [{:?}]",
                    self.tag,
                    n,
                    self.prs().get(n),
                );
            }
        }

        None
    }

    pub fn restore(&mut self, snap: Snapshot) -> bool {
        if snap.get_metadata().get_index() < self.raft_log.committed {
            return false;
        }

        if let Some(b) = self.restore_raft(&snap) {
            return b;
        }

        self.raft_log.restore(snap);
        true
    }

    pub fn set_progress(&mut self, id: u64, matched: u64, next_idx: u64, is_learner: bool) {
        let mut pr = Progress::new(next_idx, self.max_inflight);
        pr.matched = matched;
        if is_learner {
            if let Err(e) = self.mut_prs().insert_learner(id, pr) {
                panic!("{}", e);
            }
        } else if let Err(e) = self.mut_prs().insert_voter(id, pr) {
            panic!("{}", e);
        }
    }

    pub fn promotable(&self) -> bool {
        self.prs().voter_ids().contains(&self.id)
    }

    pub fn take_prs(&mut self) -> ProgressSet {
        self.prs.take().unwrap()
    }

    pub fn set_prs(&mut self, prs: ProgressSet) {
        self.prs = Some(prs);
    }

    pub fn prs(&self) -> &ProgressSet {
        self.prs.as_ref().unwrap()
    }

    pub fn mut_prs(&mut self) -> &mut ProgressSet {
        self.prs.as_mut().unwrap()
    }

    pub fn load_state(&mut self, hs: &HardState) {
        if hs.get_commit() < self.raft_log.committed || hs.get_commit() > self.raft_log.last_index()
        {
            panic!(
                "{} hs.commit {} is out of range [{}, {}]",
                self.tag,
                hs.get_commit(),
                self.raft_log.committed,
                self.raft_log.last_index(),
            );
        }
        self.raft_log.committed = hs.get_commit();
        self.term = hs.get_term();
        self.vote = hs.get_vote();
    }

    pub fn pass_election_timeout(&self) -> bool {
        self.election_elapsed >= self.randomized_election_timeout
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

    pub fn abort_leader_transfer(&mut self) {
        self.lead_transferee = None
    }
}
