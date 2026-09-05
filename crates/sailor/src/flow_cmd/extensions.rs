//! What a step needs installed beside the engine — a skill, an agent, a
//! command — and whether this machine has it. A mandate that leans on one
//! nobody declared runs worse elsewhere and says nothing: see fault 17.

use flow::{reference, FlowFile};
use inventory::{Inventory, Kind, Reach};
use serde_json::Value;
use std::collections::BTreeSet;

/// The field of `with` a step declares them in, each written `kind:name` in
/// the words `sailor inventory` prints: `skill:<name>`, `command:/<name>`.
pub(super) const NEEDS_EXTENSIONS: &str = "needs_extensions";

/// The path segment a mandate names a skill by: `skills/<name>/SKILL.md`.
const SKILLS_SEGMENT: &str = "skills/";

/// What may open a slash command in prose besides a space. A `/` glued to a
/// letter is a path, glued to a quote it is a pointer quoted as an example.
const OPENS_A_WORD: &[char] = &['(', '`', '«', '‹'];

/// One need a step declares, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WantedExtension {
    pub(super) step: String,
    /// `None` when the entry was not written `kind:name`, or names a kind the
    /// inventory does not keep.
    pub(super) kind: Option<Kind>,
    pub(super) name: String,
    pub(super) written: String,
}

/// The needs every step declares, read where a person writes: the `with`, and
/// the input declared for a step without dependencies.
pub(super) fn extensions_wanted(flow: &FlowFile) -> Vec<WantedExtension> {
    let mut wanted = Vec::new();
    for step in flow.graph.steps() {
        for place in [step.with.as_ref(), flow.inputs.get(&step.id)] {
            let Some(declared) = place.and_then(|value| value.get(NEEDS_EXTENSIONS)) else {
                continue;
            };
            for written in names_in(declared) {
                let (kind, name) = match written.split_once(':') {
                    Some((kind, name)) => (Kind::from_label(kind), name.to_owned()),
                    None => (None, written.clone()),
                };
                wanted.push(WantedExtension {
                    step: step.id.clone(),
                    kind,
                    name,
                    written,
                });
            }
        }
    }
    wanted
}

/// One name written bare, or every string of a list.
fn names_in(declared: &Value) -> Vec<String> {
    match declared {
        Value::String(one) => vec![one.clone()],
        Value::Array(many) => many
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// How a declared need stands on this machine.
enum Standing<'a> {
    Present(&'a str),
    Unreachable(&'a str),
    Unsure(&'a str),
    Absent,
}

/// Reachable wins over unsure, unsure over unreachable: a skill kept in two
/// places is present when one of them loads.
fn standing<'a>(installed: &'a Inventory, kind: Kind, name: &str) -> Standing<'a> {
    let mut best = Standing::Absent;
    let matching = installed
        .entries
        .iter()
        .filter(|entry| entry.kind == kind && entry.name == name);
    for entry in matching {
        match &entry.reach {
            Reach::Active => return Standing::Present(&entry.origin),
            Reach::Unknown(why) => best = Standing::Unsure(why),
            Reach::Inactive(why) => {
                if matches!(best, Standing::Absent) {
                    best = Standing::Unreachable(why);
                }
            }
        }
    }
    best
}

/// Writes how each declared need stands here. **AN ABSENCE IS A WARNING THAT
/// NAMES THE STEP AND THE NEED, NEVER AN ERROR**: a flow written where the
/// skill is stays sound, and whoever lacks it must work worse, not silently.
pub(super) fn extensions_into(report: &mut String, flow: &FlowFile, installed: &Inventory) {
    let mut present = Vec::new();
    let mut absent = Vec::new();
    let mut unreachable = Vec::new();
    let mut unsure = Vec::new();
    let mut unread = Vec::new();
    for wanted in extensions_wanted(flow) {
        let who = format!("{}: {}", wanted.step, wanted.written);
        let Some(kind) = wanted.kind else {
            unread.push(who);
            continue;
        };
        match standing(installed, kind, &wanted.name) {
            Standing::Present(origin) => present.push(format!("{who} ({origin})")),
            Standing::Unreachable(why) => unreachable.push(format!("{who} — {why}")),
            Standing::Unsure(why) => unsure.push(format!("{who} — {why}")),
            Standing::Absent => absent.push(who),
        }
    }
    if !present.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_present",
            &[("list", &present.join("; "))],
        ));
    }
    if !absent.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_absent",
            &[("list", &absent.join("; "))],
        ));
    }
    if !unreachable.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_unreachable",
            &[("list", &unreachable.join("; "))],
        ));
    }
    if !unsure.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_unsure",
            &[("list", &unsure.join("; "))],
        ));
    }
    if !unread.is_empty() {
        let kinds: Vec<&str> = Kind::ALL.iter().map(|kind| kind.label()).collect();
        report.push_str(&catalogue::say(
            "cli.flow.extensions_without_a_kind",
            &[("list", &unread.join("; ")), ("kinds", &kinds.join(", "))],
        ));
    }
}

