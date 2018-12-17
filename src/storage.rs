use eraft_demo::{HardState, ConfState};

use errors::{Error, Result, StorageError};

#[derive(Debug, Clone)]
pub struct RaftState {
    pub hard_state: HardState,

    pub conf_state: ConfState,
}

pub trait Storage {
    fn initial_state(&self) -> Result<RaftState>;

    fn entries(&self, low: u64, high: u64, max_size: u64) -> Result<Vec<Entry>>;


}