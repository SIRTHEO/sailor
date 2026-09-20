//! The fourth half of the same rule. ADR-001 says no engine is named, ADR-013
//! says a capability is data, ADR-020 says a forge, a remote and a trunk are
//! declared. ADR-021 says the tools standing on one person's machine are too.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

const NAMED_IN_THE_CODE_TODAY: usize = 4;

/// **WHAT SAILOR IS BUILT FROM IS NOT SOMEBODY'S WORKBENCH.** `CARGO_TARGET_DIR`
/// and `Cargo.toml` are this product's own build, `./StepNode` is a React
/// component and not the `node` descriptor: counting them gave 30 hits of which
/// 26 were noise, and a judge nobody believes is a judge nobody reads.
const BUILT_WITH: &[&str] = &[
    "cargo", "node", "npm", "pnpm", "bun", "python3", "git", "gh", "make", "just", "curl", "jq",
];

/// A shorter name matches too much prose to tell anything.
const TOO_SHORT_TO_TELL: usize = 4;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

/// The tools the product declares, which is the list this judge watches for:
/// add a tool to the descriptor and the judge starts watching its name.
fn the_tools_declared() -> Vec<String> {
    let catalog: serde_json::Value =
        serde_json::from_str(toolbox::descriptor::BUILTIN).expect("the shipped catalog");
    catalog["tools"]
        .as_array()
        .expect("the tools")
        .iter()
        .filter_map(|tool| tool["id"].as_str())
        .filter(|id| id.len() >= TOO_SHORT_TO_TELL && !BUILT_WITH.contains(id))
        .map(str::to_owned)
        .collect()
}

fn is_source(name: &str) -> bool {
    if name.contains(".test.") || name == "tests.rs" {
        return false;
    }
    name.ends_with(".rs") || name.ends_with(".ts") || name.ends_with(".tsx")
}

fn sources(under: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(under) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "examples" && name != "descriptors" {
                sources(&path, out);
            }
        } else if is_source(&name) {
            out.push(path);
        }
    }
}

/// `SOCRATICODE_PROJECT_ID` holds `socraticode`; `./StepNode` does not hold
/// `node`, because a letter stands on either side of it. Hyphen and underscore
/// are the same character here: `claude-code` is what `CLAUDE_CODE_SESSION_ID`
/// names.
fn holds(literal: &str, id: &str) -> bool {
    let same = |a: u8, b: u8| {
        let fold = |c: u8| match c {
            b'_' => b'-',
            other => other.to_ascii_lowercase(),
        };
        fold(a) == fold(b)
    };
    let text = literal.as_bytes();
    let name = id.as_bytes();
    let apart = |c: u8| !c.is_ascii_alphanumeric();
    text.windows(name.len())
        .enumerate()
        .filter(|(_, window)| window.iter().zip(name).all(|(a, b)| same(*a, *b)))
        .any(|(at, _)| {
            let before = at == 0 || apart(text[at - 1]);
            let after = at + name.len() == text.len() || apart(text[at + name.len()]);
            before && after
        })
}

/// A tool's configuration is what the product must not know: an environment
/// variable, a dotfile, a path. The bare word is the tool itself, which a
/// descriptor is allowed to be about.
fn is_a_configuration(literal: &str) -> bool {
    literal.starts_with('.')
        || literal.starts_with('$')
        || literal.contains('/')
        || (literal.chars().any(|c| c.is_ascii_uppercase())
            && !literal.chars().any(|c| c.is_ascii_lowercase()))
}

fn literals_of(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            break;
        };
        found.push(&after[..close]);
        rest = &after[close + 1..];
    }
    found
}