/// The same, asked of this machine — and only when a step declares something:
/// the inventory is a walk of the home, and a flow declaring nothing gives it
/// nothing to compare.
pub(super) fn extensions_of_this_machine_into(report: &mut String, flow: &FlowFile) {
    if extensions_wanted(flow).is_empty() {
        return;
    }
    let survey = inventory::default_roots(ledger::sailor_home().as_deref());
    extensions_into(report, flow, &inventory::collect_survey(&survey));
}

// ── what the text leans on without declaring it ──────────────────────────

/// A skill or command a step's text names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NamedExtension {
    pub(super) step: String,
    pub(super) field: String,
    pub(super) name: String,
}

/// The names a step's text leans on and `needs_extensions` does not declare.
/// Two shapes are read, the two the fault was found in: a skill by its file,
/// `skills/<name>/SKILL.md`, and a slash command, `/<name>`, opening a word.
pub(super) fn undeclared_extensions_named_in_text(flow: &FlowFile) -> Vec<NamedExtension> {
    let wanted = extensions_wanted(flow);
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        let declared: BTreeSet<&str> = wanted
            .iter()
            .filter(|need| need.step == step.id)
            .map(|need| need.name.trim_start_matches('/'))
            .collect();
        let places = [step.with.as_ref(), flow.inputs.get(&step.id)];
        for place in places.into_iter().flatten() {
            walk_text("", place, &mut |field, text| {
                for name in extensions_named_in(text) {
                    if !declared.contains(name.as_str()) {
                        found.push(NamedExtension {
                            step: step.id.clone(),
                            field: field.to_owned(),
                            name,
                        });
                    }
                }
            });
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every piece of text a person wrote, with the field it sits in. The value of
/// a pointer is skipped — it opens with `/` and names an output, not a skill —
/// and so is the declaration itself; a `$join` is the field's own text in parts.
fn walk_text(field: &str, value: &Value, visit: &mut dyn FnMut(&str, &str)) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields {
                if key == reference::FROM_KEY || key == reference::JSON_KEY || key == NEEDS_EXTENSIONS
                {
                    continue;
                }
                let trail = if field.is_empty() {
                    key.clone()
                } else if key == reference::JOIN_KEY {
                    field.to_owned()
                } else {
                    format!("{field}.{key}")
                };
                walk_text(&trail, inner, visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_text(field, item, visit);
            }
        }
        Value::String(text) => visit(field, text),
        _ => {}
    }
}

/// The skill and command names a piece of text carries.
pub(super) fn extensions_named_in(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(SKILLS_SEGMENT) {
        rest = &rest[at + SKILLS_SEGMENT.len()..];
        let name = leading_name(rest);
        if !name.is_empty() {
            names.push(name.to_owned());
        }
    }
    let mut before: Option<char> = None;
    for (at, letter) in text.char_indices() {
        let opens_a_word = before.is_none_or(|b| b.is_whitespace() || OPENS_A_WORD.contains(&b));
        before = Some(letter);
        if letter != '/' || !opens_a_word {
            continue;
        }
        let name = leading_name(&text[at + 1..]);
        // A path goes on past its first segment; a command does not.
        if name.is_empty() || text[at + 1 + name.len()..].starts_with('/') {
            continue;
        }
        names.push(name.to_owned());
    }
    names
}

