//! This file exists to be refused, and it is deleted the moment it has been.
//!
//! Five of the six checks the trunk requires had never been seen to fail, and
//! two of them could not. Asking a gate whether it works by reading it is how
//! both of those survived; the only answer that holds is a run that concluded
//! `failure`, on a real commit, with a receipt anybody can fetch again.

/// `workspace tests` is asked to refuse.
#[test]
fn the_battery_is_asked_to_refuse() {
    assert!(
        false,
        "this failure is deliberate: it is the receipt of `workspace tests`"
    );
}

/// `clippy gate` is asked to refuse: `eq_op` is `clippy::correctness`, which
/// that job raises to an error. The same comparison is one more `warning:` in
/// the plain run the style job counts, so it carries the ceiling over too.
#[test]
fn the_clippy_gate_is_asked_to_refuse() {
    // `approx_constant` is `clippy::correctness`, which that job raises to an
    // error. Two lints were tried before it: `eq_op` never fired at all, and
    // inside `assert!` the expansion hid it from clippy entirely.
    let written_out_by_hand = 3.141_592_653_589_793_f64;
    assert!(written_out_by_hand > 3.0);
}
