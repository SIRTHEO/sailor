//! Il porto Rust del gancio `PreCompact`, oggi `~/.claude/scripts/handoff-precompact.sh`.
//!
//! PERCHÉ. Lo script scrive la consegna in `${TMPDIR:-/tmp}`, che macOS svuota
//! a ogni riavvio — misurato il 25/08/2026: dopo il riavvio di quel giorno,
//! zero consegne sopravvissute. La consegna serve proprio DOPO un guasto, ed è
//! il guasto a cancellarla. Qui la fonte è `~/.claude/state/consegne-precompact/`,
//! che un riavvio non tocca; la copia in `$TMPDIR` resta come ripiego per chi
//! la cerca dove stava prima, ma non è più la fonte.
//!
//! COSA RESTA. Il nome porta l'ora e non solo la sessione — col nome fisso una
//! sessione compattata sedici volte lasciava una sola fotografia, misurato il
//! 12/08/2026 — e ogni raccolta fallita lascia la propria sezione vuota invece
//! di far fallire il gancio: un `PreCompact` che nega la compattazione è peggio
//! del problema che risolve.
//!
//! COSA CAMBIA IN PIÙ. La cartella durevole cresce a ogni compattazione di ogni
//! sessione: si tiene un tetto, le ultime [`KEEP_LATEST`] per data di modifica,
//! cancellando solo dentro quella cartella.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Quante consegne restano nella cartella durevole dopo ogni scrittura.
///
/// È un registro che si scrive a ogni compattazione di ogni sessione, quindi
/// cresce senza un limite naturale — lo stesso difetto ricorrente delle
/// automazioni di casa. Duecento file da poche righe l'uno coprono settimane di
/// lavoro vero e pesano pochi MB.
const KEEP_LATEST: usize = 200;

/// I fatti di un giro, già raccolti: nessuna funzione di questo blocco tocca
/// disco o `git`, così [`render`] si prova senza costruire una scena vera.
#[derive(Debug, Default, Clone)]
pub struct Facts {
    pub session_id: Option<String>,
    pub cwd: String,
    pub trigger: Option<String>,
    pub transcript_path: Option<String>,
    pub git: Option<GitFacts>,
}

#[derive(Debug, Clone)]
pub struct GitFacts {
    pub branch: Option<String>,
    pub uncommitted: Vec<String>,
    pub unsent: UnsentCommits,
    pub other_worktrees: Vec<String>,
}

/// Il confronto va fatto contro un riferimento remoto che esiste davvero:
/// elencare i commit contro un ramo che non è su `origin` direbbe "non
/// inviati" di commit che invece ci sono.
#[derive(Debug, Clone)]
pub enum UnsentCommits {
    /// Il ramo base trovato su `origin`, e le righe di `git log --oneline base..HEAD`.
    Against(String, Vec<String>),
    /// Nessuno dei candidati esiste su `origin`.
    NoBase,
}

/// Il primo pezzo di un id di sessione, o `ignota` se manca — stessa regola
/// dello script (`${SESSIONE:0:8}` su una stringa più corta ne restituisce
/// meno, non la riempie).
fn session_prefix(session_id: Option<&str>) -> String {
    match session_id.filter(|s| !s.trim().is_empty()) {
        Some(s) => s.chars().take(8).collect(),
        None => "ignota".to_string(),
    }
}

/// «2026-08-17T00:03:00» → «20260817-000300»: la forma senza separatori che
/// serve al nome del file.
fn compact_timestamp(iso_seconds: &str) -> String {
    let date: String = iso_seconds.chars().take(10).filter(|c| *c != '-').collect();
    let time: String = iso_seconds
        .chars()
        .skip(11)
        .take(8)
        .filter(|c| *c != ':')
        .collect();
    format!("{date}-{time}")
}

