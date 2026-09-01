//! The census says *I do not know* instead of *nothing*.
//!
//! **WHY THIS TEST EXISTS, AND WHY IT CANNOT ASK `ps`.** Inside the sandbox
//! this suite runs in `ps` is really denied: a test that invoked it would
//! measure the sandbox and not the code, and on the machine of whoever has no
//! such sandbox it would prove nothing at all.

//! So the machine here is a fake, and there are three: one that says no, one
//! that **says yes and answers nothing** — the silent denial — and one that
//! really answers.

//! The original fault: `ps -e | wc -l` writes `0` with **exit status 0**.
//! Whoever reads an empty vector has no way to tell "no terminal" apart from
//! "they did not let me ask", and the two lead to opposite decisions.

use sessions::census::{Census, Machine, Refusal};

/// The machine that refuses and says so.
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
        4242
    }
}

/// The machine that refuses **without saying so**: clean exit, empty answer.
/// That is the exact shape a denial takes once the output goes through a pipe.
struct SilentlyDenied;

impl Machine for SilentlyDenied {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(String::new())
    }
    fn working_directory(&self, _pid: u32) -> Option<String> {
        None
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// The machine that answers: a table written the way `ps -e -o
/// pid=,ppid=,tty=,etime=,comm=` writes one, copied from a real one.
struct Answering {
    table: &'static str,
}

impl Machine for Answering {
    fn process_table(&self) -> Result<String, Refusal> {
        Ok(self.table.to_owned())
    }
    fn working_directory(&self, pid: u32) -> Option<String> {
        // Only one of them is known: the rest stay `None`, which is "I do not
        // know".
        (pid == 7073).then(|| "/Users/somebody/work/general".to_owned())
    }
    fn own_pid(&self) -> u32 {
        4242
    }
}

/// A live machine with two terminals, a shared ancestor, and the asker inside
/// the table.
const FIVE_LINES: &str = "\
 3354  7157 ttys001        03:32 caffeinate
 7072   886 ttys001  02-01:05:27 /usr/bin/login
 7073  7072 ttys001  02-01:05:27 /bin/zsh
 4242  7073 ttys001        00:01 sailor
32375   886 ttys002        58:21 /usr/bin/login
  886   675 ??         1-00:00:00 /Applications/Whatever.app/Contents/Frameworks/Whatever Helper.app/Contents/MacOS/Whatever Helper
  675     1 ??         1-00:00:00 /Applications/Whatever.app/Contents/MacOS/Whatever
";

/// A live machine where **nobody** has a terminal: the table is there, the
/// asker is there, and no row has a tty.
const NO_ONE_ON_A_TERMINAL: &str = "\
    1     0 ??         9-00:00:00 /sbin/launchd
 4242     1 ??            00:01 sailor
";

#[test]
fn a_refusal_is_not_an_empty_machine() {
    match Census::of(&Denied) {
        Census::Refused(refusal) => {
            assert_eq!(refusal.tool, "ps");
            assert!(
                refusal.reason.contains("not permitted"),
                "a denial must carry the words it arrived with: {refusal}"
            );
        }
        other => panic!("a denial was taken for an empty machine: {other:?}"),
    }
}

/// **THE CANARY.** A denied `ps` can exit with code 0 and empty output, and
/// then no error betrays it. But whoever asks for the process table *is* a
/// process: if it is not there, that is not the machine.
#[test]
fn a_silent_refusal_is_caught_by_the_canary() {
    match Census::of(&SilentlyDenied) {
        Census::Refused(refusal) => assert!(
            refusal.reason.contains("4242"),
            "a silent denial must be explained by the pid that is missing: {refusal}"
        ),
        other => panic!(
            "a clean exit with an empty answer was taken for a deserted machine: {other:?}"
        ),
    }
}

#[test]
fn a_machine_without_terminals_says_so_and_is_not_a_refusal() {
    assert_eq!(
        Census::of(&Answering {
            table: NO_ONE_ON_A_TERMINAL
        }),
        Census::NoTerminal
    );
}

#[test]
fn the_terminals_are_grouped_by_tty() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    let Census::Terminals(terminals) = &census else {
        panic!("a table with two terminals yielded no terminals: {census:?}");
    };
    let names: Vec<&str> = terminals.iter().map(|found| found.tty.as_str()).collect();
    assert_eq!(names, vec!["ttys001", "ttys002"]);
    assert_eq!(terminals[0].inhabitants.len(), 4, "{:?}", terminals[0]);
    assert_eq!(terminals[1].inhabitants.len(), 1);
}

/// The ancestor is found by walking up the parent chain, and both terminals
/// reach the same one. **It is a label**: what is checked here is that it is
/// there and readable, not that it is one product rather than another.
#[test]
fn every_terminal_carries_the_label_of_who_drew_it() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    assert_eq!(census.ancestor_of("ttys001"), Some("Whatever"));
    assert_eq!(census.ancestor_of("ttys002"), Some("Whatever"));
    assert_eq!(census.ancestor_of("ttys009"), None);
}

#[test]
fn each_process_carries_its_pid_its_age_its_command_and_where_it_works() {
    let census = Census::of(&Answering { table: FIVE_LINES });
    let terminals = census.seen();
    let shell = terminals[0]
        .inhabitants
        .iter()
        .find(|found| found.pid == 7073)
        .expect("the shell is in the table");
    assert_eq!(shell.parent_pid, 7072);
    assert_eq!(shell.uptime, "02-01:05:27");
    assert_eq!(shell.command, "/bin/zsh");
    assert_eq!(
        shell.working_directory.as_deref(),
        Some("/Users/somebody/work/general")
    );
    let unknown = terminals[0]
        .inhabitants
        .iter()
        .find(|found| found.pid == 3354)
        .expect("caffeinate is in the table");
    assert_eq!(
        unknown.working_directory, None,
        "a directory that could not be read stays \"I do not know\""
    );
}

/// **`Terminals` CANNOT BE EMPTY.** If it could, the type would again have two
/// ways of saying "nothing" and the distinction this module exists to make
/// would vanish from the inside.
#[test]
fn a_census_with_terminals_is_never_an_empty_one() {
    for machine in [NO_ONE_ON_A_TERMINAL, FIVE_LINES] {
        if let Census::Terminals(terminals) = Census::of(&Answering { table: machine }) {
            assert!(
                !terminals.is_empty(),
                "Terminals(vec![]) is a second way of saying \"nothing\""
            );
        }
    }
}

/// A command with spaces inside stays whole: `npm exec something` is three
/// words and one command.
#[test]
fn a_command_made_of_several_words_stays_whole() {
    let census = Census::of(&Answering {
        table: concat!(
            "   10     1 ttys003        00:05 npm exec socraticode\n",
            " 4242    10 ttys003        00:01 sailor\n"
        ),
    });
    let terminals = census.seen();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].inhabitants[0].command, "npm exec socraticode");
}
