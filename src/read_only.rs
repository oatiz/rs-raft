use std::collections::VecDeque;

use eraftpb::Message;

use fxhash::{FxHashMap, FxHashSet};

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ReadOnlyOption {
    Safe,
    LeaseBase,
}

impl Default for ReadOnlyOption {
    fn default() -> ReadOnlyOption {
        ReadOnlyOption::Safe
    }
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct ReadState {
    pub index: u64,
    pub request_ctx: Vec<u8>,
}

#[derive(Debug, Default, Clone)]
pub struct ReadIndexStatus {
    pub req: Message,
    pub index: u64,
    pub acks: FxHashSet<u64>,
}

#[derive(Default, Debug, Clone)]
pub struct ReadOnly {
    pub option: ReadOnlyOption,
    pub pending_read_index: FxHashMap<Vec<u8>, ReadIndexStatus>,
    pub read_index_queue: VecDeque<Vec<u8>>,
}

impl ReadOnly {
    pub fn new(option: ReadOnlyOption) -> ReadOnly {
        ReadOnly {
            option,
            pending_read_index: FxHashMap::new(),
            read_index_queue: VecDeque::new(),
        }
    }

    pub fn add_request(&mut self, index: u64, msg: Message) {
        let ctx = {
            let key = msg.get_entries()[0].get_data();
            if self.pending_read_index.contains_key(key) {
                return;
            }
            key.to_vec()
        };

        let status = ReadIndexStatus {
            req: msg,
            index,
            acks: FxHashSet::default(),
        };

        self.pending_read_index.insert(ctx.clone(), status);
        self.read_index_queue.push_back(ctx);
    }

    pub fn recv_ack(&mut self, msg: &Message) -> FxHashSet<u64> {
        match self.pending_read_index.get_mut(msg.get_context()) {
            None => Default::default(),
            Some(read_status) => {
                read_status.acks.insert(msg.get_from());
                let mut set_with_self = FxHashSet::default();
                set_with_self.insert(msg.get_to());
                read_status.acks.union(&set_with_self).clone().collect()
            }
        }
    }

    pub fn advance(&mut self, m: &Message) -> Vec<ReadIndexStatus> {
        let mut rss = vec![];
        if let Some(i) = self.read_index_queue.iter().position(|x| {
            if !self.pending_read_index.contains_key(x) {
                panic!("cannot find correspond read state from pending map");
            }

            *x == m.get_context()
        }) {
            for _ in 0..=i {
                let rs = self.read_index_queue.pop_front().unwrap();
                let status = self.pending_read_index.remove(&rs).unwrap();
                rss.push(status);
            }
        }

        rss
    }

    pub fn last_pending_request_ctx(&self) -> Option<Vec<u8>> {
        self.read_index_queue.back().cloned()
    }

    #[inline]
    pub fn pending_read_count(&self) -> usize {
        self.read_index_queue.len()
    }
}
