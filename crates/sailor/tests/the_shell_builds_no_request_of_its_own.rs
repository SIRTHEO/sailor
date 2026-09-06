//! The window shell builds no run request of its own.
//!
//! **THIS TEST IS CRUDE, AND STILL BETTER THAN NOTHING.** It looks for a piece of
//! text inside a `.rs` file: it does not compile that file, does not read its
//! syntax tree, and an `ExecutionRequest` built under another name escapes it.
//! The reason is that `desktop/` **sits outside the Rust workspace**: no `cargo
//! test` compiles it, so no check living there can ever go red — and it is where
//! the action list diverged three times with no test seeing it (fault 10).
//!
//! A wrong test here costs little; having none is measured at three silent
//! divergences, the last giving terminal and window two different behaviours for
//! the same flow file. **If `desktop/` ever joins the workspace, throw this away**
//! for the real one: the compiler seeing a single `execution_request`.

use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// The shell's source that launches a run, with the receipt of having read it.
/// Its absence is the defect, not a state of the tree: this judge reads it.
fn the_shells_launcher() -> (PathBuf, String) {
    let path = repository_root().join("desktop/src-tauri/src/run.rs");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("desktop/src-tauri/src/run.rs is gone, so the launcher cannot be read: {error}")
    });
    workspace::measured(text.lines().count(), "lines of the window shell's launcher read");
    (path, text)
}

/// **ONE `ExecutionRequest` IN THE WHOLE TREE, AND IT LIVES IN `registry`.**
/// The project root is the field that would have split the two copies in the
/// worst way: a run launched from the button working wherever the process sits,
/// while the same run from the terminal works in the right root — and neither of
/// them would say so.
#[test]
fn the_window_shell_does_not_build_its_own_execution_request() {
    let (path, text) = the_shells_launcher();

    assert!(
        !text.contains("ExecutionRequest {"),
        "{} si costruisce la richiesta da sé: la costruisce «registry::execution_request», \
         o le due copie tornano a divergere come nel guasto 10",
        path.display()
    );
}

/// The question about money is asked by both launchers, or by neither.
///
/// A flow requiring a guaranteed cap must not start where the guarantee cannot
/// be given, and «must not start» has two doors: the command and the button.
/// One door that does not ask is not half a control — it is the whole of it
/// gone, because whoever launches picks the door.
const THE_QUESTION_BEFORE_A_RUN: &str = "why_a_run_here_would_not_start(";

#[test]
fn both_launchers_ask_whether_the_run_may_start_at_all() {
    let command_line = repository_root().join("crates/sailor/src/flow_cmd/run_and_resume.rs");
    let text = std::fs::read_to_string(&command_line).expect("il lanciatore della riga di comando");
    assert!(
        text.contains(THE_QUESTION_BEFORE_A_RUN),
        "{} deve chiedere se la corsa può partire prima di aprirla",
        command_line.display()
    );

    let (path, shell) = the_shells_launcher();
    assert!(
        shell.contains(THE_QUESTION_BEFORE_A_RUN),
        "{} deve fare la stessa domanda: un tetto garantito che questa macchina \
         non sa dare ferma la corsa dal pulsante come dal terminale",
        path.display()
    );
}

/// And it really calls it: without this line the test above would stay green even
/// if the shell stopped launching altogether.
#[test]
fn the_window_shell_calls_the_shared_constructor() {
    let (path, text) = the_shells_launcher();

    assert!(
        text.contains("registry::execution_request("),
        "{} deve chiedere la richiesta a «registry»",
        path.display()
    );
}
