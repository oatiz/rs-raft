use eraftpb::{Entry, Snapshot};

#[derive(Debug, PartialEq, Default)]
pub struct Unstable {
    pub snapshot: Option<Snapshot>,

    pub entries: Vec<Entry>,

    pub offset: u64,

    pub tag: String,
}

impl Unstable {
    pub fn new(offset: u64, tag: String) -> Unstable {
        Unstable {
            offset,
            snapshot: None,
            entries: vec![],
            tag,
        }
    }

    pub fn maybe_first_index(&self) -> Option<u64> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.get_metadata().get_index() + 1)
    }

    pub fn maybe_last_index(&self) -> Option<u64> {
        match self.entries.len() {
            0 => self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.get_metadata().get_index()),
            len => Some(self.offset + len as u64 - 1),
        }
    }

    pub fn maybe_term(&self, idx: u64) -> Option<u64> {
        if idx < self.offset {
            let snapshot = self.snapshot.as_ref()?;
            let meta = snapshot.get_metadata();
            if idx == meta.get_index() {
                Some(meta.get_term())
            } else {
                None
            }
        } else {
            self.maybe_last_index().and_then(|last| {
                if idx > last {
                    None
                } else {
                    Some(self.entries[(idx - self.offset) as usize].get_term())
                }
            })
        }
    }

    pub fn stable_to(&mut self, idx: u64, term: u64) {
        let t = self.maybe_term(idx);
        if t.is_none() {
            return;
        }

        if t.unwrap() == term && idx >= self.offset {
            let start = idx + 1 - self.offset;
            self.entries.drain(..start as usize);
            self.offset = idx + 1;
        }
    }

    pub fn stable_snapshot_to(&mut self, idx: u64) {
        if self.snapshot.is_none() {
            return;
        }
        if idx == self.snapshot.as_ref().unwrap().get_metadata().get_index() {
            self.snapshot = None;
        }
    }

    pub fn restore(&mut self, snapshot: Snapshot) {
        self.entries.clear();
        self.offset = snapshot.get_metadata().get_index();
        self.snapshot = Ok(snapshot);
    }

    pub fn truncate_and_append(&mut self, entries: &[Entry]) {
        let after = entries[0].get_index();
        if after == self.offset + self.entries.len() as u64 {
            self.entries.extend_from_slice(entries);
        } else if after <= self.offset {
            self.offset = after;
            self.entries.clear();
            self.entries.extend_from_slice(entries);
        } else {
            let offset = self.offset;
            self.must_check_out_of_bounds(offset, after);
            self.entries.truncate((after - offset) as usize);
            self.entries.extend_from_slice(entries);
        }
    }

    pub fn slice(&self, low: u64, high: u64) -> &[Entry] {
        self.must_check_out_of_bounds(low, high);

        let lo = low as usize;
        let hi = high as usize;
        let off = self.offset as usize;

        &self.entries[lo - off..hi - off]
    }

    pub fn must_check_out_of_bounds(&self, low: u64, high: u64) {
        if low > high {
            panic!("{} invalid unstable.slice {} > {}", self.tag, low, high)
        }

        let upper = self.offset + self.entries.len() as u64;
        if low < self.offset || high > upper {
            panic!(
                "{} unstable.slice[{}, {}] out of bound[{}, {}]",
                self.tag, low, high, self.offset, upper
            )
        }
    }
}
