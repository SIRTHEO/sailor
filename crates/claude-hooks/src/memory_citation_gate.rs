//! Nega a una memoria nuova la citazione di un file che non esiste.
//!
//! Pagare la bonifica del corpus e' costato una giornata: 177 citazioni morte
//! ridotte a 45, e le residue sono per costruzione non bonificabili (script di
//! scratchpad, esempi didattici, chiamate scambiate per percorsi). Questo
//! gancio serve a non doverlo rifare. Riusa `citations()` e, dal braccio di
//! `memory_anchors`, `resolve_all()` con le sue categorie fuori conteggio: un
//! secondo giudizio scritto a modo suo divergerebbe dal primo il giorno dopo.
//!
//! COSTA NIENTE QUANDO NON DEVE PARLARE: 0,03 s su una scrittura che non porta
//! citazioni nuove, perche' non tocca ne' l'indice ne' git. Quando invece deve
//! decidere su un nome mai visto arriva a tre secondi, e li' l'attesa e' il
//! prezzo di un diniego, non di ogni salvataggio.
//!
//! Nega solo cio' che e' NUOVO: una citazione gia' presente sul file prima
//! della scrittura passa anche se non si risolve — bonificarla e' compito di
//! `memory-anchors --dead`, non di questo gancio.

use crate::memory_anchors::{branch_holding, cites_a_build_artifact, resolve_all};
use guards::memory_anchor::citations;
use hook_io::{Decision, Mode};
use std::collections::HashSet;
use std::path::Path;

/// Vero se il percorso e' una memoria sotto `~/.claude/projects/*/memory/`.
///
/// Lo stesso albero che `memory_dirs()` percorre nel braccio di
/// `memory_anchors`: qui si controlla un percorso solo, non tutta la cartella.
fn is_memory_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        return false;
    }
    let is_named = |p: Option<&Path>, name: &str| {
        p.and_then(Path::file_name).and_then(|n| n.to_str()) == Some(name)
    };
    let memory_dir = path.parent();
    let slug_dir = memory_dir.and_then(Path::parent);
    let projects_dir = slug_dir.and_then(Path::parent);
    let claude_dir = projects_dir.and_then(Path::parent);
    is_named(memory_dir, "memory")
        && is_named(projects_dir, "projects")
        && is_named(claude_dir, ".claude")
}

/// Il contenuto che risulterebbe se questa scrittura andasse a segno.
///
/// `None` quando l'ingresso non porta nessuna delle tre forme note (`Write`,
/// `Edit`, `MultiEdit`): meglio lasciar passare che giudicare alla cieca.
fn prospective_text(current: &str, tool_input: &serde_json::Value) -> Option<String> {
    if let Some(content) = tool_input.get("content").and_then(|v| v.as_str()) {
        return Some(content.to_string());
    }
    if let Some(edits) = tool_input.get("edits").and_then(|v| v.as_array()) {
        let mut text = current.to_string();
        for edit in edits {
            text = apply_one_edit(&text, edit);
        }
        return Some(text);
    }
    if tool_input.get("new_string").is_some() {
        return Some(apply_one_edit(current, tool_input));
    }
    None
}

