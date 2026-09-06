//! The window and the engine have **one action list**, and something measures it.
//!
//! **THIS TEST LIVES IN `crates/sailor`, NOT IN `desktop/`.** `desktop/src-tauri`
//! declares an empty `[workspace]`: it sits outside the Rust workspace on
//! purpose, so no `cargo test --workspace` compiles a test written there and it
//! could never go red. `crates/sailor` is a workspace member, and the gate runs
//! it.
//!
//! **THE ANCHOR IS NEITHER COPY.** The engine's action names are taken by
//! **running** `registry::default_registry` and asking it `names()`, never
//! transcribed: two hand-written lists drift wrong together.
//!
//! **THE PRICE, DECLARED.** It reads text from a `.ts` file and does not parse
//! it, so a map written in another shape escapes it. Every case therefore
//! asserts it read something before it judges: a failed read must be red, not
//! silently green.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

fn window_vocabulary() -> String {
    let path = repository_root().join("desktop/src/flow.ts");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()))
}

/// A counter, and not the clock alone: it comes from fault 21. `cargo test`
/// runs the tests in one process and macOS's clock has no nanosecond
/// resolution, so two directories born in the same instant stole each other's
/// place.
static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let serial = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "sailor-action-list-{}-{serial}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The names the engine really registers, asked of the registry rather than
/// copied. **THE STORE MATTERS**: two actions register only when there is one,
/// and the window is right to draw them all the same. The house is this test's
/// own, so nothing of the machine's home or tools decides the list.
fn engine_action_names(label: &str) -> BTreeSet<String> {
    let dir = scratch_dir(label);
    let ledger = ledger::Ledger::open(&dir).expect("un deposito di prova si apre");
    registry::registry_in(registry::House::under(&dir), Some(ledger), None)
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The `key: value` pairs of an object literal in `flow.ts`.
///
/// It skips comment lines — the map carries them, and must be free to — and
/// stops at the closing brace. Quotes are stripped on both sides, because
/// TypeScript puts them on keys only where they are needed.
fn object_entries(source: &str, marker: &str) -> Vec<(String, String)> {
    let from = source
        .find(marker)
        .unwrap_or_else(|| panic!("«{marker}» non si trova in desktop/src/flow.ts"));
    let body = &source[from..];
    let open = body
        .find('{')
        .unwrap_or_else(|| panic!("«{marker}» non apre nessun oggetto"));

    let mut entries = Vec::new();
    for line in body[open + 1..].lines() {
        let line = line.trim();
        if line.starts_with('}') {
            break;
        }
        if line.starts_with("//") || line.starts_with('*') || line.starts_with("/*") {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_owned();
        let value = value
            .trim()
            .trim_end_matches(',')
            .trim()
            .trim_matches('"')
            .to_owned();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        entries.push((key, value));
    }
    entries
}

/// The action names the window knows: the keys of `ACTION_KIND`.
fn window_action_names(source: &str) -> BTreeSet<String> {
    object_entries(source, "const ACTION_KIND")
        .into_iter()
        .map(|(action, _kind)| action)
        .collect()
}

/// **NO INVENTED NAME IN THE WINDOW.** A name the engine does not register is
/// not a button doing nothing: it is a node you draw, move and wire, and then
/// **cannot save** — «the flow uses actions the engine does not know». The
/// defect never appears until somebody presses that key.
#[test]
fn the_window_names_no_action_the_engine_does_not_register() {
    let source = window_vocabulary();
    let named = window_action_names(&source);
    assert!(
        named.len() > 4,
        "il vocabolario della finestra non è stato letto: {} nomi trovati. \
         Una lettura fallita non deve passare per un confronto riuscito",
        named.len()
    );

    let registered = engine_action_names("inventate");
    workspace::measured_against(
        named.len(),
        "action names the window declares",
        registered.len(),
        "actions the engine registers",
    );
    let invented: Vec<&String> = named.difference(&registered).collect();
    assert!(
        invented.is_empty(),
        "la finestra nomina {} azioni che il motore non registra: {:?}. \
         Un nodo con uno di questi nomi si disegna e poi non si salva",
        invented.len(),
        invented
    );
}

/// **AND THE OTHER DIRECTION, WHERE THE WORSE DEFECT HID.** `kindOf` falls back
/// to `verifica` for a name it does not know: an engine action with no family
/// raises no error at all, it just draws the wrong node. One direction alone
/// would leave this test green while half the vocabulary is missing.
#[test]
fn every_engine_action_has_a_family_in_the_window() {
    let source = window_vocabulary();
    let named = window_action_names(&source);
    assert!(
        named.len() > 4,
        "il vocabolario della finestra non è stato letto: {} nomi trovati",
        named.len()
    );

    let registered = engine_action_names("senza-famiglia");
    workspace::measured_against(
        registered.len(),
        "actions the engine registers",
        named.len(),
        "action names the window declares",
    );
    let orphans: Vec<&String> = registered.difference(&named).collect();
    assert!(
        orphans.is_empty(),
        "il motore registra {} azioni a cui la finestra non dà una famiglia: {:?}. \
         Un nodo di questi si disegna come «verifica», in silenzio",
        orphans.len(),
        orphans
    );
}

/// **WHAT THE PALETTE CREATES MUST EXIST.** `DEFAULT_ACTION_FOR_KIND` is the
/// name a step is born with when pressed in the palette: the first list anyone
/// touches, and its values are action names like the rest.
#[test]
fn every_action_the_palette_creates_is_registered() {
    let source = window_vocabulary();
    let created: BTreeSet<String> = object_entries(&source, "const DEFAULT_ACTION_FOR_KIND")
        .into_iter()
        .map(|(_kind, action)| action)
        .collect();
    assert!(
        created.len() > 2,
        "la cassetta dei passi non è stata letta: {} azioni trovate",
        created.len()
    );

    let registered = engine_action_names("cassetta");
    workspace::measured_against(
        created.len(),
        "actions the palette creates",
        registered.len(),
        "actions the engine registers",
    );
    let invented: Vec<&String> = created.difference(&registered).collect();
    assert!(
        invented.is_empty(),
        "la cassetta crea {} passi con un'azione che il motore non registra: {:?}",
        invented.len(),
        invented
    );
}

/// **THE SHELL ASKS THE ENGINE FOR THE LIST, IT BUILDS NONE OF ITS OWN.** This
/// is the third copy of the registry, the one `save_flow` asks whether a flow
/// can be saved; built by hand from three chosen lines, it refused five actions
/// that run fine from the terminal. It is guarded from here because
/// `desktop/src-tauri` declares an empty `[workspace]` and no gate compiles it.
/// The mutant that fells it: `actions::register_default(&mut r)` put back in.
#[test]
fn the_window_shell_asks_the_engine_for_its_action_list() {
    let path = repository_root().join("desktop/src-tauri/src/flows.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));

    let from = source
        .find("fn action_registry_with(")
        .expect("il guscio costruisce il registro in `action_registry_with`");
    let body = &source[from..];
    let end = body.find("\n}").expect("la funzione si chiude");
    let body = &body[..end];

    assert!(
        body.contains("registry::default_registry"),
        "il guscio si costruisce il registro da sé invece di chiederlo al \
         motore: è la terza copia del guasto 10, e la finestra tornerebbe a \
         rifiutare al salvataggio azioni che dal terminale girano.\n{body}"
    );
    assert!(
        !body.contains("register_default(&mut"),
        "il guscio registra azioni a mano dentro `action_registry_with`: \
         qualunque riga scelta lì è una lista in più da tenere allineata.\n{body}"
    );
}

/// **AND THE POLICY TRAVELS THE SAME ROAD AS THE NAMES.** The window now draws,
/// per action, how the engine treats redoing it and whether it may close a run.
/// Written as a map beside the registry those two are fault 10 again, in the
/// same file that has already been its fifth copy; asked of the action itself
/// they cannot drift. The mutant that fells it: a `match name` in
/// `engine_actions` instead of `action.species()`.
#[test]
fn the_shell_asks_each_action_for_its_own_policy() {
    let path = repository_root().join("desktop/src-tauri/src/flows.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));

    let from = source
        .find("pub(crate) fn engine_actions(")
        .expect("the shell exposes `engine_actions`");
    let body = &source[from..];
    let end = body.find("\n}").expect("the function closes");
    let body = &body[..end];

    for asked in ["action.species()", "action.is_a_check()"] {
        assert!(
            body.contains(asked),
            "`engine_actions` never asks the action «{asked}»: a policy written \
             by hand beside the registry is fault 10.\n{body}"
        );
    }
}

/// **AND THE ANSWER REALLY VARIES.** A policy the window draws from a call that
/// always returns the same thing is a column of one word: the page would read
/// as measured while measuring nothing. Asked of the registry that runs, and
/// asked for both of the two answers a person acts on.
#[test]
fn the_engine_does_not_give_every_action_the_same_policy() {
    let dir = scratch_dir("policies");
    let ledger = ledger::Ledger::open(&dir).expect("a scratch store opens");
    let registry = registry::registry_in(registry::House::under(&dir), Some(ledger), None);

    let mut species = BTreeSet::new();
    let mut checks = 0usize;
    for name in registry.names() {
        let action = registry.get(name).expect("a registered name has its action");
        species.insert(format!("{:?}", action.species()));
        if action.is_a_check() {
            checks += 1;
        }
    }

    workspace::measured_against(
        species.len(),
        "distinct redo policies the engine declares",
        registry.names().len(),
        "actions the engine registers",
    );
    assert!(
        species.len() > 1,
        "the engine gives every action the same redo policy ({species:?}): \
         the column the window draws would say one word"
    );
    assert!(
        checks > 0 && checks < registry.names().len(),
        "«may close a run» is {checks} out of {}: a constant column \
         tells nothing apart",
        registry.names().len()
    );
}
