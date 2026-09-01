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
            let here = std::env::current_dir()
                .map_err(|error| format!("non so dove mi trovo: {error}"))?;
            init(&here)
        }
        _ => Err(format!("usage: {}", USAGE[0])),
    }
}

/// La forma di `sailor workspace`. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[&str] = &["sailor workspace init"];

/// Scrive il marcatore nella cartella data.
///
/// **`checks` NASCE VUOTO, E NON È PIGRIZIA.** Indovinare `cargo test` per un
/// progetto qualunque è la stessa presunzione del percorso assoluto che il
/// guasto 25 racconta: un comando che scrive una verifica che nessuno ha
/// chiesto la fa poi eseguire a qualcuno che crede l'abbia decisa lui.
fn init(root: &Path) -> Result<String, String> {
    let marker = root.join(MARKER);
    if marker.exists() {
        return Err(format!(
            "{} esiste già: questo comando non sovrascrive una dichiarazione, \
             perché ci si può aver scritto dentro a mano",
            marker.display()
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
    let mut text = serde_json::to_string_pretty(&declared)
        .map_err(|error| format!("non riesco a comporre la dichiarazione: {error}"))?;
    text.push('\n');
    std::fs::write(&marker, text)
        .map_err(|error| format!("non riesco a scrivere {}: {error}", marker.display()))?;

    Ok(format!(
        "scritto {}\n  nome: {name}\n  regole: {}\n  verifiche: nessuna \
         (si scrivono a mano: indovinarle sarebbe deciderle al posto tuo)",
        marker.display(),
        if rules.is_empty() {
            "nessuna trovata".to_owned()
        } else {
            rules.join(", ")
        }
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

        init(&root).expect("scrive");

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

        init(&root).expect("scrive lo stesso");

        let declared = flow::workspace::declaration_at(&root).expect("si rilegge");
        assert!(declared.rules.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    /// **NON SOVRASCRIVE.** Dentro il marcatore ci si può aver scritto a mano:
    /// riscriverlo perderebbe una dichiarazione senza chiedere.
    #[test]
    fn init_refuses_to_overwrite_a_declaration() {
        let root = scratch("gia-dichiarato");
        init(&root).expect("la prima volta");

        let refused = init(&root).expect_err("la seconda no");

        assert!(refused.contains("esiste già"), "{refused}");

        let _ = fs::remove_dir_all(&root);
    }
}