/// Un `old_string` → `new_string`, applicato come fa lo strumento vero: la
/// prima occorrenza, o tutte con `replace_all`.
fn apply_one_edit(text: &str, edit: &serde_json::Value) -> String {
    let old = edit
        .get("old_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new = edit
        .get("new_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if old.is_empty() {
        return text.to_string();
    }
    let replace_all = edit
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if replace_all {
        text.replace(old, new)
    } else {
        text.replacen(old, new, 1)
    }
}

/// Le citazioni che compaiono nel testo nuovo e non in quello di prima, e che
/// non si risolvono su nessuna radice — al netto delle tre categorie fuori
/// conteggio di `memory_anchors` (artefatto di build, ramo diverso, repo
/// sparito e' gestito a monte da chi chiama, qui non serve).
fn new_dead_citations(before: &str, after: &str) -> Vec<String> {
    let already: HashSet<String> = citations(before).into_iter().map(|c| c.path).collect();
    let mut dead = Vec::new();
    for cit in citations(after) {
        if already.contains(&cit.path) {
            continue;
        }
        if !resolve_all(&cit.path).is_empty() {
            continue;
        }
        if cites_a_build_artifact(&cit.path) {
            continue;
        }
        if branch_holding(&cit.path).is_some() {
            continue;
        }
        if !dead.contains(&cit.path) {
            dead.push(cit.path);
        }
    }
    dead
}

fn report(dead: &[String]) -> String {
    let mut s = String::from(
        "Questa scrittura introduce citazioni a file che non si trovano su nessuna \
         radice, in nessun ramo:",
    );
    for p in dead.iter().take(6) {
        s.push_str(&format!("\n  `{p}`"));
    }
    if dead.len() > 6 {
        s.push_str(&format!("\n  … e altre {}", dead.len() - 6));
    }
    s.push_str(
        "\nSe il file non esiste piu', correggi il percorso, o annota dove vive oggi con \
         una nota di mappatura — la freccia subito dopo l'apice la copre: \
         `nome-vecchio.rs` → `dove/vive/oggi.rs`. Se il file esiste ma altrove, correggi \
         la cartella. Valvola: MEMORY_CITATION_GATE=off.",
    );
    s
}

/// Legge l'ingresso del gancio, decide, e nega la scrittura se introduce una
/// citazione morta.
pub fn run() -> i32 {
    if Mode::from_env("MEMORY_CITATION_GATE") == Mode::Off {
        return 0;
    }
    let Some(input) = hook_io::read_input() else {
        return 0;
    };
    if !matches!(
        input.tool_name.as_deref(),
        Some("Write") | Some("Edit") | Some("MultiEdit")
    ) {
        return 0;
    }
    let empty = serde_json::json!({});
    let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
    let path = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if path.is_empty() || !is_memory_file(Path::new(path)) {
        return 0;
    }
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let Some(after) = prospective_text(&current, tool_input) else {
        return 0;
    };
    let dead = new_dead_citations(&current, &after);
    if dead.is_empty() {
        return 0;
    }
    hook_io::emit("memory-citation-gate", &Decision::Deny(report(&dead)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_anchors::set_test_roots;
    use std::fs;
    use std::path::PathBuf;

    /// Una cartella usa-e-getta, isolata dal disco vero.
    ///
    /// Sotto `hook_io::testing::test_dir`, che porta il pid del processo nella
    /// radice: un nome fisso sotto `/tmp` collideva fra due `cargo test`
    /// lanciati insieme sullo stesso repo (stesso precedente di `memory_anchors`).
    fn scratch(name: &str) -> PathBuf {
        hook_io::testing::test_dir(&format!("memory-citation-gate-{name}"))
    }

    #[test]
    fn recognises_a_memory_file_and_rejects_everything_else() {
        assert!(is_memory_file(Path::new(
            "/home/someone/.claude/projects/-Users-theo-orca-general/memory/x.md"
        )));
        assert!(!is_memory_file(Path::new(
            "/home/someone/orca/general/src/a.rs"
        )));
        assert!(!is_memory_file(Path::new(
            "/home/someone/.claude/projects/-x/memory/x.txt"
        )));
        // Una cartella "memory" fuori da "projects" non e' quella che cerchiamo.
        assert!(!is_memory_file(Path::new(
            "/home/someone/.claude/skills/x/memory/x.md"
        )));
    }

    #[test]
    fn a_write_replaces_the_whole_file() {
        let input = serde_json::json!({"content": "nuovo corpo"});
        assert_eq!(
            prospective_text("vecchio", &input).as_deref(),
            Some("nuovo corpo")
        );
    }

    #[test]
    fn an_edit_replaces_only_its_own_target() {
        let one = serde_json::json!({"old_string": "a", "new_string": "b"});
        assert_eq!(prospective_text("a a", &one).as_deref(), Some("b a"));
        let all = serde_json::json!({"old_string": "a", "new_string": "b", "replace_all": true});
        assert_eq!(prospective_text("a a", &all).as_deref(), Some("b b"));
    }

    #[test]
    fn a_multi_edit_applies_every_step_in_order() {
        let input = serde_json::json!({
            "edits": [
                {"old_string": "uno", "new_string": "1"},
                {"old_string": "due", "new_string": "2"}
            ]
        });
        assert_eq!(
            prospective_text("uno e due", &input).as_deref(),
            Some("1 e 2")
        );
    }

    #[test]
    fn a_new_citation_that_does_not_resolve_is_dead() {
        let dir = scratch("dead");
        let _roots = set_test_roots(vec![dir]);
        let dead = new_dead_citations("", "cita `mai-esistito.rs` qui.");
        assert_eq!(dead, vec!["mai-esistito.rs".to_string()]);
    }

    #[test]
    fn an_old_citation_survives_an_unrelated_edit() {
        // Era gia' morta prima della scrittura: non e' compito di questo
        // gancio bonificarla, solo di non aggiungerne altre.
        let dir = scratch("old-survives");
        let _roots = set_test_roots(vec![dir]);
        let before = "cita `mai-esistito.rs` in fondo.";
        let after = "cita `mai-esistito.rs` in fondo, e aggiunge una frase.";
        assert!(new_dead_citations(before, after).is_empty());
    }

    #[test]
    fn a_citation_that_resolves_is_not_dead() {
        let dir = scratch("resolves");
        fs::write(dir.join("codice.rs"), "fn a() {}\n").unwrap();
        let _roots = set_test_roots(vec![dir]);
        let dead = new_dead_citations("", "cita `codice.rs` qui.");
        assert!(dead.is_empty(), "{dead:?}");
    }

    #[test]
    fn a_mapping_note_covers_the_old_name() {
        // `nuovo.rs` e' il file di oggi e deve esistere davvero: la nota
        // mappa solo il nome vecchio, non esime anche quello nuovo.
        let dir = scratch("mapped");
        fs::write(dir.join("nuovo.rs"), "fn a() {}\n").unwrap();
        let _roots = set_test_roots(vec![dir]);
        let text = "Dove vive oggi: `vecchio.py` → `nuovo.rs`, e si cita ancora `vecchio.py`.";
        let dead = new_dead_citations("", text);
        assert!(dead.is_empty(), "{dead:?}");
    }

    #[test]
    fn a_build_artifact_citation_is_exempt() {
        let dir = scratch("artifact");
        let _roots = set_test_roots(vec![dir]);
        let dead = new_dead_citations("", "genera `dist/main.js` a ogni build.");
        assert!(dead.is_empty(), "{dead:?}");
    }

    #[test]
    fn a_citation_only_on_another_branch_is_exempt() {
        // Un albero di lavoro sta quasi sempre su un ramo suo: chi cita un
        // file appena fuso non ha scritto niente di falso.
        let dir = scratch("branch");
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", &repo.to_string_lossy()])
                .args(args)
                .output()
                .unwrap();
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "prova@example.com"]);
        git(&["config", "user.name", "prova"]);
        fs::write(repo.join("merged.ts"), "export const a = 1;\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "seed"]);
        git(&["update-ref", "refs/remotes/origin/develop", "HEAD"]);
        fs::remove_file(repo.join("merged.ts")).unwrap();
        let _roots = set_test_roots(vec![repo]);
        let dead = new_dead_citations("", "cita `merged.ts` qui.");
        assert!(dead.is_empty(), "{dead:?}");
    }
}
