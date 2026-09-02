//! A hook holds no terminal, and the session still has one.
//!
//! **THE ORIGINAL FAULT.** `open` and `event` asked `ttyname` on their own three
//! descriptors; a hook has a pipe on all three, so both exited **1** several
//! times per turn. The machine below is a fake because `ps` is denied in this
//! sandbox, and a test binary's ancestors are not a hook's.

use sessions::census::{tty_of_nearest_ancestor, Machine, Refusal};

/// A machine that answers with the table it was given, as `ps -e -o
/// pid=,ppid=,tty=,etime=,comm=` writes one: `??` is what it writes for a
/// process with no terminal.
struct Answering {
    table: &'static str,
    asking: u32,
}

impl Machine for Answering {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(self.table.to_owned())
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        self.asking
    }
}

/// The real shape: the hook (`900`) was spawned by a shell (`800`) that the
/// session (`700`) opened, and the session is the one sitting in the window.
const A_HOOK_UNDER_A_SESSION: &str = "\
700 600 ttys004 01:20 claude
800 700 ??      00:02 sh
900 800 ??      00:00 sailor
650 600 ttys009 04:11 vim
";

#[test]
fn the_hook_finds_the_window_two_steps_up() {
    let machine = Answering {
        table: A_HOOK_UNDER_A_SESSION,
        asking: 900,
    };
    assert_eq!(
        tty_of_nearest_ancestor(&machine).as_deref(),
        Some("ttys004"),
        "the hook's own descriptors carry no terminal, and its session's do"
    );
}

/// **THE CASE THAT MUST NOT PASS.** A walk that returned the first terminal it
/// found in the table instead of following the chain would answer `ttys009`
/// here — a window belonging to a stranger — and every session opened from a
/// script would be filed under whoever else happened to be logged in.
#[test]
fn a_terminal_that_is_not_on_our_chain_is_not_ours() {
    const NO_WINDOW_ABOVE_US: &str = "\
500 1   ??      02:00 launchd-ish
800 500 ??      00:02 sh
900 800 ??      00:00 sailor
650 600 ttys009 04:11 vim
";
    let machine = Answering {
        table: NO_WINDOW_ABOVE_US,
        asking: 900,
    };
    assert_eq!(
        tty_of_nearest_ancestor(&machine),
        None,
        "there is a terminal in the table, but not above us: it is someone else's"
    );
}

#[test]
fn a_process_that_has_a_terminal_of_its_own_gets_it() {
    let machine = Answering {
        table: A_HOOK_UNDER_A_SESSION,
        asking: 700,
    };
    assert_eq!(
        tty_of_nearest_ancestor(&machine).as_deref(),
        Some("ttys004")
    );
}

/// A chain that loops back on itself must end the walk, not spin it. `ps` does
/// not write such a table, but a truncated read of one can be parsed into it.
#[test]
fn a_chain_that_bites_its_own_tail_ends() {
    const A_LOOP: &str = "\
900 800 ?? 00:00 sailor
800 900 ?? 00:02 sh
";
    let machine = Answering {
        table: A_LOOP,
        asking: 900,
    };
    assert_eq!(tty_of_nearest_ancestor(&machine), None);
}

/// A denial is not an empty machine — the distinction this crate exists to
/// make. Here it means the same thing for the caller (no name for the
/// terminal), and it must arrive as `None` rather than as a panic.
#[test]
fn a_machine_that_refuses_gives_no_terminal_and_does_not_fall_over() {
    struct Denied;
    impl Machine for Denied {
        fn process_table(&self) -> Result<String, Refusal> {
            Err(Refusal {
                tool: "ps".to_owned(),
                reason: "operation not permitted: ps".to_owned(),
            })
        }
        fn working_directory(&self, _pid: u32) -> Option<String> {
            None
        }
        fn own_pid(&self) -> u32 {
            900
        }
    }
    assert_eq!(tty_of_nearest_ancestor(&Denied), None);
}
