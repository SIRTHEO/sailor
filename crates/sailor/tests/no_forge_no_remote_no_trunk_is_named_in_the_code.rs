//! The twin of `no_engine_is_named_in_the_code`, for the other half of the
//! product: delivery never promised what ADR-001 promised about engines, so a
//! forge, a remote and a trunk are written down. Both counts may only fall.
//! ADR-020.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

const NAMED_IN_THE_CODE_TODAY: usize = 5;

/// Counted apart from the code: the larger half, and the slower to come down.
const NAMED_IN_THE_SHIPPED_FLOWS_TODAY: usize = 87;

const ATOMS: &[&str] = &["origin", "main", "master", "gh"];

/// **A NAME ALONE PROVES NOTHING.** `"main"` is SQLite's own schema in
/// `ledger`, `origin-(--radix-…)` is a CSS transform origin, and a column is
/// called `origin` too: 51 of the first 56 hits were noise. A name counts only
/// beside the thing that makes it a repository's fact.
const COMPANY: &[&str] = &["git", "gh"];

const REF_SHAPES: &[&str] = &["refs/heads/", "refs/remotes/", "origin/"];

/// **COMPANY IS NOT ALWAYS ON THE SAME LINE.** `let trunk = "main";` with the
/// `git` call two lines below is the shape this judge exists to catch, and a
/// line-by-line rule let it through: found by putting the defect back and
/// watching the judge stay green while the regression tests went red.
const WINDOW: usize = 2;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn is_source(name: &str) -> bool {
    if name.contains(".test.") || name == "tests.rs" {
        return false;
    }
    name.ends_with(".rs") || name.ends_with(".ts") || name.ends_with(".tsx")
}

fn is_shipped_flow(name: &str) -> bool {
    name.ends_with(".flow.json")
}

fn sources(under: &Path, out: &mut Vec<PathBuf>, keep: fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(under) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "examples" {
                sources(&path, out, keep);
            }
        } else if keep(&name) {
            out.push(path);
        }
    }
}

