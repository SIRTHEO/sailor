//! Identifiers are in English, and now something measures it.
//!
//! **WHY THIS TEST EXISTS.** The rule is in `AGENTS.md`: "Identifiers in English
//! — function names, types, fields, options". A census counted **136
//! violations**, nearly all written after it. None was a slip of attention: the
//! session directive says "Italian" without saying "except identifiers", and a
//! rule no measure questions never turns red — the same lesson as the dead
//! pointer `AGENTS.md` tells about itself at lines 17-20.
//!
//! **IT IS NOT A PARSER, AND THAT IS WANTED.** It does not try to understand the
//! code: it looks for words from a hand-written list, in declaration position.
//! The price is declared — an Italian word missing from the list passes — and
//! the gain is that it has **no false positives**, so it forces nobody to argue
//! with it. Whoever meets a new one adds it below: it is one line.
//!
//! **WHAT IT DOES NOT LOOK AT.** Comments, text inside strings, and the
//! documents under `docs/`: there the language is the writer's business. And the
//! *fixture* names inside test strings — `f.name == "assente"` — stay what they
//! are: data, not identifiers.
//!
//! **AND IT DOES NOT LOOK AT THE FLOWS, BY DECISION.** The `id`s of flows and
//! steps — `sviluppa-sailor`, `verdetto` — and the names of the `.flow.json`
//! files stay in Italian: the owner's decision, written in `docs/decisions.md`. They
//! are data the **ledger keeps**: renaming a step would make already recorded
//! runs show unknown steps, and changing the name of a shipped flow would make
//! the flow a user wrote at home to replace it stop winning — in silence.
//!
//! **Whoever extends this check to the `.flow.json` files is breaking that
//! decision, not completing it.** It is no oversight.

use std::path::{Path, PathBuf};

/// The Italian words that must not appear in an identifier.
///
/// They are the ones **actually seen** in the census, plus the domain terms the
/// project keeps saying out loud (`corsa`, `passo`, `flusso`, `deposito`) and
/// which therefore end up in a name with nobody noticing.
///
/// **THE ONES MISSING ON PURPOSE.** `per`, `come`, `si`, `e` and their kin: they
/// are valid English words, or too short to be told apart from a piece of a
/// compound name — `cache_write_per_million` is English. A list holding them
/// would give errors nobody can fix, and the first person to meet one silences
/// it together with all the others.
const ITALIAN_WORDS: &[&str] = &[
    // `batteria`, `stile` and `finestra` were added by whoever met them — the
    // instruction this list gives about itself. They were the keys of the CI
    // jobs. The fourth, `prove`, **did not come in and cannot**: it is a valid
    // English word, exactly the family the comment above excludes on purpose.
    "assente",
    "atteso",
    "attesa",
    "batteria",
    "casa",
    "cassette",
    "ciclico",
    "cio",
    "conteggio",
    "coperto",
    "finestra",
    "stile",
    "corsa",
    "costata",
    "deposito",
    "elenco",
    "esempio",
    "esito",
    "fabbrica",
    "facoltativo",
    "famiglie",
    "flusso",
    "flussi",
    "guasto",
    "ignota",
    "lati",
    "letto",
    "listino",
    "lungo",
    "mai",
    "miei",
    "misurata",
    "motore",
    "nome",
    "nomi",
    "nuovo",
    "ondata",
    "parti",
    "passo",
    "piano",
    "prova",
    "ramo",
    "registro",
    "rotto",
    "sano",
    "senza",
    "smista",
    "spedito",
    "spediti",
    "spesa",
    "spia",
    "tetto",
    "tronco",
    "valido",
    "vecchio",
    "voce",
    "voci",
    "verdetto",
    "verifica",
    "quanto",
];

/// Where an identifier may be declared. The text following one of these words,
/// up to the first character that cannot be in a name.
const RUST_DECLARATIONS: &[&str] = &[
    "let ", "let mut ", "fn ", "struct ", "enum ", "mod ", "const ", "static ", "type ", "trait ",
];

const WEB_DECLARATIONS: &[&str] = &[
    "let ",
    "const ",
    "var ",
    "function ",
    "interface ",
    "class ",
    "type ",
];

/// The root of the repo, from which this test runs.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta due livelli sotto la radice")
        .to_path_buf()
}

/// Every source under `dir`, skipping what is not ours.
fn sources_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // `target` and `node_modules` are not our code; nor is `.git`.
            if matches!(name.as_str(), "target" | "node_modules" | ".git" | "dist") {
                continue;
            }
            sources_under(&path, found);
            continue;
        }
        // **THIS CHECK DOES NOT LOOK AT ITSELF.** It holds on purpose the names
        // it refuses — the word list, and the examples it feeds to its own
        // functions — and without this line it would accuse itself, forever, of
        // being badly written.
        if name == "identifiers_are_in_english.rs" {
            continue;
        }
        // **THE `.html` FILES TOO, AND NOT FOR COMPLETENESS.**
        // `crates/ui/assets/index.html` carries the dashboard page's JavaScript
        // inside it: two Italian `const`s lived there, invisible to this check's
        // first pass because the file did not end in `.ts`. No type-checker looks
        // at that code, so it is the one place where a wrong rename raises no
        // error — the place where the check is needed most.
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("ts") | Some("tsx") | Some("html")
        ) {
            found.push(path);
        }
    }
}

