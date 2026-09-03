//! `sailor workspace`: il progetto si dichiara, invece di essere indovinato.
//!
//! **PERCHÉ È UN COMANDO E NON UN FILE SCRITTO A MANO.** È il guasto 15: per
//! cambiare l'innesco di un flusso è stato usato uno script Python che
//! riscrive il JSON, perché Sailor non aveva nessun comando per operare sui
//! propri file. Uno strumento che si aggira non registra niente di ciò che gli
//! succede intorno, e nessun controllo se ne accorge. Ogni cosa che una
//! persona deve fare su un progetto Sailor è un comando di Sailor.

use flow::workspace::MARKER;
use std::path::Path;

/// Seconds since the epoch. The register keeps dates, and a clock that is not
/// named is a clock nobody can replace in a test.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// I documenti che, se ci sono, valgono la pena di essere dichiarati.
///
/// **È UN ELENCO DI CANDIDATI, NON UNA SCOPERTA.** Cercare «tutti i `.md` che
/// sembrano regole» vorrebbe dire indovinare, e ciò che questo comando scrive
/// lo legge poi qualcun altro come se fosse stato deciso. Quello che non è in
/// elenco si aggiunge a mano al file, che è il posto giusto per una decisione.
const RULE_CANDIDATES: [&str; 5] = [
    "AGENTS.md",
    "CLAUDE.md",
    "docs/decisioni.md",
    "docs/da-fare.md",
    "docs/guasti-incontrati.md",
];

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor workspace: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    match args {
        [command] if command == "init" => {
            let here = std::env::current_dir().map_err(|error| {
                catalogue::say(
                    "cli.no_telling_where_i_am",
                    &[("error", &error.to_string())],
                )
            })?;
            init(&here, ledger::sailor_home().as_deref())
        }
        [command] if command == "list" => list(),
        _ => Err(crate::forms_as_lines(USAGE)
            .iter()
            .map(|line| format!("{} {line}", catalogue::say("cli.usage_heading", &[])))
            .collect::<Vec<_>>()
            .join("\n")),
    }
}

/// La forma di `sailor workspace`. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor workspace init",
        says_key: "",
    },
    crate::Form {
        form: "sailor workspace list",
        says_key: "",
    },
];

