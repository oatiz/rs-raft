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
    pub fn voter_ids(&self) -> &FxHashSet<u64> {
        &self.configuration.voters
    }

    #[inline]
    pub fn learner_ids(&self) -> &FxHashSet<u64> {
        &self.configuration.learners
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