/// The line without the comment that closes it, if it has one.
///
/// A declared approximation: a line with `//` inside a string is cut too early.
/// The result is that **less** code is looked at, never more — this test can let
/// something through, it cannot accuse wrongly.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// The pieces of an identifier: `spesa_totale` and `spesaTotale` both give
/// `["spesa", "totale"]`.
/// **THE HUMP IS CUT ONLY AFTER A LOWERCASE LETTER.** The first version cut at
/// every capital, and `CASSETTE_KINDS` became thirteen single letters: no letter
/// is on the list, so every shouted constant would have passed clean forever.
/// The test that measures this test caught it, not an eye.
fn parts_of(identifier: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_was_lower = false;
    for character in identifier.chars() {
        if character == '_' {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            previous_was_lower = false;
            continue;
        }
        if character.is_uppercase() && previous_was_lower {
            parts.push(std::mem::take(&mut current));
        }
        previous_was_lower = character.is_lowercase();
        current.push(character.to_ascii_lowercase());
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// The name declared after `keyword`, if that line declares one.
fn declared_after<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
    let at = code.find(keyword)?;
    // It must be a whole word: `applet ` holds no declaration.
    let before = code[..at].chars().next_back();
    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let rest = &code[at + keyword.len()..];
    let end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// The Italian words in a name, if there are any.
fn italian_in(name: &str) -> Vec<String> {
    parts_of(name)
        .into_iter()
        .filter(|part| ITALIAN_WORDS.contains(&part.as_str()))
        .collect()
}

/// **EVERY DECLARED IDENTIFIER IS IN ENGLISH.**
///
/// The message lists everything it found, with path and line: a test that says
/// "there is one" forces the search to be started over by hand.
#[test]
fn every_declared_identifier_is_in_english() {
    let root = repository_root();
    let mut sources = Vec::new();
    for place in ["crates", "desktop/src", "desktop/src-tauri/src"] {
        sources_under(&root.join(place), &mut sources);
    }
    assert!(
        sources.len() > 20,
        "cercato in {} sorgenti: troppo pochi, la scansione non sta guardando dove crede",
        sources.len()
    );
    workspace::measured_against(
        sources.len(),
        "sources read for declarations",
        ITALIAN_WORDS.len(),
        "words that must not name anything",
    );

    let mut found: Vec<String> = Vec::new();
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let web = path.extension().is_some_and(|e| e != "rs");
        let keywords = if web {
            WEB_DECLARATIONS
        } else {
            RUST_DECLARATIONS
        };
        for (number, line) in text.lines().enumerate() {
            let code = code_part(line);
            for keyword in keywords {
                let Some(name) = declared_after(code, keyword) else {
                    continue;
                };
                let italian = italian_in(name);
                if italian.is_empty() {
                    continue;
                }
                found.push(format!(
                    "{}:{}  {name}  (in italiano: {})",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    number + 1,
                    italian.join(", ")
                ));
            }
        }
    }

    assert!(
        found.is_empty(),
        "{} identificatori in italiano, e la regola sta in AGENTS.md dal 28/08/2026:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// **THE FILE NAMES TOO.**
///
/// In Rust a file *is* a module, so its name is an identifier — but the rule in
/// `AGENTS.md` lists "functions, types, fields, options" and files are absent
/// from it. That is one of the reasons an Italian-named source file could be
/// born with nothing protesting.
#[test]
fn every_source_file_is_named_in_english() {
    let root = repository_root();
    let mut sources = Vec::new();
    for place in ["crates", "desktop/src", "desktop/src-tauri/src"] {
        sources_under(&root.join(place), &mut sources);
    }

    let mut found: Vec<String> = Vec::new();
    for path in &sources {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().replace('-', "_"))
            .unwrap_or_default();
        let italian = italian_in(&stem);
        if italian.is_empty() {
            continue;
        }
        found.push(format!(
            "{}  (in italiano: {})",
            path.strip_prefix(&root).unwrap_or(path).display(),
            italian.join(", ")
        ));
    }

    assert!(
        found.is_empty(),
        "{} file con nome italiano:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// **THE CI JOB KEYS TOO.**
///
/// The two tests above look at `.rs`, `.ts`, `.tsx`, `.html`. A `.yml` file is
/// code for neither of them, so the CI file — today `the-battery.yml` — was born
/// with three jobs called `prove`, `stile` and `finestra` and **nobody
/// protested**. The rule was there; nothing questioned it on that kind of file,
/// and a rule nothing questions there never turns red. It is the same shape as
/// the test on file names, one kind of file further along.
///
/// **THE DECLARED LIMIT: `prove` CANNOT BE CAUGHT.** It is a valid English word,
/// the family that list excludes on purpose because it would give accusations
/// nobody can fix. So `stile`, `finestra` and `batteria` are caught here, and
/// not everything: a check that declares where it stops is worth more than one
/// that lets you believe it reaches everywhere.
///
/// **WHY THE KEYS AND NOT THE `name:` FIELDS.** The border is the one in
/// `AGENTS.md`: what a machine reads is in English, what a person reads is not.
/// The keys are read by `needs:`, by `jobs.<id>` in the APIs and by `gh run`
/// filters — they are identifiers as much as a `struct` field. The `name:`
/// fields are the sentence shown to whoever watches a run: they are messages,
/// and messages stay in Italian as long as that line of `AGENTS.md` says so.
#[test]
fn every_workflow_job_key_is_in_english() {
    let root = repository_root();
    let workflows = root.join(".github/workflows");
    let Ok(entries) = std::fs::read_dir(&workflows) else {
        panic!("nessun workflow da controllare in {}", workflows.display());
    };

    let mut found: Vec<String> = Vec::new();
    let mut looked_at = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        ) {
            continue;
        }
        looked_at += 1;
        let text = std::fs::read_to_string(&path).expect("leggere il workflow");
        for key in job_keys_of(&text) {
            let italian = italian_in(&key.replace('-', "_"));
            if italian.is_empty() {
                continue;
            }
            found.push(format!(
                "{}  lavoro «{key}» (in italiano: {})",
                path.strip_prefix(&root).unwrap_or(&path).display(),
                italian.join(", ")
            ));
        }
    }

    // Without this line the test would stay green the day the directory is
    // renamed: it would look at zero files and tell nobody.
    assert!(
        looked_at > 0,
        "nessun file .yml letto: la prova non sta guardando niente"
    );
    assert!(
        found.is_empty(),
        "{} lavori con la chiave in italiano:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The top-level keys under `jobs:`, that is the lines indented by two spaces
/// ending in `:` before another section starts at column zero. It is not a YAML
/// reader and does not want to be: it reads the shape our workflows have, and
/// the day that shape is no longer enough the job count drops to zero — the
/// right direction to be wrong in, because the `looked_at > 0` line above
/// notices that edge case.
fn job_keys_of(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("jobs:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        // A line at column zero that is not blank closes the section.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('#') {
            continue;
        }
        if let Some(name) = rest.strip_suffix(':') {
            keys.push(name.trim().to_owned());
        }
    }
    keys
}

/// The test that measures this test.
///
/// **WHOEVER MEASURES MUST BE MEASURED.** A check that looks for words in a list
/// can be broken in a way nobody sees: if `parts_of` stopped splitting compound
/// names, or `declared_after` found nothing any more, the two tests above would
/// stay **green forever** and nobody would know they had stopped looking. Here
/// they are fed a known case and required to see it.
#[test]
fn the_check_can_still_see_a_name_it_should_reject() {
    assert_eq!(
        parts_of("write_listino"),
        vec!["write", "listino"],
        "i nomi composti si spezzano, o l'elenco non trova mai niente"
    );
    assert_eq!(
        parts_of("CASSETTE_KINDS"),
        vec!["cassette", "kinds"],
        "anche quelli in maiuscolo"
    );
    assert_eq!(
        parts_of("spesaTotale"),
        vec!["spesa", "totale"],
        "e quelli scritti a gobbe, che è come li scrive la finestra"
    );
    assert_eq!(
        declared_after("    let listino = read();", "let "),
        Some("listino")
    );
    assert_eq!(
        declared_after("    applet_size = 3;", "let "),
        None,
        "«applet» non dichiara niente: la parola dev'essere intera"
    );
    assert_eq!(
        code_part("    let x = 1; // qui il listino resta italiano"),
        "    let x = 1; ",
        "il commento si taglia via, o ogni riga di prosa diventerebbe un'accusa"
    );
    // And the key reader: it must take the jobs and **not** the lines inside a
    // job, or `name`, `steps` and `run` would become jobs.
    assert_eq!(
        job_keys_of("name: x\n\njobs:\n  # un commento\n  stile:\n    name: il debito\n    steps:\n      - run: echo\n  desktop:\n    name: la finestra\n"),
        vec!["stile", "desktop"],
        "le chiavi dei lavori, e solo quelle"
    );
    assert!(
        !italian_in("stile").is_empty(),
        "«stile» è nell'elenco: se non lo fosse, il lavoro chiamato così passerebbe"
    );
    assert!(
        italian_in("prove").is_empty(),
        "**limite dichiarato, non difetto**: «prove» è una parola inglese valida \
         e non può stare nell'elenco. Se un giorno ci finisse, questa riga \
         diventerebbe rossa e chi la legge saprebbe di aver appena reso \
         impossibile chiamare qualcosa `prove` in inglese"
    );
    assert!(!italian_in("write_listino").is_empty());
    assert!(
        italian_in("write_price_list").is_empty(),
        "e un nome inglese passa, o non si potrebbe mai riparare niente"
    );
}