/// `origin/main` gives two names, `remaining` gives none.
fn words_of(line: &str) -> Vec<String> {
    line.split(|letter: char| !letter.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

/// Prose about the code is not the code: the rule bans a name from what runs.
fn is_prose(line: &str) -> bool {
    let start = line.trim_start();
    start.starts_with("//") || start.starts_with('#') || start.starts_with("* ") || start == "*"
}

/// **A SHIPPED FLOW'S SHELL IS A PROGRAM ON ONE JSON LINE**, whose newlines are
/// the two characters `\` and `n`: a file is cut on both.
fn newlines_of(text: &str) -> Vec<&str> {
    text.split('\n').flat_map(|line| line.split("\\n")).collect()
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The file with its test modules taken out, and what follows them put back.
///
/// **NOT CUT AT THE FIRST MARK, AND NOT COUNTED IN BRACES.** Cutting hid 3.311
/// of ledger's 3.341 lines, and those ship; counting braces reads the ones
/// inside a JSON string, which this tree is full of. A module ends at the lone
/// `}` in the column its `mod` stands in, which no string literal can be.
fn the_code_of(text: &str) -> Vec<&str> {
    let lines = newlines_of(text);
    let mut kept = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if !lines[at].trim_start().starts_with("#[cfg(test)]") {
            kept.push(lines[at]);
            at += 1;
            continue;
        }
        let mut head = at + 1;
        while head < lines.len() && lines[head].trim().is_empty() {
            head += 1;
        }
        let Some(opening) = lines.get(head) else {
            break;
        };
        if !opening.contains('{') {
            at = head + 1;
            continue;
        }
        let column = indent_of(opening);
        let mut close = head + 1;
        while close < lines.len()
            && !(lines[close].trim() == "}" && indent_of(lines[close]) == column)
        {
            close += 1;
        }
        at = close + 1;
    }
    kept
}

/// **A NAME GIVEN TO A SYMBOL KEEPS NO COMPANY.** `pub const TRUNK: &str =
/// "main";` stood in `branches.rs` with no call within two lines, and every
/// judgement on a branch name leaned on it, so a published constant holding a
/// bare name counts on its own. A `let` does not: `let schema = "main"` is
/// SQLite's own, and the control below holds it.
fn binds_a_name(line: &str) -> bool {
    let Some((left, right)) = line.split_once('=') else {
        return false;
    };
    if !["const ", "static "].iter().any(|word| left.contains(word)) {
        return false;
    }
    let value = right.trim();
    ATOMS.iter().any(|atom| {
        value.starts_with(&format!("\"{atom}\"")) || value.starts_with(&format!("'{atom}'"))
    })
}

fn named_in(text: &str) -> usize {
    let lines = the_code_of(text);
    let words: Vec<Vec<String>> = lines.iter().map(|line| words_of(line)).collect();
    let keeps_company = |at: usize| {
        !is_prose(lines[at])
            && words[at]
                .iter()
                .any(|word| COMPANY.contains(&word.as_str()))
    };
    let mut named = 0;
    for (at, line) in lines.iter().enumerate() {
        if is_prose(line) || !words[at].iter().any(|word| ATOMS.contains(&word.as_str())) {
            continue;
        }
        if REF_SHAPES.iter().any(|shape| line.contains(shape)) || binds_a_name(line) {
            named += 1;
            continue;
        }
        let from = at.saturating_sub(WINDOW);
        let to = (at + WINDOW).min(lines.len() - 1);
        if (from..=to).any(keeps_company) {
            named += 1;
        }
    }
    named
}

fn measure(under: &[PathBuf], keep: fn(&str) -> bool) -> (usize, Vec<(usize, PathBuf)>) {
    let root = root();
    let mut files = Vec::new();
    for place in under {
        sources(place, &mut files, keep);
    }
    workspace::measured_against(files.len(), "sources read", ATOMS.len(), "delivery names");
    let mut per_file = Vec::new();
    let mut total = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let count = named_in(&text);
        if count > 0 {
            total += count;
            per_file.push((
                count,
                file.strip_prefix(&root).unwrap_or(&file).to_path_buf(),
            ));
        }
    }
    per_file.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    (total, per_file)
}

fn told(per_file: &[(usize, PathBuf)]) -> String {
    per_file
        .iter()
        .map(|(count, file)| format!("{count:>4}  {}", file.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_control_a_name_counts_only_in_the_company_that_makes_it_a_repository_s_fact() {
    // The defect this judge exists for, in both halves of the product.
    assert_eq!(named_in(r#"git(&root, &["rev-parse", "main"])"#), 1);
    assert_eq!(named_in(r#""command": "git push origin main""#), 1);
    assert_eq!(named_in(r#""check": "gh pr list --state open""#), 1);
    assert_eq!(named_in(r#"let at = "refs/heads/main";"#), 1);
    assert_eq!(named_in(r#"let at = "origin/main";"#), 1);
    // The shape a line-by-line rule let through, with the call below it.
    assert_eq!(
        named_in("let trunk = \"main\".to_owned();\n    let commit = git(&root, &at)?;"),
        1
    );

    // The shape the window cannot see: a name given to a symbol, no call near.
    assert_eq!(named_in(r#"pub const TRUNK: &str = "main";"#), 1);
    assert_eq!(named_in(r#"const A_REMOTE: &str = "origin";"#), 1);
    assert_eq!(named_in(r#"const TRUNK = 'master';"#), 1);
    assert_eq!(named_in(r#"const NOT_A_TRUNK: &str = "main-thing";"#), 0);

    // The same words where they are not a repository's fact at all.
    assert_eq!(named_in(r#"let shown = if schema == "main" { name };"#), 0);
    assert_eq!(
        named_in(r#"className="origin-(--radix-transform-origin)""#),
        0
    );
    assert_eq!(named_in(r#""kind,name,path,origin,reach,reason""#), 0);
    assert_eq!(named_in(r#"say("window.flows.column.origin")"#), 0);
    assert_eq!(
        named_in(r#"let a = "remaining"; let b = "the original";"#),
        0
    );
    // Company that is prose does not make a name a repository's fact.
    assert_eq!(
        named_in("// this is what git does\nlet schema = \"main\";"),
        0
    );

    // Prose about the rule is not a breach of it; a test module is not code.
    assert_eq!(named_in("// git push origin main is what this replaces"), 0);
    assert_eq!(
        named_in("let x = 1;\n#[cfg(test)]\nmod tests {\n    let y = \"git checkout main\";\n}"),
        0
    );
    // And the file goes on after the module: those lines ship.
    assert_eq!(
        named_in("#[cfg(test)]\nmod tests {\n    let a = 1;\n}\nlet y = \"git checkout main\";"),
        1
    );
    // A module declared in a file beside this one has no body to skip.
    assert_eq!(
        named_in("#[cfg(test)]\nmod tests;\nlet y = \"git checkout main\";"),
        1
    );

    // A shell block written on one JSON line is cut on its own newlines.
    assert_eq!(
        named_in(r#""command": "set -e\ngit fetch origin\ngit rev-parse main\necho done""#),
        2
    );
}

#[test]
fn no_new_forge_remote_or_trunk_is_named_in_the_code() {
    let root = root();
    let (total, per_file) = measure(
        &[
            root.join("crates"),
            root.join("desktop/src-tauri/src"),
            root.join("desktop/src"),
        ],
        is_source,
    );
    match weigh(NAMED_IN_THE_CODE_TODAY, total) {
        Weighed::TreeIsAbove(more) => panic!(
            "a forge, a remote or a trunk named in the code: {total} ({more} more than the \
             seed's {NAMED_IN_THE_CODE_TODAY}). Which repository this is belongs in its own \
             declaration, not in the product (ADR-020).\n{}",
            told(&per_file)
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {NAMED_IN_THE_CODE_TODAY} and the tree holds {total}, {apart} \
             apart: somebody pruned without re-measuring; write {total}\n{}",
            told(&per_file)
        ),
        Weighed::Holds => {}
    }
}

#[test]
fn no_new_forge_remote_or_trunk_is_named_in_a_shipped_flow() {
    let root = root();
    let (total, per_file) = measure(&[root.join("crates/flow/system")], is_shipped_flow);
    match weigh(NAMED_IN_THE_SHIPPED_FLOWS_TODAY, total) {
        Weighed::TreeIsAbove(more) => panic!(
            "a forge, a remote or a trunk named in a shipped flow: {total} ({more} more than \
             the seed's {NAMED_IN_THE_SHIPPED_FLOWS_TODAY}). A flow the product ships runs in \
             somebody else's repository too (ADR-020).\n{}",
            told(&per_file)
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {NAMED_IN_THE_SHIPPED_FLOWS_TODAY} and the tree holds {total}, \
             {apart} apart: somebody pruned without re-measuring; write {total}\n{}",
            told(&per_file)
        ),
        Weighed::Holds => {}
    }
}
