use eraftpb::{Entry, Snapshot};
use errors::{Error, Result, StorageError};
use log_unstable::Unstable;
use std::cmp;
use storage::Storage;
use util;

#[derive(Default)]
pub struct RaftLog<T: Storage> {
    pub store: T,

    pub unstable: Unstable,

    pub committed: u64,

    pub applied: u64,

    pub tag: String,
}

impl<T> ToString for RaftLog<T>
where
    T: Storage,
{
    fn to_string(&self) -> String {
        format!(
            "committed:{}, applied={}, unstable.offset={}, unstable.entries.len()={}",
            self.committed,
            self.applied,
            self.unstable.offset,
            self.unstable.entries.len(),
        )
    }
}

impl<T: Storage> RaftLog<T> {
    pub fn new(storage: T, tag: String) -> RaftLog<T> {
        let first_index = storage.first_index().unwrap();
        let last_index = storage.last_index().unwrap();

        RaftLog {
            store: storage,
            committed: first_index - 1,
            applied: first_index - 1,
            unstable: Unstable::new(last_index + 1, tag.clone()),
            tag,
        }
    }

    pub fn last_term(&self) -> u64 {
        match self.term(self.last_index()) {
            Ok(t) => t,
            Err(e) => panic!(
                "{} unexpected error with getting the last term: {:?}",
                self.tag, e
            ),
        }
    }

    #[inline]
    pub fn get_store(&self) -> &T {
        &self.store
    }

    #[inline]
    pub fn mut_store(&mut self) -> &mut T {
        &mut self.store
    }

    pub fn term(&self, idx: u64) -> Result<u64> {
        let dummy_index = self.first_index() - 1;
        if idx < dummy_index || idx > self.last_index() {
            return Ok(0u64);
        }

        match self.unstable.maybe_term(idx) {
            Some(term) => Ok(term),
            _ => self.store.term(idx).map_err(|e| {
                match e {
                    Error::Store(StorageError::Compacted)
                    | Error::Store(StorageError::Unavailable) => {}
                    _ => panic!("{} unexpected error: {:?}", self.tag, e),
                }
                e
            }),
        }
    }

    pub fn first_index(&self) -> u64 {
        match self.unstable.maybe_first_index() {
            Some(idx) => idx,
            None => self.store.first_index().unwrap(),
        }
    }

    pub fn last_index(&self) -> u64 {
        match self.unstable.maybe_last_index() {
            Some(idx) => idx,
            None => self.store.last_index().unwrap(),
        }
    }

    pub fn find_conflict(&self, entries: &[Entry]) -> u64 {
        for entry in entries {
            if !self.match_term(entry.get_index(), entry.get_term()) {
                if entry.get_index() <= self.last_index() {
                    info!(
                        "{} found conflict at index {}, [existing term:{}, conflicting term:{}]",
                        self.tag,
                        entry.get_index(),
                        self.term(entry.get_index()).unwarp_or(0),
                        entry.get_term()
                    );
                }
                return entry.get_index();
            }
        }
        0
    }

    pub fn match_term(&self, idx: u64, term: u64) -> bool {
        self.term(idx).map(|t| t == term).unwrap_or(false)
    }

    pub fn maybe_append(
        &mut self,
        idx: u64,
        term: u64,
        committed: u64,
        entries: &[Entry],
    ) -> Option<u64> {
        let last_new_index = idx + entries.len() as u64;
        if self.match_term(idx, term) {
            let conflict_idx = self.find_conflict(entries);
            if conflict_idx == 0 {
            } else if conflict_idx <= self.committed {
                panic!(
                    "{} entry {} conflict with committed entry {}",
                    self.tag, conflict_idx, self.committed
                );
            } else {
                let offset = idx + 1;
                self.append(&entries[(conflict_idx - offset) as usize..]);
            }
            self.commit_to(cmp::min(committed, last_new_index));
            return Some(last_new_index);
        }
        None
    }