fn is_prose(line: &str) -> bool {
    let start = line.trim_start();
    start.starts_with("//") || start.starts_with('#') || start.starts_with("* ") || start == "*"
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
    let lines: Vec<&str> = text.lines().collect();
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

fn named_in(text: &str, tools: &[String]) -> usize {
    the_code_of(text)
        .into_iter()
        .filter(|line| !is_prose(line))
        .flat_map(literals_of)
        .filter(|literal| is_a_configuration(literal))
        .filter(|literal| tools.iter().any(|id| holds(literal, id)))
        .count()
}

fn measure() -> (usize, Vec<(usize, PathBuf)>) {
    let root = root();
    let tools = the_tools_declared();
    let mut files = Vec::new();
    for place in [
        root.join("crates"),
        root.join("desktop/src-tauri/src"),
        root.join("desktop/src"),
    ] {
        sources(&place, &mut files);
    }
    workspace::measured_against(files.len(), "sources read", tools.len(), "declared tools");
    let mut per_file = Vec::new();
    let mut total = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let count = named_in(&text, &tools);
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

#[test]
fn the_control_a_tool_counts_only_where_it_is_a_machine_s_configuration() {
    let tools: Vec<String> = ["socraticode", "claude-code", "docker"]
        .iter()
        .map(|id| (*id).to_owned())
        .collect();

    // The defect this judge exists for, in the three shapes it took.
    assert_eq!(named_in(r#"const F: &str = ".socraticode.json";"#, &tools), 1);
    assert_eq!(named_in(r#"var("SOCRATICODE_PROJECT_ID")"#, &tools), 1);
    assert_eq!(named_in(r#"let at = "$CLAUDE_CODE_SESSION_ID";"#, &tools), 1);

    // The tool as a tool, which a descriptor is allowed to be about.
    assert_eq!(named_in(r#"resolve("socraticode")"#, &tools), 0);
    assert_eq!(named_in(r#"if tool.id == "docker" { run() }"#, &tools), 0);

    // A word that only contains the name is not the name.
    assert_eq!(named_in(r#"import Step from "./StepNode";"#, &tools), 0);
    assert_eq!(named_in(r#"let at = "/usr/local/dockerfiles";"#, &tools), 0);

    // Prose about the rule is not a breach of it; a test module is not code.
    assert_eq!(named_in(r#"// ".socraticode.json" is what it was"#, &tools), 0);
    assert_eq!(
        named_in("#[cfg(test)]\nmod tests {\n    let y = \".socraticode.json\";\n}", &tools),
        0
    );
    // A module declared in a file beside this one has no body to skip.
    assert_eq!(
        named_in("#[cfg(test)]\nmod tests;\nconst F: &str = \".socraticode.json\";", &tools),
        1
    );
    // And the file goes on after the module: those lines ship.
    assert_eq!(
        named_in(
            "#[cfg(test)]\nmod tests {\n    let y = \".socraticode.json\";\n}\nconst F: &str = \".socraticode.json\";",
            &tools
        ),
        1
    );
}

#[test]
fn the_judge_watches_the_tools_the_descriptor_declares() {
    let tools = the_tools_declared();
    assert!(
        tools.iter().any(|id| id == "socraticode"),
        "the descriptor declares socraticode, so the judge watches it: {tools:?}"
    );
    assert!(
        !tools.iter().any(|id| id == "cargo"),
        "what Sailor is built from is not somebody's workbench: {tools:?}"
    );
}

#[test]
fn no_new_workbench_tool_is_named_in_the_code() {
    let (total, per_file) = measure();
    let told = per_file
        .iter()
        .map(|(count, file)| format!("{count:>4}  {}", file.display()))
        .collect::<Vec<_>>()
        .join("\n");
    match weigh(NAMED_IN_THE_CODE_TODAY, total) {
        Weighed::TreeIsAbove(more) => panic!(
            "a tool of somebody's workbench named in the code: {total} ({more} more than the \
             seed's {NAMED_IN_THE_CODE_TODAY}). Which tools stand on a machine is that machine's \
             fact, declared in a descriptor, and a capability nobody declares triggers nothing \
             (ADR-021).\n{told}"
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {NAMED_IN_THE_CODE_TODAY} and the tree holds {total}, {apart} apart: \
             somebody pruned without re-measuring; write {total}\n{told}"
        ),
        Weighed::Holds => {}
    }
}