/// The projects this machine has been opened in.
///
/// **A PROJECT THAT LOST ITS MARKER IS PRINTED, NOT DROPPED.** Whoever moved a
/// folder yesterday and cannot find it today learns nothing from a shorter
/// list; the row says `gone` and keeps the path, which is the one thing that
/// makes the situation repairable.
fn list() -> Result<String, String> {
    let home = ledger::sailor_home()
        .ok_or_else(|| catalogue::say("cli.workspace.no_house_to_read", &[]))?;
    // A tree Sailor worked in and that declares itself is a project too, or
    // the list stays empty while the projects exist.
    let seen = sessions::Sessions::default_path()
        .ok()
        .and_then(|path| sessions::Sessions::open(path).ok())
        .and_then(|store| store.trees_worked_in().ok())
        .unwrap_or_default();
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default();
    let known = flow::workspace::known_including(&home, &seen, at)?;
    if known.is_empty() {
        return Ok(catalogue::say("cli.workspace.none_known", &[]));
    }
    let width = known
        .iter()
        .map(|entry| entry.name.len())
        .max()
        .unwrap_or(0);
    Ok(known
        .iter()
        .map(|entry| {
            let standing = match flow::workspace::standing_of(entry) {
                flow::workspace::Standing::Declared => "declared",
                flow::workspace::Standing::Gone => "gone",
            };
            format!(
                "{:width$}  {standing:8}  {}",
                entry.name,
                entry.root.display(),
                width = width
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Scrive il marcatore nella cartella data.
///
/// **`checks` NASCE VUOTO, E NON È PIGRIZIA.** Indovinare `cargo test` per un
/// progetto qualunque è la stessa presunzione del percorso assoluto che il
/// guasto 25 racconta: un comando che scrive una verifica che nessuno ha
/// chiesto la fa poi eseguire a qualcuno che crede l'abbia decisa lui.
fn init(root: &Path, home: Option<&Path>) -> Result<String, String> {
    let marker = root.join(MARKER);
    if marker.exists() {
        return Err(catalogue::say(
            "cli.workspace.already_declared",
            &[("file", &marker.display().to_string())],
        ));
    }
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rules: Vec<String> = RULE_CANDIDATES
        .iter()
        .filter(|candidate| root.join(candidate).is_file())
        .map(|candidate| (*candidate).to_owned())
        .collect();

    let declared = serde_json::json!({
        "name": name,
        "rules": rules,
        "checks": serde_json::Map::new(),
    });
    let mut text = serde_json::to_string_pretty(&declared).map_err(|error| {
        catalogue::say(
            "cli.workspace.cannot_compose",
            &[("error", &error.to_string())],
        )
    })?;
    text.push('\n');
    std::fs::write(&marker, text)
        .map_err(|error| format!("cannot write {}: {error}", marker.display()))?;

    // DECLARING A PROJECT PUTS IT ON THE LIST, or the marker exists and
    // `workspace list` cannot see it. **The house is an argument**: fetched
    // here, the tests of this command wrote into the real register of whoever
    // ran `cargo test`. A house that will not take it is not a reason to fail.
    if let Some(home) = home {
        let _ = flow::workspace::remember_in(home, root, now());
    }

    let found = if rules.is_empty() {
        catalogue::say("cli.workspace.no_rules_found", &[])
    } else {
        rules.join(", ")
    };
    Ok(catalogue::say(
        "cli.workspace.written",
        &[
            ("file", &marker.display().to_string()),
            ("name", &name),
            ("rules", &found),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-workspace-cmd-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("cartella di prova");
        dir
    }

    /// Il marcatore nasce con le regole che ci sono davvero, e **senza
    /// verifiche**: quelle le scrive chi sa cosa vuol dire «provato» qui.
    #[test]
    fn init_writes_the_marker_with_the_rules_it_finds_and_no_checks() {
        let root = scratch("init");
        fs::write(root.join("AGENTS.md"), "regole").expect("un documento");
        fs::create_dir_all(root.join("docs")).expect("docs");
        fs::write(root.join("docs/decisioni.md"), "decisioni").expect("un altro");

        init(&root, None).expect("scrive");

        let declared = flow::workspace::declaration_at(&root).expect("si rilegge");
        assert_eq!(declared.rules, vec!["AGENTS.md", "docs/decisioni.md"]);
        assert!(
            declared.checks.is_empty(),
            "indovinare una verifica è deciderla al posto di chi lavora qui"
        );
        assert!(
            flow::workspace::find_root(&root).is_some(),
            "ora è una radice"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// Un documento che non c'è non finisce nell'elenco: una regola dichiarata
    /// e assente manda a leggere un indirizzo vuoto, che è il difetto che
    /// `AGENTS.md` racconta di sé.
    #[test]
    fn a_rule_that_is_not_there_is_not_declared() {
        let root = scratch("senza-regole");

        init(&root, None).expect("scrive lo stesso");

        let declared = flow::workspace::declaration_at(&root).expect("si rilegge");
        assert!(declared.rules.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    /// **NON SOVRASCRIVE.** Dentro il marcatore ci si può aver scritto a mano:
    /// riscriverlo perderebbe una dichiarazione senza chiedere.
    #[test]
    fn init_refuses_to_overwrite_a_declaration() {
        let root = scratch("gia-dichiarato");
        init(&root, None).expect("declared the first time");

        let refused = init(&root, None).expect_err("and refused the second");

        // English, because that is what the product speaks when nobody asked for
        // anything else. The Italian of the same key is checked where the two
        // catalogues are, not here.
        assert!(refused.contains("is already there"), "{refused}");

        let _ = fs::remove_dir_all(&root);
    }

    /// **DECLARING A PROJECT PUTS IT ON THE LIST**, and the list is the one in
    /// the house that was handed in. Without this the tests above, which all
    /// pass `None`, would leave the registration untested — and the version
    /// that registered nothing at all would pass them just the same.
    #[test]
    fn declaring_a_project_writes_it_into_the_house_it_was_given() {
        let root = scratch("registrato");
        let house = scratch("casa");
        init(&root, Some(&house)).expect("dichiara");

        let known = flow::workspace::known_in(&house).expect("the register reads");
        assert_eq!(known.len(), 1, "the declaration never reached the list");
        assert_eq!(known[0].root, root);
    }

    /// **AND WITH NO HOUSE IT WRITES NOWHERE.** This is the other half, and it
    /// is the one that comes from a real fault: the tests of this command used
    /// to reach for the real home, and six scratch projects ended up in the
    /// register of whoever ran `cargo test`.
    #[test]
    fn with_no_house_the_marker_is_written_and_nothing_else_is() {
        let root = scratch("senza-casa");
        let elsewhere = scratch("casa-che-resta-vuota");
        init(&root, None).expect("dichiara lo stesso");

        assert!(root.join(MARKER).is_file(), "the marker was not written");
        assert!(
            flow::workspace::known_in(&elsewhere)
                .expect("si legge")
                .is_empty(),
            "a house nobody named was written into"
        );
    }
}
