//! Il catalogo degli eventi, e la domanda che il censimento non fa.
//!
//! `scripts/hook-census.py` risponde già a due domande, e bene: ogni gancio
//! registrato punta a un file che esiste, e ogni script sotto `~/.claude` è
//! chiamato da qualcuno — launchd, `.zshrc` e i `CLAUDE.md` compresi, dopo che
//! il 14/08/2026 aveva dichiarato orfani quattro strumenti vivi.
//!
//! LA DOMANDA CHE MANCA è quella che il 16/08/2026 è costata la macchina:
//! **il gancio parte?** Quel giorno quattro script erano stati rinominati e
//! `settings.json` era rimasto ai vecchi percorsi — un caso che il censimento
//! prende, perché il file non c'era più. Ma il modo più comune in cui un gancio
//! si rompe è un altro: parte e risponde male. Sintassi rotta da una modifica a
//! metà, file troncato da un disco pieno, binario compilato per l'architettura
//! sbagliata. Lì `is_file()` dice sì e la macchina si ferma lo stesso.
//!
//! COME LO CHIEDE, senza eseguire niente. Ogni linguaggio ha il suo modo di
//! dire «questo file è compilabile» a costo quasi zero e **senza effetti
//! collaterali**:
//!
//! ```text
//! .py            python3 -m py_compile
//! .mjs .js       node --check
//! .sh .bash      bash -n
//! il binario     claude-hooks --check, che decide casi noti
//! ```
//!
//! PERCHÉ NON SI ESEGUONO DAVVERO. Fra i 45 ganci registrati ce ne sono su
//! `SessionEnd` che chiudono la registrazione di una sessione e su `Stop` che
//! armano una consegna: eseguirli per provarli produrrebbe gli effetti che
//! descrivono. Un controllo di salute che sporca lo stato che sorveglia è
//! peggio del guasto che cerca. Il limite è dichiarato: un import mancante non
//! si vede dalla compilazione, e questo controllo non lo trova.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Cosa si è potuto dire di un file citato da un gancio.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Health {
    Fine,
    Missing,
    Broken,
    /// Nessuno strumento noto sa dire se questo file è sano.
    Unknown,
}

struct Entry {
    event: String,
    matcher: String,
    file: PathBuf,
    health: Health,
    detail: String,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// I percorsi di file citati in un comando registrato.
///
/// Si prendono solo i percorsi assoluti: un comando può nominare molte cose, e
/// ciò che va verificato è il file che eseguirà davvero.
fn cited_files(command: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for raw in command.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ';') {
        let piece = raw.trim_matches(|c| c == '"' || c == '\'');
        // Le righe adottate scrivono `B=/percorso/claude-hooks; if [ -x "$B" ]`:
        // il percorso è il valore di un'assegnazione, non un token a sé, e
        // cercando solo ciò che comincia con `/` il binario non compariva mai.
        // Cioè il controllo saltava esattamente il file da cui oggi dipendono
        // dieci ganci. Trovato guardando l'elenco, non l'uscita.
        let candidate = match piece.split_once('=') {
            Some((_, value)) if value.starts_with('/') => value,
            _ => piece,
        };
        if !candidate.starts_with('/') {
            continue;
        }
        let ok = [".py", ".sh", ".bash", ".mjs", ".js"]
            .iter()
            .any(|e| candidate.ends_with(e))
            || candidate.ends_with("claude-hooks");
        if ok {
            let path = PathBuf::from(candidate);
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// La salute di un file, chiesta allo strumento del suo linguaggio.
fn health_of(path: &Path) -> (Health, String) {
    if !path.is_file() {
        return (Health::Missing, "il file non esiste".to_string());
    }
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (program, args): (&str, Vec<&str>) = match extension.as_str() {
        "py" => ("python3", vec!["-m", "py_compile"]),
        "mjs" | "js" => ("node", vec!["--check"]),
        "sh" | "bash" => ("bash", vec!["-n"]),
        _ => {
            // Il binario dei ganci si prova col suo stesso `--check`, che gli fa
            // decidere casi noti: è più di una compilazione, ed è il motivo per
            // cui esiste.
            if path.file_name().map(|n| n == "claude-hooks").unwrap_or(false) {
                let output = Command::new(path).arg("--check").output();
                return match output {
                    Ok(o) if o.status.success() => (Health::Fine, "--check verde".to_string()),
                    Ok(o) => (
                        Health::Broken,
                        String::from_utf8_lossy(&o.stderr).trim().to_string(),
                    ),
                    Err(e) => (Health::Broken, format!("non parte: {e}")),
                };
            }
            return (Health::Unknown, "nessuno strumento sa provarlo".to_string());
        }
    };

    let output = Command::new(program).args(&args).arg(path).output();
    match output {
        Ok(o) if o.status.success() => (Health::Fine, String::new()),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr);
            let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
            (Health::Broken, last.trim().to_string())
        }
        // Manca l'interprete: non è il gancio a essere rotto, ed è importante
        // non dirlo — un falso allarme su un controllo di salute lo fa spegnere.
        Err(e) => (Health::Unknown, format!("{program} non disponibile: {e}")),
    }
}

/// Verifica i file su più thread, uno per volta ciascuno.
///
/// Niente canali né dipendenze: ogni thread prende una fetta contigua e
/// restituisce i suoi risultati alla fine. Con una trentina di file e otto
/// nuclei il lavoro finisce in un ottavo del tempo, che è ciò che permette di
/// mettere questo controllo all'apertura invece che alla chiusura.
fn check_in_parallel(files: &[PathBuf]) -> BTreeMap<PathBuf, (Health, String)> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(files.len().max(1));
    let size = files.len().div_ceil(workers.max(1));

