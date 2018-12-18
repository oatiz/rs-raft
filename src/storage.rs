use eraftpb::{ConfState, Entry, HardState, Snapshot};
use errors::{Error, Result, StorageError};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use util;

#[derive(Debug, Clone)]
pub struct RaftState {
    pub hard_state: HardState,

    pub conf_state: ConfState,
}

pub trait Storage {
    fn initial_state(&self) -> Result<RaftState>;

    fn entries(&self, low: u64, high: u64, max_size: u64) -> Result<Vec<Entry>>;

    fn term(&self, idx: u64) -> Result<u64>;

    fn first_index(&self) -> Result<u64>;

    fn last_index(&self) -> Result<u64>;

    fn snapshot(&self) -> Result<Snapshot>;
}

pub struct MemStorageCore {
    hard_state: HardState,
    snapshot: Snapshot,
    entries: Vec<Entry>,
}

impl Default for MemStorageCore {
    fn default() -> MemStorageCore {
        MemStorageCore {
            hard_state: HardState::new(),
            snapshot: Snapshot::new(),
            entries: vec![Entry::new()],
        }
    }
}

impl MemStorageCore {
    pub fn set_hard_state(&mut self, new_hard_state: HardState) {
        self.hard_state = new_hard_state;
    }

    fn inner_last_index(&self) -> u64 {
        self.entries[0].get_index() + self.entries.len() as u64 - 1
    }

    pub fn apply_snapshot(&mut self, new_snapshot: Snapshot) -> Result<()> {
        // 与老版本对比快照
        let old_snapshot_index = self.snapshot.get_metadata().get_index();
        let new_snapshot_index = new_snapshot.get_metadata().get_index();

        if old_snapshot_index >= new_snapshot_index {
            return Err(Error::Store(StorageError::SnapshotOutOfDate));
        }

        let mut e = Entry::new();
        e.set_term(new_snapshot.get_metadata().get_term());
        e.set_index(new_snapshot.get_metadata().get_index());
        self.entries = vec![e];
        self.snapshot = new_snapshot;

        Ok(())
    }

    pub fn create_snapshot(
        &mut self,
        idx: u64,
        conf_state: Option<ConfState>,
        data: Vec<u8>,
    ) -> Result<&Snapshot> {
        if idx <= self.snapshot.get_metadata().get_index() {
            return Err(Error::Store(StorageError::SnapshotOutOfDate));
        }

        let offset = self.entries[0].get_index();
        if idx > self.inner_last_index() {
            panic!(
                "Snapshot {} is out of bound lastIndex({})",
                idx,
                self.inner_last_index()
            )
        }

        self.snapshot.mut_metadata().set_index(idx);
        self.snapshot
            .mut_metadata()
            .set_term(self.entries[(idx - offset) as usize].get_term());

        if let Some(conf_state) = conf_state {
            self.snapshot.mut_metadata().set_conf_state(conf_state);
        }

        self.snapshot.set_data(data);

        Ok(&self.snapshot)
    }

    pub fn compact(&mut self, compact_idx: u64) -> Result<()> {
        let offset = self.entries[0].get_index();
        if compact_idx <= offset {
            return Err(Error::Store(StorageError::Compacted));
        }

        if compact_idx > self.inner_last_index() {
            panic!(
                "Compact {} is out of bound lastIndex({})",
                compact_idx,
                self.inner_last_index()
            )
        }

        let i = (compact_idx - offset) as usize;
        let entries = self.entries.drain(i..).collect();
        self.entries = entries;

        Ok(())
    }

    pub fn append(&mut self, entries: &[Entry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let first = self.entries[0].get_index() + 1;
        let last = entries[0].get_index() + entries.len() as u64 - 1;

        if last < first {
            return Ok(());
        }

        let te: &[Entry] = if first > entries[0].get_index() {
            let start_end = (first - entries[0].get_index()) as usize;
            &entries[start_end..]
        } else {
            entries
        };

        let offset = te[0].get_index() - self.entries[0].get_index();
        if self.entries.len() as u64 > offset {
            let mut new_entries: Vec<Entry> = vec![];
            new_entries.extend_from_slice(&self.entries[..offset as usize]);
            new_entries.extend_from_slice(te);
        } else if self.entries.len() as u64 == offset {
            self.entries.extend_from_slice(te);
        } else {
            panic!(
                "Missing log entry[last: {}, append at: {}]",
                self.inner_last_index(),
                te[0].get_index()
            )
        }

        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct MemStorage {
    core: Arc<RwLock<MemStorageCore>>,
}

impl MemStorage {
    pub fn new() -> MemStorage {
        MemStorage {
            ..Default::default()
        }
    }

    pub fn rl(&self) -> RwLockReadGuard<MemStorageCore> {
        self.core.read().unwrap()
    }

    pub fn wl(&self) -> RwLockWriteGuard<MemStorageCore> {
        self.core.write().unwrap()
    }
}

impl Storage for MemStorage {
    fn initial_state(&self) -> Result<RaftState> {
        let core = self.rl();

        Ok(RaftState {
            hard_state: core.hard_state.clone(),
            conf_state: core.snapshot.get_metadata().get_conf_state().clone(),
        })
    }

    fn entries(&self, low: u64, high: u64, max_size: u64) -> Result<Vec<Entry>> {
        let core = self.rl();
        let offset = core.entries[0].get_index();

        if low <= offset {
            return Err(Error::Store(StorageError::Compacted));
        }

        if high > core.inner_last_index() + 1 {
            panic!("index out of the bound");
        }

        if core.entries.len() == 1 {
            return Err(Error::Store(StorageError::Unavailable));
        }

        let lo = (low - offset) as usize;
        let hi = (high - offset) as usize;
        let mut entries = core.entries[lo..hi].to_vec();
        util::limit_size(&mut entries, max_size);

        Ok(entries)
    }

    fn term(&self, idx: u64) -> Result<u64> {
        let core = self.rl();
        let offset = core.entries[0].get_index();

        if idx < offset {
            return Err(Error::Store(StorageError::Compacted));
        }

        if idx - offset >= core.entries.len() as u64 {
            return Err(Error::Store(StorageError::Unavailable));
        }

        Ok(core.entries[(idx - offset) as usize].get_term())
    }

    fn first_index(&self) -> Result<u64> {
        let core = self.rl();
        Ok(core.entries[0].get_index() + 1)
    }

    fn last_index(&self) -> Result<u64> {
        let core = self.rl();
        Ok(core.inner_last_index())
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let core = self.rl();
        Ok(core.snapshot.clone())
    }
}
