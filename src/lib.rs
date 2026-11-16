//! Feedwatch poller: reads RedStone push feeds, checks them, and writes results to InfluxDB.
//!
//! Module map and the requirement each delivers (see feedwatch-platform `docs/REQUIREMENTS.md`):
//! `manifest` FR-FW-1, `abi` and `rpc` FR-FW-2, `checks` FR-FW-3/4, `reference` FR-FW-4,
//! `influx` FR-FW-5, `poller` NFR-1/2. `config` reads the environment.

pub mod abi;
pub mod checks;
pub mod config;
pub mod influx;
pub mod manifest;
pub mod poller;
pub mod reference;
pub mod rpc;
