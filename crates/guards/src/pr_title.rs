//! Il titolo di una richiesta di modifica diventa un commit: si controlla lì.
//!
//! Porta di `skills/hooks/pr-title.py`.
//!
//! PERCHÉ ESISTE, con la misura del 14/08/2026. Contati gli ultimi 200 commit
//! di ciascun repo, quelli fuori formato sono 25 nella suite (nessun gate), 30
//! nel motore (nessuno), 5 in a-client (col gate). E in **tutti e tre** il 100%
//! di quelle violazioni è entrato **dal titolo di una richiesta**, mai da un
//! messaggio scritto a mano: GitHub usa il titolo come oggetto del commit di
//! fusione, quindi il gate locale è rispettato senza eccezioni e la cronologia
//! si sporca dal gesto che quel gate non vede.
//!
//! Questo gancio guarda il gesto giusto: `gh pr create --title` e
//! `gh pr edit --title`, prima che partano.
//!
//! COSA NON FA. Non guarda il corpo, non guarda i titoli scritti sul sito, non
//! tocca le richieste di terzi. Se il titolo manca — `gh pr create` interattivo,
//! o `--fill`, che lo prende dal commit — tace: quel caso è già coperto a monte.

use crate::language::is_italian;
use crate::shell::split_words;
use hook_io::Decision;
use regex::Regex;
use std::sync::OnceLock;

const TYPES: &[&str] = &[
    "feat", "fix", "refactor", "perf", "chore", "docs", "test", "build", "ci", "style", "revert",
];
const MAX_TITLE: usize = 72;

fn conventional() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^({})(\([a-z0-9 ./-]+\))?!?: .+",
            TYPES.join("|")
        ))
        .unwrap()
    })
}

fn pr_command() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bgh\s+pr\s+(create|edit)\b").unwrap())
}

/// I titoli passati esplicitamente a `gh pr create|edit`, in ordine.
///
/// Si passa dallo splitter di shell e non da una regex sul testo grezzo perché
/// il titolo sta quasi sempre fra virgolette e contiene spazi e due punti: una
/// regex perde `--title='…'` o tronca all'oggetto.
pub fn titles(command: &str) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let split = SPLIT.get_or_init(|| Regex::new(r"&&|\|\||;|\|").unwrap());

    let mut found = Vec::new();
    for segment in split.split(command) {
        if !pr_command().is_match(segment) {
            continue;
        }
        // Un comando che non si sa spezzare non produce titoli e il gancio tace:
        // meglio muto che a caso.
        let Some(parts) = split_words(segment) else {
            continue;
        };
        for (i, p) in parts.iter().enumerate() {
            if (p == "--title" || p == "-t") && i + 1 < parts.len() {
                found.push(parts[i + 1].clone());
            } else if let Some(rest) = p.strip_prefix("--title=") {
                found.push(rest.to_string());
            }
        }
    }
    found
}

/// Cosa c'è che non va, in parole che dicono come si corregge.
pub fn flaws(title: &str) -> Vec<String> {
    let mut found = Vec::new();
    if is_italian(title) {
        found.push("è in italiano, e diventerà l'oggetto di un commit".to_string());
    }
    if !conventional().is_match(title) {
        found.push(format!(
            "non è nel formato `<tipo>(<ambito>): <oggetto>` (tipi: {})",
            TYPES.join(", ")
        ));
    }
    // Conta i **caratteri**, non i byte: un titolo con un accento occupa due
    // byte e sarebbe lungo uno in più di quanto si legge.
    let length = title.chars().count();
    if length > MAX_TITLE {
        found.push(format!(
            "è lungo {length} caratteri, il tetto è {MAX_TITLE}"
        ));
    }
    if title.ends_with('.') {
        found.push("finisce con un punto".to_string());
    }
    found
}

fn message(title: &str, found: &[String]) -> String {
    let mut lines = vec![
        format!(
            "Il titolo «{title}» diventerà l'oggetto del commit di fusione, e da lì non si corregge più."
        ),
        String::new(),
    ];
    lines.extend(found.iter().map(|f| format!("  · {f}")));
    lines.extend([
        String::new(),
        "Misurato il 14/08/2026 sui tre repo: il 100% dei commit fuori formato è entrato da un titolo di richiesta, mai da un messaggio scritto a mano.".to_string(),
        "Riscrivi il titolo in inglese e in formato Conventional, poi rilancia. Se il caso è davvero un'eccezione, metti TITOLO_RICHIESTA=off davanti al comando e dillo.".to_string(),
    ]);
    lines.join("\n")
}

pub fn judge(command: &str) -> Decision {
    for title in titles(command) {
        let found = flaws(&title);
        if !found.is_empty() {
            return Decision::Deny(message(&title, &found));
        }
    }
    Decision::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(command: &str) -> bool {
        matches!(judge(command), Decision::Deny(_))
    }

    #[test]
    fn a_well_formed_title_goes_through() {
        assert!(!refused(r#"gh pr create --title "feat(ui): add the filter chip row""#));
        assert!(!refused(r#"gh pr create --title "fix: stop losing the draft on reload""#));
        assert!(!refused(r#"gh pr create -t "chore(deps): adopt the phone package 0.2.0""#));
        assert!(!refused("gh pr create --title='refactor(query): extract the shared filter'"));
    }

    #[test]
    fn it_names_the_flaw_it_found_one_at_a_time() {
        let says = |command: &str, needle: &str| match judge(command) {
            Decision::Deny(m) => m.contains(needle),
            _ => false,
        };
        assert!(says(r#"gh pr create --title "I controlli girano sulle richieste""#, "italiano"));
        assert!(says(r#"gh pr create --title "add the filter chip row""#, "formato"));
        assert!(says(
            &format!(r#"gh pr create --title "feat(ui): {}""#, "x".repeat(80)),
            "lungo"
        ));
        assert!(says(r#"gh pr create --title "feat(ui): add the row.""#, "punto"));
        assert!(says(r#"gh pr edit 12 --title "Sistema la tabella dei residui""#, "italiano"));
    }

    #[test]
    fn it_does_not_touch_what_is_not_a_title() {
        assert!(!refused("gh pr create --fill"));
        assert!(!refused("gh pr list --limit 5"));
        assert!(!refused("gh pr view 42"));
        assert!(!refused(r#"git commit -m "feat: un oggetto in italiano""#));
    }

    #[test]
    fn the_length_boundary_is_where_the_python_put_it() {
        assert!(!flaws(&format!("feat: {}", "x".repeat(66)))
            .iter()
            .any(|f| f.contains("lungo")));
        assert!(flaws(&format!("feat: {}", "x".repeat(67)))
            .iter()
            .any(|f| f.contains("lungo")));
    }

    #[test]
    fn an_invented_type_is_not_conventional() {
        assert!(!flaws("feet(ui): add the row").is_empty());
        assert!(!flaws("feat:").is_empty());
    }

    #[test]
    fn a_title_keeps_its_colons() {
        assert_eq!(
            titles(r#"gh pr create --title "feat(ui): a: b""#),
            vec!["feat(ui): a: b"]
        );
    }

    #[test]
    fn an_unbalanced_quote_produces_no_title_at_all() {
        assert!(titles(r#"gh pr create --title "aperta"#).is_empty());
    }
}
