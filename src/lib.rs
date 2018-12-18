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

#[cfg(test)]
mod raft;
mod raft_log;
pub mod storage;
pub mod util;
