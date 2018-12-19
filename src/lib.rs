#[cfg(feature = "failpoint")]
#[macro_use]
extern crate fail;
extern crate fxhash;
#[macro_use]
extern crate log;
extern crate protobuf;
#[macro_use]
extern crate quick_error;
#[cfg(test)]
extern crate env_logger;
extern crate rand;

pub mod eraftpb;
mod errors;

mod log_unstable;
#[cfg(test)]
mod raft;
mod raft_log;
pub mod storage;
pub mod util;

pub use self::config::Config;
pub use self::errors::{Error, Result, StorageError};
pub use self::log_unstable::Unstable;
pub use self::progress::{Inflights, Progress, ProgressSet, ProgressState};
pub use self::raft::{vote_resp_msg_type, Raft, SoftState, StateRole, INVALID_ID, INVALID_INDEX};
pub use self::raft_log::{RaftLog, NO_LIMIT};
pub use self::raw_node::{is_empty_snap, Peer, RawNode, Ready, SnapshotStatus};
pub use self::read_only::{ReadOnlyOption, ReadState};
pub use self::status::Status;
pub use self::storage::{RaftState, Storage};

pub mod prelude {
    //! A "prelude" for crates using the `raft` crate.
    //!
    //! This prelude is similar to the standard library's prelude in that you'll
    //! almost always want to import its entire contents, but unlike the standard
    //! library's prelude you'll have to do so manually:
    //!
    //! ```
    //! use raft::prelude::*;
    //! ```
    //!
    //! The prelude may grow over time as additional items see ubiquitous use.

    pub use eraftpb::{
        ConfChange, ConfChangeType, ConfState, Entry, EntryType, HardState, Message, MessageType,
        Snapshot, SnapshotMetadata,
    };

    pub use config::Config;
    pub use raft::Raft;

    pub use storage::{RaftState, Storage};

    pub use raw_node::{Peer, RawNode, Ready, SnapshotStatus};

    pub use progress::Progress;

    pub use status::Status;

    pub use read_only::{ReadOnlyOption, ReadState};
}

#[cfg(test)]
fn setup_for_test() {
    let _ = env_logger::try_init();
}