/// Il nome del file: sessione **e** ora, non la sola sessione — vedi la
/// premessa del modulo.
fn file_name(prefix: &str, compact_ts: &str) -> String {
    format!("consegna-{prefix}-{compact_ts}.md")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn tmp_dir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// La cartella durevole: sopravvive a un riavvio, a differenza di `$TMPDIR`.
pub fn durable_dir() -> PathBuf {
    home_dir()
        .join(".claude")
        .join("state")
        .join("consegne-precompact")
}

fn git_output(dir: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_ok(dir: &str, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Il primo ramo remoto candidato che esiste per davvero su `origin`, provato
/// nello stesso ordine dello script: il ramo corrente, poi `develop`, poi `main`.
fn unsent_commits(cwd: &str, branch: Option<&str>) -> UnsentCommits {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(b) = branch.filter(|b| !b.is_empty() && *b != "?") {
        candidates.push(format!("origin/{b}"));
    }
    candidates.push("origin/develop".to_string());
    candidates.push("origin/main".to_string());
    for base in candidates {
        if git_ok(cwd, &["rev-parse", "--verify", &base]) {
            let lines = git_output(cwd, &["log", "--oneline", &format!("{base}..HEAD")])
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect();
            return UnsentCommits::Against(base, lines);
        }
    }
    UnsentCommits::NoBase
}

/// I fatti di git, o `None` se `cwd` non è un repo — la stessa porta
/// dell'originale (`git rev-parse --git-dir`). Best-effort: un comando che
/// fallisce lascia la propria lista vuota, non fa saltare la raccolta.
fn collect_git(cwd: &str) -> Option<GitFacts> {
    if cwd.is_empty() || !git_ok(cwd, &["rev-parse", "--git-dir"]) {
        return None;
    }
    let branch = git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let uncommitted = git_output(cwd, &["status", "--short"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let unsent = unsent_commits(cwd, branch.as_deref());
    // La prima riga di `worktree list` è questa stessa copia: si salta, come
    // fa `tail -n +2` nello script.
    let other_worktrees = git_output(cwd, &["worktree", "list"])
        .unwrap_or_default()
        .lines()
        .skip(1)
        .map(str::to_string)
        .collect();
    Some(GitFacts {
        branch,
        uncommitted,
        unsent,
        other_worktrees,
    })
}

/// I fatti dal JSON grezzo del gancio, con lo stesso ripiego dello script per
/// la cartella (quella del processo, se il campo manca).
fn facts_from_payload(payload: &Value) -> Facts {
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let cwd = str_field("cwd").unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let git = collect_git(&cwd);
    Facts {
        session_id: str_field("session_id"),
        cwd,
        trigger: str_field("trigger"),
        transcript_path: str_field("transcript_path"),
        git,
    }
}

/// Il testo della consegna: fatti misurati, non un riassunto. `written_at` è
/// già formattato (`%Y-%m-%d %H:%M:%S`), passato da fuori perché questa
/// funzione resti pura — nessun orologio da fermare per provarla.
pub fn render(facts: &Facts, written_at: &str) -> String {
    let mut out = String::new();
    let prefix = session_prefix(facts.session_id.as_deref());
    let trigger = facts.trigger.as_deref().unwrap_or("ignoto");

    out.push_str(&format!("# Consegna automatica — {prefix}\n"));
    out.push('\n');
    out.push_str(&format!(
        "Scritta dal gancio PreCompact il {written_at} (motivo: {trigger}).\n"
    ));
    out.push_str("Sono **fatti misurati**, non un riassunto: la parte ragionata la scrive\n");
    out.push_str("`handoff`, che va invocata prima della compattazione.\n");
    out.push('\n');
    out.push_str(&format!("- cartella di lavoro: `{}`\n", facts.cwd));
    if let Some(t) = facts.transcript_path.as_deref() {
        out.push_str(&format!("- trascrizione intera: `{t}`\n"));
    }
    out.push('\n');

    if let Some(git) = &facts.git {
        out.push_str("## Git\n");
        out.push('\n');
        out.push_str(&format!(
            "- ramo: `{}`\n",
            git.branch.as_deref().unwrap_or("?")
        ));
        if git.uncommitted.is_empty() {
            out.push_str("- nessuna modifica non committata\n");
        } else {
            out.push_str(&format!(
                "- **modifiche non committate** ({} file):\n",
                git.uncommitted.len()
            ));
            out.push('\n');
            out.push_str("```\n");
            out.push_str(&git.uncommitted.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
            out.push_str("\n```\n");
        }
        out.push('\n');

        match &git.unsent {
            UnsentCommits::Against(base, lines) if !lines.is_empty() => {
                out.push_str(&format!(
                    "- **commit presenti qui e non in `{base}`** ({}):\n",
                    lines.len()
                ));
                out.push('\n');
                out.push_str("```\n");
                out.push_str(&lines.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
                out.push_str("\n```\n");
            }
            UnsentCommits::Against(base, _) => {
                out.push_str(&format!("- nessun commit oltre `{base}`\n"));
            }
            UnsentCommits::NoBase => {
                out.push_str(
                    "- nessun riferimento remoto confrontabile: lo stato dei commit va verificato a mano\n",
                );
            }
        }
        out.push('\n');

        if !git.other_worktrees.is_empty() {
            out.push_str(
                "- altre copie di lavoro dello stesso repo (possono trattenere lavoro non inviato):\n",
            );
            out.push('\n');
            out.push_str("```\n");
            out.push_str(&git.other_worktrees.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
            out.push_str("\n```\n");
        }
        out.push('\n');
    }

    out.push_str("## Da dove si riprende\n");
    out.push('\n');
    out.push_str("1. Leggi lo stato reale con `git status` e `git log`: quanto sopra è la\n");
    out.push_str("   fotografia del momento della compattazione, non di adesso.\n");
    out.push_str("2. I documenti di lavoro vivono nel repo (`docs/plans/`, `docs/specs/`,\n");
    out.push_str("   `Plans.md`): leggi quelli invece di fidarti del riassunto.\n");
    out.push_str("3. Se il lavoro passa a un'altra sessione, invoca `handoff`\n");
    out.push_str("   adesso che il contesto è di nuovo ampio.\n");
    out
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Tiene le `keep` consegne più recenti per data di modifica dentro `dir`,
/// cancellando **solo lì dentro**. Un file che non si legge o non si cancella
/// si lascia stare: la rotazione è best-effort quanto il resto del gancio.
fn rotate(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, SystemTime)> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .filter(|e| e.file_name().to_string_lossy().starts_with("consegna-"))
        .filter_map(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (e.path(), t))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(_, t)| *t);
    let excess = files.len() - keep;
    for (path, _) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

/// Scrive la consegna nel posto durevole e, come ripiego, anche in `$TMPDIR`.
/// Ritorna il percorso durevole solo se la scrittura lì è riuscita: è quello il
/// posto che rende vero il nome del gancio, la copia in `$TMPDIR` non basta a
/// dire "scritta".
pub fn write_report(facts: &Facts) -> Option<PathBuf> {
    let secs = now_epoch();
    let offset = hook_io::local_time::local_offset(secs);
    let iso = hook_io::local_time::iso_seconds(secs, offset);
    let written_at = iso.replacen('T', " ", 1);
    let prefix = session_prefix(facts.session_id.as_deref());
    let name = file_name(&prefix, &compact_timestamp(&iso));
    let text = render(facts, &written_at);

    let dir = durable_dir();
    let durable_path = if fs::create_dir_all(&dir).is_ok() && fs::write(dir.join(&name), &text).is_ok() {
        rotate(&dir, KEEP_LATEST);
        Some(dir.join(&name))
    } else {
        None
    };

    // Il ripiego per chi la cerca dove stava prima: se questa scrittura fallisce
    // non è un guasto del gancio, il durevole sopra è già la fonte.
    let _ = fs::write(tmp_dir().join(&name), &text);

    durable_path
}

/// Il gancio vero: legge il JSON di `PreCompact` da stdin e non fallisce mai —
/// un `PreCompact` che nega la compattazione è peggio del problema che
/// risolve. Uno stdin illeggibile o non-JSON produce fatti vuoti, esattamente
/// come faceva lo script con `except Exception: {}`.
pub fn run() -> i32 {
    let mut raw = String::new();
    use std::io::Read as _;
    let _ = std::io::stdin().read_to_string(&mut raw);
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    hook_io::mark_live_from_payload(&payload);

    let facts = facts_from_payload(&payload);
    let written = write_report(&facts);
    let message = match written {
        Some(path) => format!(
            "Consegna automatica salvata in {} (ramo, modifiche non committate, commit non inviati). \
             La compattazione perde i fatti verificabili: rileggi quel file prima di dichiarare lo \
             stato del lavoro. Se il lavoro passa a un'altra sessione, invoca handoff ora che il \
             contesto è ampio.",
            path.display()
        ),
        None => "Il gancio PreCompact non è riuscito a scrivere la consegna automatica nella \
                  cartella durevole."
            .to_string(),
    };
    println!("{}", crate::json_tool::message(&message));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_prefix_takes_the_first_eight_characters_or_falls_back_to_ignota() {
        assert_eq!(session_prefix(Some("d6cefb4d1234abcd")), "d6cefb4d");
        // Più corta di otto: come lo slicing di bash, resta così com'è.
        assert_eq!(session_prefix(Some("abc")), "abc");
        assert_eq!(session_prefix(None), "ignota");
        assert_eq!(session_prefix(Some("")), "ignota");
        assert_eq!(session_prefix(Some("   ")), "ignota");
    }

    #[test]
    fn compact_timestamp_strips_separators_and_keeps_the_order() {
        assert_eq!(compact_timestamp("2026-08-17T00:03:00"), "20260817-000300");
        assert_eq!(compact_timestamp("2026-01-05T23:59:01"), "20260105-235901");
    }

    #[test]
    fn the_file_name_carries_the_session_and_the_instant_not_the_session_alone() {
        // Il difetto del 12/08/2026: col nome fisso, sedici compattazioni della
        // stessa sessione lasciavano una sola fotografia. Due chiamate allo
        // stesso prefisso ma istanti diversi devono dare nomi diversi.
        let a = file_name("d6cefb4d", "20260812-100000");
        let b = file_name("d6cefb4d", "20260812-100016");
        assert_ne!(a, b);
        assert_eq!(a, "consegna-d6cefb4d-20260812-100000.md");
    }

    #[test]
    fn a_json_payload_missing_every_field_still_produces_facts() {
        // La "sessione ignota" e il resto del contratto best-effort: un JSON
        // vuoto non fa saltare la raccolta, ogni campo assente resta `None`
        // e la cartella si ripiega sul processo.
        let facts = facts_from_payload(&serde_json::json!({}));
        assert!(facts.session_id.is_none());
        assert!(facts.trigger.is_none());
        assert!(facts.transcript_path.is_none());
        assert!(!facts.cwd.is_empty(), "la cartella ripiega su quella del processo");
    }

    #[test]
    fn empty_string_fields_count_as_missing_not_as_empty_facts() {
        // Un campo presente ma vuoto è la forma che prende un JSON scritto a
        // metà: si tratta come assente, non come un fatto vero.
        let facts = facts_from_payload(&serde_json::json!({
            "session_id": "", "trigger": "", "transcript_path": "", "cwd": ""
        }));
        assert!(facts.session_id.is_none());
        assert!(facts.trigger.is_none());
        assert!(facts.transcript_path.is_none());
    }

    #[test]
    fn a_realistic_payload_is_read_field_by_field() {
        let facts = facts_from_payload(&serde_json::json!({
            "session_id": "d6cefb4d-89ab-cdef-0123-456789abcdef",
            "cwd": "/tmp/non-un-repo",
            "trigger": "auto",
            "transcript_path": "/tmp/qualcosa.jsonl",
        }));
        assert_eq!(facts.session_id.as_deref(), Some("d6cefb4d-89ab-cdef-0123-456789abcdef"));
        assert_eq!(facts.cwd, "/tmp/non-un-repo");
        assert_eq!(facts.trigger.as_deref(), Some("auto"));
        assert_eq!(facts.transcript_path.as_deref(), Some("/tmp/qualcosa.jsonl"));
        // La cartella non e' un repo: la sezione git resta assente, non un
        // panico.
        assert!(facts.git.is_none());
    }

    #[test]
    fn render_falls_back_the_same_way_the_script_did_on_missing_fields() {
        let facts = Facts {
            session_id: None,
            cwd: "/qui".to_string(),
            trigger: None,
            transcript_path: None,
            git: None,
        };
        let text = render(&facts, "2026-08-25 21:03:00");
        assert!(text.contains("# Consegna automatica — ignota"));
        assert!(text.contains("(motivo: ignoto)"));
        assert!(text.contains("- cartella di lavoro: `/qui`"));
        assert!(!text.contains("trascrizione intera"), "nessun percorso, nessuna riga");
        assert!(!text.contains("## Git"), "senza fatti di git la sezione non compare");
    }

    #[test]
    fn render_shows_uncommitted_changes_and_their_count() {
        let facts = Facts {
            session_id: Some("d6cefb4d".to_string()),
            cwd: "/repo".to_string(),
            trigger: Some("manual".to_string()),
            transcript_path: None,
            git: Some(GitFacts {
                branch: Some("suite-229".to_string()),
                uncommitted: vec![" M a.rs".to_string(), "?? b.rs".to_string()],
                unsent: UnsentCommits::NoBase,
                other_worktrees: vec![],
            }),
        };
        let text = render(&facts, "2026-08-25 21:03:00");
        assert!(text.contains("## Git"));
        assert!(text.contains("- ramo: `suite-229`"));
        assert!(text.contains("**modifiche non committate** (2 file)"));
        assert!(text.contains(" M a.rs"));
        assert!(text.contains("nessun riferimento remoto confrontabile"));
    }

    #[test]
    fn render_says_no_uncommitted_changes_when_the_list_is_empty() {
        let facts = Facts {
            session_id: Some("d6cefb4d".to_string()),
            cwd: "/repo".to_string(),
            trigger: None,
            transcript_path: None,
            git: Some(GitFacts {
                branch: Some("main".to_string()),
                uncommitted: vec![],
                unsent: UnsentCommits::Against("origin/main".to_string(), vec![]),
                other_worktrees: vec![],
            }),
        };
        let text = render(&facts, "2026-08-25 21:03:00");
        assert!(text.contains("- nessuna modifica non committata"));
        assert!(text.contains("- nessun commit oltre `origin/main`"));
    }

    #[test]
    fn render_lists_unsent_commits_and_other_worktrees_when_present() {
        let facts = Facts {
            session_id: Some("d6cefb4d".to_string()),
            cwd: "/repo".to_string(),
            trigger: None,
            transcript_path: None,
            git: Some(GitFacts {
                branch: Some("suite-229".to_string()),
                uncommitted: vec![],
                unsent: UnsentCommits::Against(
                    "origin/develop".to_string(),
                    vec!["abc1234 fix: qualcosa".to_string()],
                ),
                other_worktrees: vec!["/altra/copia  abcd123 [suite-230]".to_string()],
            }),
        };
        let text = render(&facts, "2026-08-25 21:03:00");
        assert!(text.contains("**commit presenti qui e non in `origin/develop`** (1)"));
        assert!(text.contains("abc1234 fix: qualcosa"));
        assert!(text.contains("altre copie di lavoro dello stesso repo"));
        assert!(text.contains("/altra/copia"));
    }

    /// Un file più vecchio, con un `mtime` esplicito — necessario perché due
    /// scritture nello stesso test possono cadere nello stesso secondo, e la
    /// rotazione deve comunque saper distinguere quale tenere.
    fn write_with_mtime(path: &Path, seconds_ago: u64) {
        fs::write(path, "x").unwrap();
        let when = SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        fs::File::open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn rotation_keeps_only_the_newest_and_only_inside_the_given_directory() {
        let dir = std::env::temp_dir().join(format!(
            "handoff-precompact-rotate-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Cinque consegne, dalla più vecchia alla più recente.
        for (name, age) in [
            ("consegna-aaaaaaaa-20260101-000000.md", 500u64),
            ("consegna-bbbbbbbb-20260102-000000.md", 400),
            ("consegna-cccccccc-20260103-000000.md", 300),
            ("consegna-dddddddd-20260104-000000.md", 200),
            ("consegna-eeeeeeee-20260105-000000.md", 100),
        ] {
            write_with_mtime(&dir.join(name), age);
        }
        // Un file che non è una consegna: la rotazione non lo deve toccare
        // nemmeno se è il più vecchio di tutti.
        write_with_mtime(&dir.join("altro-file.md"), 999);
        write_with_mtime(&dir.join("consegna-vecchia.txt"), 999);

        // Una cartella diversa, con la sua consegna vecchissima: la rotazione
        // di `dir` non deve cancellare fuori dai propri confini.
        let sibling_dir = std::env::temp_dir().join(format!(
            "handoff-precompact-rotate-sibling-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&sibling_dir);
        fs::create_dir_all(&sibling_dir).unwrap();
        write_with_mtime(&sibling_dir.join("consegna-ffffffff-19990101-000000.md"), 100_000);

        rotate(&dir, 3);

        let remaining: std::collections::BTreeSet<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            remaining,
            std::collections::BTreeSet::from([
                "consegna-cccccccc-20260103-000000.md".to_string(),
                "consegna-dddddddd-20260104-000000.md".to_string(),
                "consegna-eeeeeeee-20260105-000000.md".to_string(),
                "altro-file.md".to_string(),
                "consegna-vecchia.txt".to_string(),
            ]),
            "solo le tre consegne piu' recenti restano, i due file estranei non si toccano"
        );
        assert!(
            sibling_dir.join("consegna-ffffffff-19990101-000000.md").exists(),
            "la rotazione di una cartella non deve cancellare in un'altra"
        );

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&sibling_dir);
    }

    #[test]
    fn rotation_does_nothing_when_the_count_is_already_within_the_limit() {
        let dir = std::env::temp_dir().join(format!(
            "handoff-precompact-rotate-below-limit-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write_with_mtime(&dir.join("consegna-aaaaaaaa-20260101-000000.md"), 10);
        rotate(&dir, 3);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Una `HOME` e una `TMPDIR` usa-e-getta, con lo stesso lucchetto di
    /// [`crate::test_home::HomeIsolata`]: sono entrambe variabili di processo
    /// e i test girano in parallelo dentro lo stesso processo.
    struct Scene {
        _home: crate::test_home::HomeIsolata,
        previous_tmpdir: Option<String>,
        pub tmp: PathBuf,
    }

    impl Scene {
        fn new(name: &str) -> Self {
            let home = crate::test_home::HomeIsolata::nuova(name);
            let tmp = home.dir.join("tmp");
            fs::create_dir_all(&tmp).unwrap();
            let previous_tmpdir = std::env::var("TMPDIR").ok();
            std::env::set_var("TMPDIR", &tmp);
            Self {
                _home: home,
                previous_tmpdir,
                tmp,
            }
        }
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            match &self.previous_tmpdir {
                Some(v) => std::env::set_var("TMPDIR", v),
                None => std::env::remove_var("TMPDIR"),
            }
        }
    }

    #[test]
    fn write_report_lands_in_the_durable_folder_with_the_right_name() {
        let scene = Scene::new("durable-write");
        let facts = Facts {
            session_id: Some("d6cefb4d1234".to_string()),
            cwd: "/qui".to_string(),
            trigger: Some("auto".to_string()),
            transcript_path: None,
            git: None,
        };
        let path = write_report(&facts).expect("la cartella durevole e' scrivibile");
        assert_eq!(path.parent(), Some(durable_dir().as_path()));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("consegna-d6cefb4d-"), "{name}");
        assert!(name.ends_with(".md"), "{name}");
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("(motivo: auto)"));

        // La copia in $TMPDIR resta come ripiego, con lo stesso nome.
        let copy = scene.tmp.join(&name);
        assert!(copy.exists(), "manca la copia di ripiego in $TMPDIR");
    }

    #[test]
    fn collecting_git_facts_on_a_fresh_repository_stays_best_effort() {
        let scene = Scene::new("fresh-repo");
        let repo = scene.tmp.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git deve girare")
        };
        run(&["init", "--quiet", "-b", "main"]);
        run(&["config", "user.email", "prova@example.com"]);
        run(&["config", "user.name", "prova"]);
        fs::write(repo.join("a.txt"), "x").unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "primo"]);

        let repo_str = repo.to_string_lossy().into_owned();
        let git = collect_git(&repo_str).expect("e' un repo git vero");
        assert_eq!(git.branch.as_deref(), Some("main"));
        assert!(git.uncommitted.is_empty(), "appena committato: niente da salvare");
        // Nessun `origin`: nessun riferimento remoto confrontabile.
        assert!(matches!(git.unsent, UnsentCommits::NoBase));

        // Un file non tracciato: deve comparire fra le modifiche non committate.
        fs::write(repo.join("b.txt"), "y").unwrap();
        let dirty_git = collect_git(&repo_str).expect("resta un repo git vero");
        assert_eq!(dirty_git.uncommitted.len(), 1);
    }

    #[test]
    fn collecting_git_facts_outside_a_repository_gives_none() {
        let scene = Scene::new("not-a-repo");
        let dir = scene.tmp.join("non-git");
        fs::create_dir_all(&dir).unwrap();
        assert!(collect_git(&dir.to_string_lossy()).is_none());
        assert!(collect_git("").is_none());
    }
}