    pub fn commit_to(&mut self, to_commit: u64) {
        if self.committed >= to_commit {
            return;
        }

        if self.last_index() < to_commit {
            panic!(
                "{} to_commit {} is out of range [last_index {}]",
                self.tag,
                to_commit,
                self.last_index()
            );
        }

        self.committed = to_commit;
    }

    pub fn applied_to(&mut self, idx: u64) {
        if idx == 0 {
            return;
        }

        if self.committed < idx || idx < self.applied {
            panic!(
                "{} applied({}) is out of range [prev_applied({}), committed({})]",
                self.tag, self.applied, self.committed
            );
        }

        self.applied = idx;
    }

    pub fn get_append(&self) -> u64 {
        self.applied
    }

    pub fn stable_to(&mut self, idx: u64, term: u64) {
        self.unstable.stable_to(idx, term);
    }

    pub fn stable_snapshot_to(&mut self, idx: u64) {
        self.unstable.stable_snapshot_to(idx);
    }

    pub fn get_unstable(&self) -> &Unstable {
        &self.unstable
    }

    pub fn append(&mut self, entries: &[Entry]) -> u64 {
        if entries.is_empty() {
            return self.last_index();
        }

        let after = entries[0].get_index() - 1;
        if after < self.committed {
            panic!(
                "{} after {} is out of range [committed {}]",
                self.tag, after, self.committed
            )
        }
        self.unstable.truncate_and_append(entries);
        self.last_index()
    }

    pub fn unstable_entries(&self) -> Option<&[Entry]> {
        if self.unstable.entries.is_empty() {
            return None;
        }

        Some(&self.unstable.entries)
    }

    pub fn entries(&self, idx: u64, max_idx: u64) -> Result<Vec<Entry>> {
        let last = self.last_index();

        if idx > last {
            return Ok(Vec::new());
        }

        self.sli
    }

    fn must_check_out_of_bounds(&self, low: u64, high: u64) -> Option<Error> {
        if low > high {
            panic!("{} invalid slice {} > {}", self.tag, low, high);
        }

        let first_index = self.first_index();
        if low < first_index {
            return Some(Error::Store(StorageError::Compacted));
        }

        let length = self.last_index() + 1 - first_index;
        if low < first_index || high > first_index + length {
            panic!(
                "{} slice[{}, {}] out of bound[{}, {}]",
                self.tag,
                low,
                high,
                first_index,
                self.last_index()
            );
        }

        None
    }

    pub fn slice(&self, low: u64, high: u64, max_size: u64) -> Result<Vec<Entry>> {
        let err = self.must_check_out_of_bounds(low, high);
        if err.is_some() {
            return Err(err.unwrap());
        }

        let mut entries = vec![];
        if low >= high {
            return Ok(entries);
        }

        if low < self.unstable.offset {
            let stored_entries =
                self.store
                    .entries(low, cmp::min(high, self.unstable.offset), max_size);

            if stored_entries.is_err() {
                let e = stored_entries.unwrap_err();
                match e {
                    Error::Store(StorageError::Compacted) => return Err(e),
                    Error::Store(StorageError::Unavailable) => panic!(
                        "{} entries[{}:{}], is unavailable form storage",
                        self.tag,
                        low,
                        cmp::min(high, self.unstable.offset)
                    ),
                    _ => panic!("{} unexpected error: {:?}", self.tag, e),
                }
            }
            entries = stored_entries.unwrap();
            if (entries.len() as u64) < cmp::min(high, self.unstable.offset) - low {
                return Ok(entries);
            }
        }

        if high > self.unstable.offset {
            let offset = self.unstable.offset;
            let unstable = self.unstable.slice(cmp::max(low, offset), high);
            entries.extend_from_slice(unstable);
        }
        util::limit_size(&mut entries, max_size);

        Ok(entries)
    }
}
