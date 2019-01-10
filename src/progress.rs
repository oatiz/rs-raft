use std::cell::RefCell;
use std::cmp;
use std::collections::{HashMap, HashSet};

use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};

use errors::Error;

#[inline]
fn majority(total: usize) -> usize {
    (total / 2) + 1
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProgressState {
    Probe,
    Replicate,
    Snapshot,
}

impl Default for ProgressState {
    fn default() -> ProgressState {
        ProgressState::Probe
    }
}

#[derive(Clone, Debug, Default)]
struct Configuration {
    voters: FxHashSet<u64>,
    learners: FxHashSet<u64>,
}

#[derive(Copy, Clone, Debug)]
pub enum CandidacyStatus {
    Elected,
    Eligible,
    Ineligible,
}

#[derive(Default, Clone)]
pub struct ProgressSet {
    progress: FxHashMap<u64, Progress>,
    configuration: Configuration,
    sort_buffer: RefCell<Vec<u64>>,
}

impl ProgressSet {
    pub fn new() -> ProgressSet {
        ProgressSet {
            progress: Default::default(),
            configuration: Default::default(),
            sort_buffer: Default::default(),
        }
    }

    pub fn with_capacity(voters: usize, learners: usize) -> ProgressSet {
        ProgressSet {
            progress: HashMap::with_capacity_and_hasher(
                voters + learners,
                FxBuildHasher::default(),
            ),
            configuration: Configuration {
                voters: HashSet::with_capacity_and_hasher(voters, FxBuildHasher::default()),
                learners: HashSet::with_capacity_and_hasher(learners, FxBuildHasher::default()),
            },
            sort_buffer: Default::default(),
        }
    }

    #[inline]
    pub fn voters(&self) -> impl Iterator<Item = (&u64, &Progress)> {
        let voter_set = self.voter_ids();
        self.progress
            .iter()
            .filter(move |(&k, _)| voter_set.contains(&k))
    }

    #[inline]
    pub fn learners(&self) -> impl Iterator<Item = (&u64, &Progress)> {
        let learner_set = self.learner_ids();
        self.progress
            .iter()
            .filter(move |(&k, _)| learner_set.contains(&k))
    }

    #[inline]
    pub fn votes_mut(&mut self) -> impl Iterator<Item = (&u64, &mut Progress)> {
        let vote_ids = &self.configuration.voters;
        self.progress
            .iter_mut()
            .filter(move |(k, _)| vote_ids.contains(k))
    }

    #[inline]
    pub fn learners_mut(&mut self) -> impl Iterator<Item = (&u64, &mut Progress)> {
        let learner_ids = &self.configuration.learners;
        self.progress
            .iter_mut()
            .filter(move |(k, _)| learner_ids.contains(k))
    }

    #[inline]
    pub fn voter_ids(&self) -> &FxHashSet<u64> {
        &self.configuration.voters
    }

    #[inline]
    pub fn learner_ids(&self) -> &FxHashSet<u64> {
        &self.configuration.learners
    }

    #[inline]
    pub fn get(&self, id: u64) -> Option<&Progress> {
        self.progress.get(&id)
    }

    #[inline]
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Progress> {
        self.progress.get_mut(&id)
    }

    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (u64, &Progress)> {
        self.progress.iter()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (u64, &mut Progress)> {
        self.progress.iter_mut()
    }

    pub fn insert_voter(&mut self, id: u64, progress: Progress) -> Result<(), Error> {
        if self.progress.contains_key(&id) {
            if self.learner_ids().contains(&id) {
                return Err(Error::Exists(id, "learners"));
            }
            return Err(Error::Exists(id, "voters"));
        }
        self.configuration.voters.insert(id);
        self.progress.insert(id, progress);
        self.assert_progress_and_configuration_consistent();

        Ok(())
    }

    pub fn insert_learner(&mut self, id: u64, progress: Progress) -> Result<(), Error> {
        if self.progress.contains_key(&id) {
            if self.learner_ids().contains(&id) {
                return Err(Error::Exist(id, "learners"));
            }
            return Err(Error::Exist(id, "votes"));
        }
        self.configuration.learners.insert(id);
        self.progress.insert(id, progress);
        self.assert_progress_and_configuration_consistent();

        Ok(())
    }

    pub fn remove(&mut self, id: u64) -> Option<Progress> {
        self.configuration.voters.remove(&id);
        self.configuration.learners.remove(&id);
        let removed = self.progress.remove(&id);
        self.assert_progress_and_configuration_consistent();

        removed
    }

    pub fn promote_learner(&mut self, id: u64) -> Result<(), Error> {
        if !self.configuration.learners.remove(&id) {
            return Err(Error::NotExists(id, "learners"));
        }
        if !self.configuration.voters.remove(&id) {
            return Err(Error::NotExists(id, "voters"));
        }
        self.assert_progress_and_configuration_consistent();

        Ok(())
    }

    #[inline(always)]
    fn assert_progress_and_configuration_consistent(&self) {
        debug_assert!(self
            .configuration
            .voters
            .union(&self.configuration.learners)
            .all(|v| self.progress.contains_key(v)));
        debug_assert!(self
            .progress
            .keys()
            .all(|v| self.configuration.learners.contains(v)
                || self.configuration.voters.contains(v)));
        assert_eq!(
            self.configuration.voters.len() + self.configuration.learners.len(),
            self.progress.len()
        );
    }

    pub fn maximal_committed_index(&self) -> u64 {
        let mut matched = self.sort_buffer.borrow_mut();
        matched.clear();
        self.voters().for_each(|(_id, peer)| {
            matched.push(peer.matched);
        });
        matched.sort_by(|a, b| b.cmp(a));

        matched[matched.len() / 2]
    }

    pub fn candidacy_status<'a>(
        &self,
        id: u64,
        votes: impl IntoIterator<Item = (&'a u64, &'a bool)>,
    ) -> CandidacyStatus {
        let (accepted, total) =
            votes
                .into_iter()
                .fold((0, 0), |(mut accepted, mut total), (_, nominated)| {
                    if *nominated {
                        accepted += 1;
                    }
                    total += 1;
                    (accepted, total)
                });
        let quorum = majority(self.voter_ids().len());
        let rejected = total - accepted;

        info!(
            "{} [quorum: {}] has received {} votes and {} votes rejections",
            id, quorum, accepted, rejected
        );

        if accepted >= quorum {
            CandidacyStatus::Elected
        } else if rejected == quorum {
            CandidacyStatus::Eligible
        } else {
            CandidacyStatus::Ineligible
        }
    }

    pub fn quorum_recently_active(&mut self, perspective_of: u64) -> bool {
        let mut active = 0;
        for (&id, progress) in self.votes_mut() {
            if id == perspective_of {
                active += 1;
                continue;
            }
            if progress.recent_active {
                active += 1;
            }

            progress.recent_active = false;
        }
        for (&id, progress) in self.learners_mut() {
            progress.recent_active = false;
        }

        active >= majority(self.voter_ids().len())
    }

    pub fn has_quorum(&self, potential_quorum: &FxHashSet<u64>) -> bool {
        potential_quorum.len() >= majority(self.voter_ids().len())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub matched: u64,
    pub next_idx: u64,
    pub state: ProgressState,
    pub paused: bool,
    pub pending_snapshot: u64,
    pub recent_active: bool,
    pub ins: Inflights,
}

impl Progress {
    pub fn new(next_idx: u64, ins_size: usize) -> Progress {
        Progress {
            matched: 0,
            next_idx: 0,
            state: ProgressState::Probe,
            paused: false,
            pending_snapshot: 0,
            recent_active: false,
            ins: Inflights::new(ins_size),
        }
    }

    pub fn reset_state(&mut self, state: ProgressState) {
        self.paused = false;
        self.pending_snapshot = 0;
        self.state = state;
        self.ins.reset();
    }

    pub(crate) fn reset(&mut self, next_idx: u64) {
        self.matched = 0;
        self.next_idx = next_idx;
        self.state = ProgressState::default();
        self.paused = false;
        self.pending_snapshot = 0;
        self.recent_active = false;
        debug_assert!(self.ins.cap() != 0);
        self.ins.reset();
    }

    pub fn become_probe(&mut self) {
        if self.state == ProgressState::Snapshot {
            let pending_snapshot = self.pending_snapshot;
            self.reset_state(ProgressState::Probe);
            self.next_idx = cmp::max(self.matched + 1, pending_snapshot + 1);
        } else {
            self.reset_state(ProgressState::Probe);
            self.next_idx = self.matched + 1;
        }
    }

    pub fn become_replicate(&mut self) {
        self.reset_state(ProgressState::Replicate);
        self.next_idx = self.matched + 1;
    }

    pub fn become_snapshot(&mut self, snapshot_idx: u64) {
        self.reset_state(ProgressState::Snapshot);
        self.next_idx = snapshot_idx;
    }

    pub fn snapshot_failure(&mut self) {
        self.pending_snapshot = 0;
    }

    pub fn maybe_snapshot_abort(&self) -> bool {
        self.state == ProgressState::Snapshot && self.matched >= self.pending_snapshot
    }

    pub fn maybe_update(&mut self, n: u64) -> bool {
        let need_update = self.matched < n;
        if need_update {
            self.matched = n;
            self.resume();
        }

        if self.next_idx < n + 1 {
            self.next_idx = n + 1;
        }

        need_update
    }

    pub fn optimistic_update(&mut self, n: u64) {
        self.next_idx = n + 1;
    }

    pub fn maybe_decr_to(&mut self, rejected: u64, last: u64) -> bool {
        if self.state == ProgressState::Replicate {
            if rejected <= self.matched {
                return false;
            }
            self.next_idx = self.matched + 1;
            return true;
        }

        if self.next_idx == 0 || self.next_idx - 1 != rejected {
            return false;
        }

        self.next_idx = cmp::min(rejected, last + 1);
        if self.next_idx < 1 {
            self.next_idx = 1;
        }
        self.resume();

        true
    }

    pub fn is_paused(&self) -> bool {
        match self.state {
            ProgressState::Probe => self.paused,
            ProgressState::Replicate => self.ins.full(),
            ProgressState::Snapshot => true,
        }
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }
}

pub struct Inflights {
    start: usize,
    count: usize,

    buffer: Vec<u64>,
}

impl Inflights {
    pub fn new(cap: usize) -> Inflights {
        Inflights {
            start: 0,
            count: 0,
            buffer: Vec::with_capacity(cap),
        }
    }

    pub fn full(&self) -> bool {
        self.count == self.cap()
    }

    pub fn cap(&self) -> usize {
        self.buffer.capacity()
    }

    pub fn add(&mut self, inflight: u64) {
        if self.full() {
            panic!("cannot add into a full inflights");
        }

        let mut next = self.start + self.count;
        if next >= self.cap() {
            next -= self.cap();
        }
        assert!(next <= self.buffer.len());

        if next == self.buffer.len() {
            self.buffer.push(inflight);
        } else {
            self.buffer[next] = inflight;
        }

        self.count += 1;
    }

    pub fn free_to(&mut self, to: u64) {
        if self.count == 0 || to < self.buffer[self.start] {
            return;
        }

        let mut i = 0usize;
        let mut idx = self.start;
        while i < self.count {
            if to < self.buffer[idx] {
                break;
            }

            idx += 1;
            if idx >= self.cap() {
                idx -= self.cap();
            }

            i += 1
        }

        self.count -= i;
        self.start = idx;
    }

    pub fn free_first_one(&mut self) {
        let start = self.buffer[self.start];
        self.free_to(start);
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.start = 0;
    }
}
