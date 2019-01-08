pub use super::read_only::{ReadOnlyOption, ReadState};
use super::{
    errors::{Error, Result},
    INVALID_ID,
};

pub struct Config {
    pub id: u64,

    pub peers: Vec<u64>,

    pub learners: Vec<u64>,

    pub election_tick: usize,

    pub heartbeat_tick: usize,

    pub applied: u64,

    pub max_size_per_msg: u64,

    pub max_inflight_msgs: usize,

    pub check_quorum: bool,

    pub pre_vote: bool,

    pub min_election_tick: usize,

    pub max_election_tick: usize,

    pub read_only_option: ReadOnlyOption,

    pub skip_bcast_commit: bool,

    pub tag: String,
}

impl Default for Config {
    fn default() -> Self {
        const HEARTBEAT_TICK: usize = 2;
        Config {
            id: 0,
            peers: vec![],
            learners: vec![],
            election_tick: HEARTBEAT_TICK * 10,
            heartbeat_tick: HEARTBEAT_TICK,
            applied: 0,
            max_size_per_msg: 0,
            max_inflight_msgs: 256,
            check_quorum: false,
            pre_vote: false,
            min_election_tick: 0,
            max_election_tick: 0,
            read_only_option: ReadOnlyOption::Safe,
            skip_bcast_commit: false,
            tag: "".to_string(),
        }
    }
}

impl Config {
    pub fn new(id: u64) -> Self {
        Self {
            id: 0,
            tag: format!("{}", id),
            ..Self::default()
        }
    }

    #[inline]
    pub fn min_election_tick(&self) -> usize {
        if self.election_tick == 0 {
            self.election_tick
        } else {
            self.min_election_tick
        }
    }

    #[inline]
    pub fn max_election_tick(&self) -> usize {
        if self.max_election_tick == 0 {
            2 * self.election_tick
        } else {
            self.election_tick
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.id == INVALID_ID {
            return Err(Error::ConfigInvalid("invalid node id".to_owned()));
        }

        if self.heartbeat_tick <= 0 {
            return Err(Error::ConfigInvalid(
                "heartbeat tick must be greater than 0".to_owned(),
            ));
        }

        if self.election_tick <= self.heartbeat_tick {
            return Err(Error::ConfigInvalid(
                "election tick must be greater than heartbeat tick".to_owned(),
            ));
        }

        let min_timeout = self.min_election_tick();
        let max_timeout = self.max_election_tick();
        if min_timeout < self.election_tick {
            return Err(Error::ConfigInvalid(format!(
                "min election tick {} must not be less than election tick {}",
                min_timeout, self.election_tick
            )));
        }

        if min_timeout >= max_timeout {
            return Err(Error::ConfigInvalid(format!(
                "min election tick {} should be less than max election tick {}",
                min_timeout, max_timeout
            )));
        }

        if self.max_inflight_msgs <= 0 {
            return Err(Error::ConfigInvalid(
                "max inflight messages must be greater than 0".to_owned(),
            ));
        }

        if self.read_only_option == ReadOnlyOption::LeaseBase && !self.check_quorum {
            return Err(Error::ConfigInvalid(
                "read_only_option == LeaseBase requires check_quorum == true".into(),
            ));
        }

        Ok(())
    }
}
