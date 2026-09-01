//! Flow accounting for Sailor, kept apart from the I/O that feeds it. **THE
//! `ui` NAME OUTLIVED THE SERVER IT BACKED**: what the page knew how to say —
//! today's summary, the run history, what is installed — was carried into the
//! window **before** the rest was taken away, not after. Two sums written in
//! two places would give two figures and nobody would know which to believe.
//! `dashboard`, `registry`, `parse`: pure, so tests run with no I/O at all.

pub mod dashboard;
pub mod gather;
pub mod parse;
pub mod registry;
