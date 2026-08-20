//! The library half of `tezgah-server` — `main.rs` is a thin binary over
//! these modules, split out so `tests/` can build a router and drive it
//! with a request in-process, without a live process or a database.

pub mod config;
pub mod host;
pub mod http;
pub mod identity;
pub mod provider;
pub mod schedule;
pub mod seed;
