//! I conti sui flussi di Sailor, separati dall'I/O che li alimenta.
//!
//! **IL NOME `ui` È RIMASTO, IL SERVITORE NO.** Fino al 31/08/2026 questa
//! libreria stava dietro `sailor ui`, che serviva una pagina su
//! `127.0.0.1:47831`. Quel comando non c'è più: l'unica interfaccia è la
//! finestra, e ciò che la pagina sapeva dire — il riepilogo di oggi, la storia
//! delle esecuzioni, cosa è installato — è stato portato dentro prima di
//! togliere il resto, non dopo. Quello che resta qui è il motore dei conti, e
//! la finestra lo chiama esattamente come lo chiamava la pagina: due somme
//! scritte in due posti darebbero due cifre, e nessuno saprebbe quale credere.
//!
//! `dashboard` e `registry` sono puri — niente disco, niente rete — e le
//! prove ci girano sopra direttamente. `parse` traduce l'uscita grezza del
//! deposito (anch'esso puro: prende un `serde_json::Value`, non un file).
//! `gather` è il collante impuro: apre il deposito.

pub mod dashboard;
pub mod gather;
pub mod parse;
pub mod registry;
