//! Where a heavy step asks for the machine before its action starts. This
//! crate cannot read the machine, so whoever runs flows says how to wait for
//! it, once per process; a process that says nothing runs heavy steps at once.

use std::sync::OnceLock;

/// The shared key a heavy step's action reads the turn's token from, and the
/// variable it hands that token on in to the processes it starts.
pub const MACHINE_TURN: &str = "machine.turn";
pub const MACHINE_TURN_VARIABLE: &str = "SAILOR_MACHINE_TURN";

/// A turn on the machine: it ends when `hold` is dropped.
pub struct HeldTurn {
    pub token: String,
    pub hold: Box<dyn Send>,
}

pub trait MachineTurns: Send + Sync {
    /// Blocks until the machine is this step's. `carried` is the token of the
    /// turn the run already runs inside, when a heavy step called it.
    fn wait_for_the_machine(
        &self,
        run_id: &str,
        step_id: &str,
        carried: Option<&str>,
    ) -> Result<HeldTurn, String>;
}

static TURNS: OnceLock<Box<dyn MachineTurns>> = OnceLock::new();

pub fn heavy_steps_wait_on(turns: Box<dyn MachineTurns>) {
    let _ = TURNS.set(turns);
}

pub(crate) fn the_machine_for(
    run_id: &str,
    step_id: &str,
    carried: Option<&str>,
) -> Result<Option<HeldTurn>, String> {
    match TURNS.get() {
        None => Ok(None),
        Some(turns) => turns
            .wait_for_the_machine(run_id, step_id, carried)
            .map(Some),
    }
}
