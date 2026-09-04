//! **THE SAME TRUTH IN TWO PROSES IS TWO TRUTHS WAITING TO DISAGREE.** The
//! page said a terminal outlives the window, and the shell beside it had a
//! paragraph saying that was exactly what it did not give. Both were green,
//! because neither is code. This reads them together.

use std::path::{Path, PathBuf};

/// What a claim about a terminal's lifetime looks like, in either language.
/// The word alone, and not a phrase: «outlives the window», «outlives this
/// one» and «outlives this window» were all written in these three files, and
/// a check on the phrase read two of them as saying nothing.
const A_SURVIVAL_CLAIM: [&str; 2] = ["outlives", "sopravvive"];

/// Who actually holds a terminal. A page claiming survival must be able to
/// name it, and so must the shell: one of the two saying it alone is the
/// state this test exists to refuse.
const THE_HOLDER: &str = "sailor terminal host";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn read(at: &str) -> String {
    let path = root().join(at);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is not readable", path.display()))
}

/// The comment lines of a source, run together as one line of words: a
/// sentence wrapped across two lines is the same sentence, and matching on
/// the raw lines would miss exactly the claims that are long enough to wrap.
fn prose(text: &str) -> String {
    text.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("//") || line.starts_with('*') || line.starts_with("/*"))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn whoever_promises_a_terminal_outlives_the_window_names_who_holds_it() {
    let shell = prose(&read("desktop/src-tauri/src/terminal.rs"));
    let pages = [
        ("desktop/src/terminal.ts", prose(&read("desktop/src/terminal.ts"))),
        ("desktop/src/Terminals.tsx", prose(&read("desktop/src/Terminals.tsx"))),
    ];

    let claiming: Vec<&str> = pages
        .iter()
        .filter(|(_, text)| A_SURVIVAL_CLAIM.iter().any(|claim| text.contains(claim)))
        .map(|(name, _)| *name)
        .collect();
    // THE CONTROL: with nobody claiming anything the check below would pass
    // for having read two files that say nothing.
    assert!(
        !claiming.is_empty(),
        "no page claims a terminal outlives the window: this test is measuring nothing"
    );

    assert!(
        A_SURVIVAL_CLAIM.iter().any(|claim| shell.contains(claim)),
        "{claiming:?} promise a terminal outlives the window, and the shell beside them does not say so"
    );
    assert!(
        shell.contains(THE_HOLDER),
        "the shell promises survival without naming «{THE_HOLDER}», which is what holds it"
    );
}