/// The word a name is: opened by a small letter, then letters, digits, dashes
/// and underscores. `/Users` and `/<field>` are not names.
fn leading_name(text: &str) -> &str {
    if !text.starts_with(|c: char| c.is_ascii_lowercase()) {
        return "";
    }
    let end = text
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::super::check::check_report;
    use super::*;
    use inventory::{collect, Root};
    use registry::default_registry;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn flow_of(action: &str, with: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "d",
                "graph": {{"steps": [{{
                    "id": "root", "deps": [], "action": "{action}", "max_attempts": 1,
                    "when": null, "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}},
                    "with": {with}
                }}], "skippable_dependencies": []}},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("the flow loads")
    }

    fn flow_with(with: &str) -> FlowFile {
        flow_of("external_engine", with)
    }

    /// A throwaway directory that carries the run that dug it.
    fn scratch(what: &str) -> PathBuf {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sailor-extensions-{what}-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        dir
    }

    fn write_skill(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).expect("the skill's directory");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\n"),
        )
        .expect("the skill file");
    }

    /// A home holding these skills where the shipped descriptor says the
    /// command line keeps them: the test names no product.
    fn home_with_skills(names: &[&str]) -> PathBuf {
        let home = scratch("home");
        let product = inventory::extensions::declared(None)
            .into_iter()
            .next()
            .expect("the shipped descriptor declares a command line");
        let place = product.skills.first().expect("and where it keeps skills");
        for name in names {
            write_skill(&home.join(&place.under).join(name), name);
        }
        home
    }

    /// The two sides of fault 17 in one report: the skill this machine has is
    /// said present, the one it lacks is a warning that names step and skill.
    #[test]
    fn a_declared_need_is_said_present_or_absent_with_its_step() {
        let home = home_with_skills(&["here"]);
        let installed = collect(&[Root::home(&home)]);
        let flow = flow_with(
            r#"{"tool": "x", "needs_extensions": ["skill:here", "skill:elsewhere"], "timeout_secs": 1}"#,
        );

        let mut report = String::new();
        extensions_into(&mut report, &flow, &installed);

        let present = catalogue::say(
            "cli.flow.extensions_present",
            &[("list", "root: skill:here (home)")],
        );
        assert!(report.contains(&present), "{report}");
        let absent = catalogue::say(
            "cli.flow.extensions_absent",
            &[("list", "root: skill:elsewhere")],
        );
        assert!(report.contains(&absent), "{report}");
    }

    /// A flow declaring nothing adds nothing: a report that spoke of needs
    /// nobody wrote would send the reader after a field that is not there.
    #[test]
    fn a_flow_declaring_nothing_adds_no_line() {
        let home = home_with_skills(&["here"]);
        let installed = collect(&[Root::home(&home)]);
        let flow = flow_with(r#"{"tool": "x", "timeout_secs": 1}"#);

        let mut report = String::new();
        extensions_into(&mut report, &flow, &installed);

        assert!(report.is_empty(), "{report}");
    }

    /// A skill on disk that no configuration loads is not absent, and saying
    /// «absent» would send the reader to install what is already there.
    #[test]
    fn a_skill_nobody_loads_is_unreachable_and_told_apart_from_absent() {
        let shed = scratch("shed");
        write_skill(&shed.join("parked"), "parked");
        let installed = collect(&[Root::warehouse("shed", &shed)]);
        let flow = flow_with(r#"{"needs_extensions": "skill:parked", "timeout_secs": 1}"#);

        let mut report = String::new();
        extensions_into(&mut report, &flow, &installed);

        assert!(
            report.contains("root: skill:parked — it sits in a directory no configuration loads"),
            "{report}"
        );
        let absent = catalogue::say("cli.flow.extensions_absent", &[("list", "root: skill:parked")]);
        assert!(!report.contains(&absent), "{report}");
    }

    /// A need without its kind is named as such, and nothing is looked for:
    /// guessing «skill» would let a typo pass as a measured absence.
    #[test]
    fn a_need_written_without_a_kind_is_named_and_not_looked_for() {
        let home = home_with_skills(&["here"]);
        let installed = collect(&[Root::home(&home)]);
        let flow = flow_with(r#"{"needs_extensions": ["here", "plugin:here"], "timeout_secs": 1}"#);

        let mut report = String::new();
        extensions_into(&mut report, &flow, &installed);

        let said = catalogue::say(
            "cli.flow.extensions_without_a_kind",
            &[
                ("list", "root: here; root: plugin:here"),
                ("kinds", "skill, agent, command, rule, hook"),
            ],
        );
        assert!(report.contains(&said), "{report}");
        assert!(!report.contains("(home)"), "nothing was looked for: {report}");
    }

    /// The field belongs to both specs that carry a mandate, or the check
    /// would call an honest declaration a typo.
    #[test]
    fn declaring_needs_is_not_a_stray_field() {
        let registry = default_registry(None, None);
        let engine = r#"{"tool": "x", "needs_extensions": ["skill:one"], "timeout_secs": 1}"#;
        let handed = r#"{"mandate": "m", "holder": "h", "handoff_timeout_secs": 1,
            "needs_extensions": ["skill:one"], "options": [{"label": "done"}]}"#;
        for (action, with) in [("external_engine", engine), ("handed_to_agent", handed)] {
            let (report, _) = check_report(&flow_of(action, with), &registry, None, None);
            assert!(
                !report.contains("campi che l'azione non conosce"),
                "{action}: {report}"
            );
        }
    }

    /// The two shapes the fault was found in: a skill by its file, and a slash
    /// command opening a word.
    #[test]
    fn a_skill_named_by_its_file_or_as_a_slash_command_is_read_from_the_text() {
        let names = extensions_named_in(
            "the way is in skills/first-one/SKILL.md; then run /second-one and (/third) — done",
        );
        assert_eq!(names, vec!["first-one", "second-one", "third"]);
    }

    /// What is not a skill name: a pointer, a path, a slash inside a word, an
    /// example quoted from the pointer rule. Each would have been a false alarm
    /// on a shipped flow, and a warning that cries wolf is switched off.
    #[test]
    fn a_pointer_a_path_and_a_quoted_example_are_not_skill_names() {
        let text = "read /answer/verdict and crates/flow/system/ and either/or; the text is \
                    \"/text\"; https://x/y; /Users/x; the skills/ directory";
        assert_eq!(extensions_named_in(text), Vec::<String>::new());
    }

    /// A step that declares what it names is clean; the same name undeclared
    /// is reported with its field, so the fix is one line away.
    #[test]
    fn a_named_skill_is_reported_only_when_the_step_does_not_declare_it() {
        let declaring = flow_with(
            r#"{"stdin": "follow skills/first/SKILL.md and run /second",
                "needs_extensions": ["skill:first", "command:/second"], "timeout_secs": 1}"#,
        );
        assert_eq!(undeclared_extensions_named_in_text(&declaring), Vec::new());

        let silent = flow_with(
            r#"{"stdin": {"$join": ["follow skills/first/SKILL.md", {"$from": "/text"}]},
                "timeout_secs": 1}"#,
        );
        let found = undeclared_extensions_named_in_text(&silent);
        assert_eq!(
            found,
            vec![NamedExtension {
                step: "root".to_owned(),
                field: "stdin".to_owned(),
                name: "first".to_owned(),
            }]
        );
    }

    /// And the check says it, before the run.
    #[test]
    fn the_check_names_what_a_step_leans_on_without_declaring() {
        let flow = flow_with(r#"{"stdin": "run /first", "timeout_secs": 1}"#);

        let (report, _) = check_report(&flow, &default_registry(None, None), None, None);

        let said = catalogue::say(
            "cli.flow.extensions_named_not_declared",
            &[("fields", "root in «stdin» (first)")],
        );
        assert!(report.contains(&said), "{report}");
    }

    /// The judge over what ships: a shipped flow leaning on a skill nobody
    /// declared is fault 17 inside the binary, where the user cannot fix it.
    #[test]
    fn no_shipped_flow_names_a_skill_it_does_not_declare() {
        for (name, text) in flow::system::FLOWS {
            let flow: FlowFile = serde_json::from_str(text)
                .unwrap_or_else(|why| panic!("the shipped flow «{name}» does not load: {why}"));
            let found = undeclared_extensions_named_in_text(&flow);
            assert!(
                found.is_empty(),
                "«{name}» leans on what it does not declare: {found:?}"
            );
        }
    }
}
