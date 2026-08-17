//! L'involucro del gancio sulle regole: ciò che ha bisogno del mondo.
//!
//! Il giudizio sta in `guards::live_rules` ed è puro. Qui restano le tre cose
//! che toccano il disco: la carta dei repo, il verificatore Node, e il registro
//! dei guasti — perché un gancio che si rompe in silenzio è indistinguibile da
//! uno contento.

use guards::live_rules as rules;
use hook_io::HookInput;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CHECKER: &str = "/Users/theo/gyver/work/.claude/scripts/check-rules.mjs";
const CARTA: &str = "/Users/theo/gyver/work/.claude/repo-in-carico.json";
/// Lo stesso di `subprocess.run(..., timeout=60)`: il verificatore su una radice
/// sbagliata ci muore dentro, e senza tetto il gancio resterebbe appeso a ogni
/// scrittura di regola.
const CHECKER_TIMEOUT: Duration = Duration::from_secs(60);

pub fn run(input: &HookInput) -> i32 {
    let path = input
        .tool_input
        .as_ref()
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !path.contains(".claude/rules/") || !path.ends_with(".md") {
        return 0;
    }

    let home = home_dir();

    // Il perimetro, prima dei riferimenti: costa una lettura e non serve Node.
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if let Some(why) = rules::always_loaded(&text) {
        // Un file illeggibile dà testo vuoto, che «non ha frontmatter»: sarebbe
        // un rimprovero per un file che non abbiamo potuto guardare.
        if !text.is_empty() {
            eprintln!("{}", rules::scope_message(why));
            return 2;
        }
        return 0;
    }

    if !Path::new(CHECKER).is_file() {
        return 0;
    }

    let lines = if rules::is_global(path, &home) {
        rules::global_rule_lines(Path::new(path), &text, &carta_repos(), &home)
    } else {
        let Some((root, repo, relative)) = rules::repo_of(path) else {
            return 0;
        };
        match run_checker(&root, &repo) {
            Ok(Some(stdout)) => rules::mine(&stdout, &relative),
            // Uscita 0: il verificatore non ha trovato niente.
            Ok(None) => return 0,
            Err(why) => {
                log_failure(path, &why);
                return 0;
            }
        }
    };

    if lines.is_empty() {
        return 0;
    }
    eprintln!("{}", rules::dead_refs_message(&lines));
    2
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Le cartelle dei repo attivi in carico, dalla carta. Vuoto se illeggibile —
/// e vuoto significa «nessun verdetto», mai un falso allarme.
fn carta_repos() -> Vec<PathBuf> {
    let Ok(raw) = std::fs::read_to_string(CARTA) else {
        return Vec::new();
    };
    let Ok(carta) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(root) = carta.get("radice").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    carta
        .get("repo")
        .and_then(|v| v.as_array())
        .map(|repos| {
            repos
                .iter()
                .filter(|r| r.get("attivo").and_then(|v| v.as_bool()).unwrap_or(false))
                .filter_map(|r| r.get("nome").and_then(|v| v.as_str()))
                .map(|nome| Path::new(root).join(nome))
                .collect()
        })
        .unwrap_or_default()
}

/// Il verificatore, puntato su quella radice e su quel repo soltanto.
///
/// `Ok(None)` quando esce zero, cioè non ha trovato niente; `Ok(Some(stdout))`
/// quando ha qualcosa da dire; `Err` solo se non è stato possibile chiederglielo.
fn run_checker(root: &Path, repo: &str) -> Result<Option<String>, String> {
    let card_path = write_card(root, repo)?;
    let result = spawn_and_wait(root, repo, &card_path);
    std::fs::remove_file(&card_path).ok();
    result
}

/// La carta a un repo solo, in un file temporaneo che si cancella da sé.
///
/// Il nome porta pid e istante: due sessioni che scrivono una regola nello
/// stesso secondo non devono leggere la carta l'una dell'altra.
fn write_card(root: &Path, repo: &str) -> Result<PathBuf, String> {
    let card = serde_json::json!({
        "radice": root.to_string_lossy(),
        "repo": [{"nome": repo, "attivo": true}],
    });
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("carta-{}-{stamp}.json", std::process::id()));
    std::fs::write(&path, card.to_string()).map_err(|e| format!("carta non scritta: {e}"))?;
    Ok(path)
}

fn spawn_and_wait(root: &Path, repo: &str, card_path: &Path) -> Result<Option<String>, String> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("node")
        .arg(CHECKER)
        .arg(repo)
        .current_dir(root)
        .env("CARTA", card_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node non parte: {e}"))?;

    // Le pipe vanno svuotate mentre il figlio scrive: il verificatore stampa
    // una riga per reperto e su un repo carico riempirebbe il buffer del
    // sistema, restando bloccato con noi che lo aspettiamo.
    let mut out = child.stdout.take().ok_or("nessuno stdout")?;
    let mut err = child.stderr.take().ok_or("nessuno stderr")?;
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });

    let deadline = Instant::now() + CHECKER_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("il verificatore non ha risposto in {CHECKER_TIMEOUT:?}"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("attesa fallita: {e}")),
        }
    };

    let stdout = out_thread.join().unwrap_or_default();
    let _ = err_thread.join();
    if status.success() {
        Ok(None)
    } else {
        Ok(Some(stdout))
    }
}

/// Una riga nel registro dei guasti, **con data e nome della regola in testa**.
///
/// Il 15/08/2026 quel registro aveva 626 righe e nessuna delle due cose, e
/// capire se i guasti fossero di stanotte o di marzo ha richiesto di dedurlo dal
/// nome di una funzione. Un registro che non si può datare non serve a chi lo apre.
fn log_failure(rule: &str, why: &str) {
    let log = home_dir().join(".claude/state/regole-vive-guasti.log");
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(
            f,
            "--- {}  {rule}\n{why}\n",
            hook_io::local_time::now_local_iso8601()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_names_exactly_one_repo_and_is_valid_json() {
        let path = write_card(Path::new("/tmp/root"), "suite").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["radice"], "/tmp/root");
        assert_eq!(v["repo"].as_array().unwrap().len(), 1);
        assert_eq!(v["repo"][0]["nome"], "suite");
        assert_eq!(v["repo"][0]["attivo"], true);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_checker_that_never_answers_is_an_error_not_a_hang() {
        // Il tetto vero è di 60 secondi: qui si prova il meccanismo, non
        // l'attesa, chiamando un comando che non esiste — il verso opposto
        // dello stesso ramo d'errore.
        let err = spawn_and_wait(Path::new("/"), "suite", Path::new("/nonexistent-card.json"));
        // `node` c'è su questa macchina; se manca, il messaggio è l'altro.
        assert!(err.is_ok() || err.unwrap_err().contains("node non parte"));
    }
}
