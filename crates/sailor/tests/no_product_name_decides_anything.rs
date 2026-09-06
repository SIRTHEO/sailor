//! **THE IRON RULE OF THE TRACKING: a product name may appear in a label, never
//! in a condition.**
//!
//! `println!("gira in Orca")` is fine: a label, read by a person, and visibly
//! wrong when it is wrong. `if host == "orca"` is forbidden: a decision, read by
//! the program, which when wrong does something else without saying so — on a
//! machine where that product is absent, named differently, or replaced.
//!
//! **A TEST AND NOT A LINE IN A DOCUMENT**, because a rule nobody interrogates
//! never goes red: the lesson `AGENTS.md` tells about itself, and it cost 136
//! renames on the identifiers. The first `if` on a product name arrives alone,
//! looks reasonable where it is written, and from then on the tracking is
//! specific to one product with no check saying so.
//!
//! **WHAT IT WATCHES.** The tracking's sources and nothing else: `crates/sessions`
//! and `sailor session`. Elsewhere product names are legitimate, because
//! elsewhere those products are the subject — the command-line descriptors, the
//! profiles, the model catalogue. Not here: here the anchor is `(tty, tree,
//! ancestor)`, and the ancestor is **only a label**.
//!
//! **THE TEST MEASURES ITSELF.** The last test in the file hands its own detector
//! a piece of code that breaks the rule and demands it be found: without that, a
//! change switching the detector off would leave everything green for ever.

use std::path::{Path, PathBuf};

/// The field the rule is really about: the ancestor is a **label**, so it is
/// recorded and printed and never interrogated.
const THE_LABEL_THAT_MUST_NOT_DECIDE: &str = "ancestor";

/// A comparison against a written-down value, spaces removed so `== "x"` and
/// `=="x"` read alike. **This is the half that needs no list of names**:
/// whatever the eighteenth emulator is called, comparing the ancestor with a
/// constant is the defect. Bare `Some("…")` is deliberately absent — it was the
/// first draft's defect, since `ancestor: Some("x".to_owned())` builds a row
/// rather than questioning one.
const COMPARED_WITH_A_WRITTEN_VALUE: &[&str] = &[
    "==\"",
    "!=\"",
    "==some(\"",
    "!=some(\"",
    "contains(\"",
    "starts_with(\"",
    "ends_with(\"",
    "eq(\"",
    "eq_ignore_ascii_case(\"",
];

/// The names that must decide nothing. **The wide net, not the check**: it
/// catches a product name wherever it decides anything, ancestor or not, but it
/// is walked around by picking the entry that is not in it. **Proper names
/// only** — `terminal`, `shell`, `window` are the things, not the products, and
/// a list holding them would raise errors nobody can fix, so the first person
/// to hit one would silence the test along with everything else.
const PRODUCT_NAMES: &[&str] = &[
    "orca",
    "iterm",
    "warp",
    "ghostty",
    "alacritty",
    "wezterm",
    "kitty",
    "zellij",
    "tmux",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "copilot",
    "aider",
    "vscode",
    "jetbrains",
];

/// The signs that a line **decides** instead of reporting.
///
/// The list is wide on purpose: a line naming a product and holding one of these
/// wants a person's eye, and looking costs one comment line. Not looking costs a
/// tracking that works on one machine only.
const SIGNS_OF_A_DECISION: &[&str] = &[
    "if ",
    "else if",
    "match ",
    "while ",
    "==",
    "!=",
    "=>",
    "contains(",
    "contains_key(",
    "starts_with(",
    "ends_with(",
    "matches!",
    ".any(",
    ".all(",
    ".find(",
    ".filter(",
    ".position(",
    "unwrap_or_else(",
    "then(",
    "then_some(",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// The line without the comment that closes it.
///
/// **THE BORDER BETWEEN LABEL AND CONDITION RUNS HERE.** A comment, and a doc
/// comment, name products as much as they need to — this file first of all — and
/// decide nothing. The cut errs downwards: a line with `//` inside a string is
/// shortened too far, so **less** code is watched, never more. This test may let
/// something through; it cannot accuse wrongly.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// The sources of the tracking, and those of the conduit.
///
/// The conduit belongs here because the rule is the same. Whoever holds a
/// command line's terminal and writes into it must not know which one it is: not which
/// window drew it, not which engine runs in it. The first `if` on a name makes
/// Sailor the product of a single command line, and there is no way back.
fn tracking_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = vec![
        root.join("crates/sailor/src/session_cmd.rs"),
        root.join("crates/sailor/src/terminal_cmd.rs"),
    ];
    collect_under(&root.join("crates/sessions"), &mut found);
    collect_under(&root.join("crates/terminal"), &mut found);
    found.retain(|path| path.exists());
    found
}

fn collect_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_under(&path, found);
            continue;
        }
        if path.extension().and_then(|kind| kind.to_str()) == Some("rs") {
            found.push(path);
        }
    }
}

/// Where the ancestor's label is compared with a value written into the code,
/// whatever that value says.
///
/// The literal has to sit **inside** the comparison, or `if ancestor.is_none()
/// { return "unknown"; }` would be accused: a condition on the ancestor and a
/// string on the same line, but the string is the answer, not the comparand.
fn the_label_compared_with_a_constant_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line).to_lowercase();
        if !code.contains(THE_LABEL_THAT_MUST_NOT_DECIDE) {
            continue;
        }
        // An assertion compares what was **recorded**, and a test must be able
        // to. What is forbidden is deciding at run time, not checking.
        if code.contains("assert") {
            continue;
        }
        let tight: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let matched_on = tight.contains("match") && tight.contains("some(\"");
        let shape = if matched_on {
            "match … Some(\"…\")"
        } else {
            match COMPARED_WITH_A_WRITTEN_VALUE
                .iter()
                .find(|shape| tight.contains(**shape))
            {
                Some(shape) => shape,
                None => continue,
            }
        };
        found.push(format!(
            "riga {}: il capostipite confrontato con un valore scritto («{shape}»): {}",
            number + 1,
            line.trim()
        ));
    }
    found
}

