//! Il catalogo dei modelli che Sailor può usare, filtrabile, con i soli
//! gratuiti accesi per ora — mandato di Theo del 27/08/2026.
//!
//! `catalog`, `usage` e `listino` sono puri: JSON dentro, valori fuori, provati sui
//! campioni salvati in `tests/fixtures/`, mai sulla rete. `config` è la
//! forma della scelta dell'utente e la regola dei soli gratuiti; `store` e
//! `fetch` sono i due soli punti che toccano disco e rete, e restano senza
//! prove di rete per lo stesso motivo per cui `notte` non ne ha.

pub mod catalog;
pub mod command;
pub mod config;
pub mod fetch;
pub mod pricing;
pub mod store;
pub mod usage;
