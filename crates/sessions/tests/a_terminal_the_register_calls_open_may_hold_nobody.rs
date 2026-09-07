//! A terminal killed without closing stays open in the register for ever, and
//! that is a fact to show. This is the reading that shows it, and that refuses
//! to speak when the machine would not let us look. Fault 126.
//!
//! The machine is a fake, for the reason the sibling test gives: in this
//! suite's sandbox `ps` is really denied.

use sessions::census::{Abandoned, Census, Machine, Refusal};
use sessions::store::TerminalRow;

struct Answering {
    table: &'static str,
}

impl Machine for Answering {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(self.table.to_owned())
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

struct Denied;

impl Machine for Denied {
    fn process_table(&self) -> Result<String, Refusal> {
        Err(Refusal { tool: "ps".to_owned(), reason: "operation not permitted: ps".to_owned() })
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// One terminal alive, and the asker on it.
const ONE_ALIVE: &str = "\
 7073  7072 ttys001  02-01:05:27 /bin/zsh
 4242  7073 ttys001        00:01 sailor
";

fn row(tty: &str, closed_at: Option<i64>) -> TerminalRow {
    TerminalRow {
        tty: tty.to_owned(),
        worktree: "/work/project".to_owned(),
        ancestor: None,
        session_id: None,
        transcript_path: None,
        opened_at: 100,
        closed_at,
        detached_at: None,
    }
}

#[test]
fn an_open_row_no_process_backs_is_named() {
    let census = Census::of(&Answering { table: ONE_ALIVE });
    let found = census.abandoned(&[row("ttys001", None), row("ttys009", None)]);
    assert_eq!(
        found,
        Abandoned::Seen { ttys: vec!["ttys009".to_owned()] },
        "the register calls both open; only one of them holds anybody"
    );
}

/// A row the register already closed claims nothing, so it is not accused.
#[test]
fn a_closed_row_is_not_asked_about() {
    let census = Census::of(&Answering { table: ONE_ALIVE });
    assert_eq!(census.abandoned(&[row("ttys009", Some(200))]), Abandoned::Seen { ttys: Vec::new() });
}

/// **THE CASE THE TWO STATES EXIST FOR.** Answered with a list alone, a machine
/// that would not let us look would report a clean register.
#[test]
fn a_machine_we_could_not_look_at_says_so_instead_of_saying_none() {
    let census = Census::of(&Denied);
    match census.abandoned(&[row("ttys009", None)]) {
        Abandoned::CouldNotLook { refusal } => assert!(
            refusal.reason.contains("not permitted"),
            "the refusal must carry the words it arrived with: {refusal}"
        ),
        Abandoned::Seen { ttys } => {
            panic!("a machine we could not question reported a verdict: {ttys:?}")
        }
    }
}

/// A machine where nobody holds a terminal is an answer, not a refusal: every
/// open row is then genuinely abandoned.
#[test]
fn a_machine_with_no_terminal_at_all_still_answers() {
    let census = Census::of(&Answering { table: " 4242     1 ??            00:01 sailor\n" });
    assert_eq!(census, Census::NoTerminal);
    assert_eq!(
        census.abandoned(&[row("ttys001", None)]),
        Abandoned::Seen { ttys: vec!["ttys001".to_owned()] },
        "looked at and empty is not the same as not looked at"
    );
}