/// The breaches in a text: line by line, the name found and the sign that makes
/// that line a decision.
fn decisions_on_a_product_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line).to_lowercase();
        let Some(product) = PRODUCT_NAMES.iter().find(|name| code.contains(**name)) else {
            continue;
        };
        let Some(sign) = SIGNS_OF_A_DECISION
            .iter()
            .find(|sign| code.contains(**sign))
        else {
            continue;
        };
        found.push(format!(
            "riga {}: «{product}» dentro una condizione («{sign}»): {}",
            number + 1,
            line.trim()
        ));
    }
    found
}

#[test]
fn no_product_name_appears_in_a_condition_of_the_tracking() {
    let sources = tracking_sources();
    assert!(
        sources.len() >= 4,
        "guardati {} sorgenti: la scansione non sta guardando dove crede",
        sources.len()
    );

    let mut broken: Vec<String> = Vec::new();
    for path in &sources {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
        for problem in decisions_on_a_product_in(&text) {
            broken.push(format!("{}: {problem}", path.display()));
        }
        for problem in the_label_compared_with_a_constant_in(&text) {
            broken.push(format!("{}: {problem}", path.display()));
        }
    }
    workspace::measured_against(
        sources.len(),
        "tracking sources read",
        PRODUCT_NAMES.len() + COMPARED_WITH_A_WRITTEN_VALUE.len(),
        "names and shapes a condition must not carry",
    );

    assert!(
        broken.is_empty(),
        "il tracciamento decide sul nome di un prodotto, e non deve:\n{}\n\n\
         Un nome di prodotto è un'etichetta: si stampa e si registra. \
         L'ancora è (tty, albero, capostipite), e il capostipite non si \
         interroga. Se serve distinguere un caso, distinguilo su ciò che il \
         caso ha di diverso, non su come si chiama chi lo ha aperto.",
        broken.join("\n")
    );
}

/// **WHOEVER MEASURES GETS MEASURED.** The detector must find the breach when it
/// is there, and leave the label alone when it is a label. Without these two
/// lines the test above would stay green even with the detector switched off.
#[test]
fn the_check_finds_a_violation_that_is_there_and_leaves_a_label_alone() {
    let forbidden = "    if ancestor.as_deref() == Some(\"Orca\") {\n";
    assert_eq!(
        decisions_on_a_product_in(forbidden).len(),
        1,
        "il rilevatore non vede una condizione su un nome di prodotto"
    );

    let allowed = "    println!(\"gira in Orca\");\n";
    assert!(
        decisions_on_a_product_in(allowed).is_empty(),
        "un'etichetta non è una decisione, e questa prova non deve vietarla"
    );

    let commented = "    let found = 1; // qui il capostipite risulta Orca\n";
    assert!(
        decisions_on_a_product_in(commented).is_empty(),
        "un commento parla dei prodotti quanto serve: non decide niente"
    );
}

/// **THE EIGHTEENTH NAME, WHICH THE LIST DOES NOT HAVE.** This is the whole
/// reason the shape check exists: a list is walked around by picking the entry
/// that is not in it, and `PRODUCT_NAMES` has seventeen entries.
#[test]
fn a_product_the_list_has_never_heard_of_is_caught_all_the_same() {
    let unheard_of = "    if ancestor.as_deref() == Some(\"Rossignol\") {\n";
    assert!(
        decisions_on_a_product_in(unheard_of).is_empty(),
        "premessa di questa prova: il nome non è nell'elenco, o non prova niente"
    );
    assert_eq!(
        the_label_compared_with_a_constant_in(unheard_of).len(),
        1,
        "il capostipite confrontato con una costante è il difetto, comunque si chiami"
    );

    for shape in [
        "    match ancestor.as_deref() { Some(\"Rossignol\") => 1, _ => 0 }\n",
        "    if ancestor.contains(\"Rossignol\") {\n",
        "    if ancestor.starts_with(\"Rossignol\") {\n",
    ] {
        assert_eq!(
            the_label_compared_with_a_constant_in(shape).len(),
            1,
            "forma non vista: {shape}"
        );
    }
}

/// And the shapes that **must** pass, or the check would be silenced on day one
/// by somebody who cannot write a legitimate line.
#[test]
fn recording_the_label_and_asking_whether_it_is_there_stay_allowed() {
    for allowed in [
        "        ancestor: Some(\"Whatever\".to_owned()),\n",
        "    if ancestor.is_none() { return \"sconosciuto\"; }\n",
        "    println!(\"capostipite: {ancestor}\");\n",
        "    row.ancestor = arrival.ancestor.clone();\n",
        // An assertion compares what was recorded, and must be able to.
        "        assert_eq!(row.ancestor.as_deref(), Some(\"Whatever\"));\n",
    ] {
        assert!(
            the_label_compared_with_a_constant_in(allowed).is_empty(),
            "accusata a torto una riga che registra invece di decidere: {allowed}"
        );
    }
}
