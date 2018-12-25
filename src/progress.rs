use errors::Error;
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::cmp;
use std::collections::{HashMap, HashSet};

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
            progress: HashMap::with_capacity_and_hasher(votes + learners, FxBuildHasher::default()),
            configuration: Configuration {
                voters: HashSet::with_capacity_and_hasher(votes, FxBuildHasher::default()),
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

        }

        Ok(())
    }

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

pub struct Inflights {
    start: usize,
    count: usize,

    buffer: Vec<u64>,
}
