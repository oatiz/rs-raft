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

#[cfg(test)]
pub mod raft;
