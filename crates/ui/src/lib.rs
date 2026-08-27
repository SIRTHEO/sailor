//! La libreria dietro `sailor ui`: i conti sui flussi di Sailor, separati
//! dall'I/O che li alimenta.
//!
//! `dashboard` e `registry` sono puri — niente disco, niente rete — e le
//! prove ci girano sopra direttamente. `parse` traduce l'uscita grezza del
//! deposito (anch'esso puro: prende un `serde_json::Value`, non un file).
//! `gather` e `server` sono il collante impuro: aprono il deposito e
//! ascoltano una porta.

pub mod dashboard;
pub mod gather;
pub mod parse;
pub mod registry;
pub mod server;
