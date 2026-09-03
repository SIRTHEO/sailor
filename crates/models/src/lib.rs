//! The catalog of models Sailor can use, filterable, with only the free ones
//! configurable for now. `catalog`, `usage` and `pricing` are pure: JSON in,
//! values out, tested on the samples in `tests/fixtures/`, never over the
//! network. `config` is the shape of the user's choice and the free-only rule;
//! `store` and `fetch` are the **only two** points that touch disk and network,
//! and the network half deliberately has no tests.

pub mod catalog;
pub mod command;
pub mod config;
pub mod fetch;
pub mod pact;
pub mod pricing;
pub mod remaining;
pub mod store;
pub mod strengths;
pub mod usage;