    let mut out = BTreeMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(size.max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|f| (f.clone(), health_of(f)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(part) = handle.join() {
                out.extend(part);
            }
        }
    });
    out
}

/// Legge `settings.json` e restituisce (evento, matcher, comando).
fn registered() -> Result<Vec<(String, String, String)>, String> {
    let path = home().join(".claude").join("settings.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("settings.json illeggibile: {e}"))?;
    let mut out = Vec::new();
    let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else {
        return Err("settings.json non ha una sezione hooks".to_string());
    };
    for (event, groups) in hooks {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group
                .get("matcher")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            for hook in group
                .get("hooks")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(command) = hook.get("command").and_then(|v| v.as_str()) {
                    out.push((event.clone(), matcher.clone(), command.to_string()));
                }
            }
        }
    }
    Ok(out)
}

/// Dove va il messaggio, e con che uscita.
///
/// All'apertura di una sessione le due cose cambiano: il testo deve finire nel
/// contesto del modello — cioè su **stdout** — e l'uscita deve restare 0,
/// perché un gancio di `SessionStart` che fallisce è un guasto in più, non un
/// avviso. Da riga di comando vale l'opposto: stderr e uscita 1, così si può
/// mettere in una catena di controlli.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Command,
    SessionStart,
}

/// Il catalogo, con la salute di ogni file citato.
///
/// `verbose` stampa anche l'elenco per evento; senza, parla solo dei guasti —
/// un rapporto che dice le stesse quaranta righe a ogni apertura si smette di
/// leggere in due giorni, ed è così che è morto il censimento precedente.
pub fn run_with(verbose: bool, voice: Voice) -> i32 {
    let entries = match registered() {
        Ok(e) => e,
        Err(why) => {
            eprintln!("preflight: {why}");
            return 1;
        }
    };

    // Un file citato da dieci ganci si prova una volta sola: la compilazione
    // costa, e con 45 voci su 31 file distinti sarebbe un terzo di lavoro
    // sprecato.
    let mut distinct: Vec<PathBuf> = Vec::new();
    for (_, _, command) in &entries {
        for file in cited_files(command) {
            if !distinct.contains(&file) {
                distinct.push(file);
            }
        }
    }

    // In parallelo, perché il posto giusto per questo controllo è l'apertura
    // della sessione e non la sua chiusura: il censimento precedente girava su
    // `SessionEnd` e parlava a danno già subito. In fila costava 910 ms, che a
    // ogni apertura è un prezzo che si finisce per togliere.
    let checked = check_in_parallel(&distinct);

    let mut rows: Vec<Entry> = Vec::new();
    for (event, matcher, command) in &entries {
        for file in cited_files(command) {
            let (health, detail) = checked
                .get(&file)
                .cloned()
                .unwrap_or((Health::Unknown, String::new()));
            rows.push(Entry {
                event: event.clone(),
                matcher: matcher.clone(),
                file,
                health,
                detail,
            });
        }
    }

    let broken: Vec<&Entry> = rows
        .iter()
        .filter(|r| r.health == Health::Missing || r.health == Health::Broken)
        .collect();

    if verbose {
        let mut by_event: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
        for row in &rows {
            by_event.entry(&row.event).or_default().push(row);
        }
        println!(
            "{} ganci registrati su {} eventi, {} file distinti\n",
            entries.len(),
            by_event.len(),
            checked.len()
        );
        for (event, group) in &by_event {
            println!("{event}");
            for row in group {
                let mark = match row.health {
                    Health::Fine => "  ",
                    Health::Unknown => "? ",
                    _ => "! ",
                };
                let name = row
                    .file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                println!("  {mark}{:<26} {}", row.matcher, name);
            }
        }
        println!();
    }

    if broken.is_empty() {
        if verbose {
            println!("ogni gancio registrato esiste e compila.");
        }
        return 0;
    }

    let mut message = format!(
        "{} ganci registrati non partirebbero. Un PreToolUse in errore rifiuta \
         OGNI strumento, Read compreso: se qui sotto c'è un gancio su Bash, la \
         macchina si ferma alla prossima sessione.",
        broken.len()
    );
    for row in &broken {
        message.push_str(&format!(
            "\n  {}/{}: {} — {}",
            row.event,
            row.matcher,
            row.file.display(),
            row.detail
        ));
    }
    if voice == Voice::SessionStart {
        // `settings.json` è protetto: la riga la corregge Theo, e dirlo qui
        // evita che la sessione perda un turno a scoprirlo.
        println!(
            "{message}\n  La riga di settings.json la corregge Theo: il file è \
             protetto dal gancio che vieta le scritture su Linear."
        );
        return 0;
    }
    eprintln!("{message}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_picks_the_absolute_paths_out_of_a_registered_command() {
        let command = "B=/Users/x/claude-hooks; if [ -x \"$B\" ]; then \"$B\" cd-guard; \
                       else python3 /Users/x/.claude/skills/hooks/cd-guard.py; fi";
        let files = cited_files(command);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("claude-hooks")));
        assert!(files.iter().any(|f| f.ends_with("cd-guard.py")));
    }

    /// Il binario sta a destra di un'assegnazione, e per un giro il controllo
    /// non l'ha visto: verificava tutti i ganci tranne quello da cui dipendono.
    #[test]
    fn the_binary_hides_behind_an_assignment() {
        let files = cited_files("B=/Users/x/rust/target/release/claude-hooks; \"$B\" cd-guard");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("claude-hooks"), "{files:?}");
    }

    #[test]
    fn a_relative_word_is_not_a_file() {
        assert!(cited_files("echo ciao.py").is_empty());
        assert!(cited_files("nohup bash -c 'true'").is_empty());
    }

    #[test]
    fn a_file_that_is_not_there_is_missing_not_broken() {
        let (health, _) = health_of(Path::new("/tmp/non-esiste-davvero-12345.py"));
        assert_eq!(health, Health::Missing);
    }

    /// Il caso che conta: un file che esiste e **non** compila. È quello che
    /// `is_file()` non vede, ed è il motivo per cui questo controllo esiste.
    #[test]
    fn a_file_that_exists_but_does_not_compile_is_broken() {
        let path = std::env::temp_dir().join(format!("preflight-{}.py", std::process::id()));
        std::fs::write(&path, "def rotto(:\n").unwrap();
        let (health, detail) = health_of(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(health, Health::Broken, "{detail}");
        assert!(!detail.is_empty(), "il guasto va nominato, non solo contato");
    }

    #[test]
    fn a_healthy_script_is_fine() {
        let path = std::env::temp_dir().join(format!("preflight-ok-{}.sh", std::process::id()));
        std::fs::write(&path, "#!/bin/bash\necho fine\n").unwrap();
        let (health, _) = health_of(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(health, Health::Fine);
    }
}
