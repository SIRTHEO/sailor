//! `SessionStart`: la ronda delle novità, gradino 9 della scala
//! (`docs/plans/2026-08-23-la-scala-sailor-e-la-squadra.md`), decisa da Theo
//! il 23/08/2026 (libro di bordo, voce «La configurazione si mantiene da
//! sola»). Due soli inneschi, giudicati in `guards::ronda_trigger`: **A** —
//! una versione nuova di Claude Code; **B** — la configurazione è cambiata
//! senza che nessuna ronda l'abbia guardata (impronta di `settings.json`, o
//! il binario dei ganci non allineato a `HEAD`). Chi scatta scrive UN
//! mandato in coda — mai due lo stesso giorno per lo stesso innesco, mai un
//! secondo se ce n'è già uno aperto — e la sessione generale lo raccoglie:
//! qui non si decide niente, si delega a chi legge la coda.
//!
//! LE RADICI DI STATO SEGUONO `HOME`, come ogni altro gancio di questa cassa
//! (`register_session::state_dir`): un `HOME` isolato nei test sposta anche
//! `~/.claude/cache/changelog.md`, `~/.claude/state/*` e il repo `git` su cui
//! si legge `HEAD`, senza bisogno di una variabile dedicata.
//!
//! FAIL-OPEN OVUNQUE: nel dubbio, silenzio. Un gancio di `SessionStart` che
//! rompe l'avvio costa più del guasto che segnala.
//!
//! Valvola: `RONDA_TRIGGER=off`.

use guards::ronda_trigger as judge;
use hook_io::Mode;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// La riga che Theo dovrà aggiungere a `settings.json` — mai scritta lì da
/// questo gancio, solo citata nel mandato che finisce in coda.
const SETTINGS_LINE: &str = r#"{"type": "command", "command": "/home/someone/.claude/rust/target/release/claude-hooks ronda-trigger", "timeout": 10}"#;

const VERSION_SEEN: &str = "ronda-versione-vista";
const FINGERPRINT_SEEN: &str = "ronda-fingerprint-vista";
const COOLDOWN_A: &str = "ronda-ultima-a";
const COOLDOWN_B: &str = "ronda-ultima-b";
const MANDATE_A: &str = "AUTO-ronda-innesco-a-versione-claude-code.md";
const MANDATE_B: &str = "AUTO-ronda-innesco-b-configurazione-senza-prova.md";

fn state_dir() -> PathBuf {
    crate::register_session::state_dir()
}

fn claude_dir() -> PathBuf {
    // `state_dir()` è già `$HOME/.claude/state`: risalire di un livello evita
    // di rileggere `HOME` una seconda volta con una propria copia.
    state_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn queue_dir() -> PathBuf {
    state_dir().join("plancia").join("segnalazioni")
}

fn read_trim(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// `YYYY-MM-DD`, per il cooldown giornaliero. UTC: non dipende dal fuso del
/// sistema, e non deve combaciare col formato di nessun altro registro — è
/// una chiave interna a questo solo gancio.
fn today() -> String {
    hook_io::journal::now_iso8601_seconds()[..10].to_string()
}

/// Per il frontmatter del mandato, nella stessa forma leggibile delle voci
/// già in coda (`quando: 2026-08-23 22:37`).
fn now_stamp() -> String {
    let full = hook_io::journal::now_iso8601_seconds(); // "2026-08-23T21:04:17Z"
    format!("{} {}", &full[..10], &full[11..16])
}

fn changelog_text() -> Option<String> {
    fs::read_to_string(claude_dir().join("cache").join("changelog.md")).ok()
}

/// L'ultima riga non vuota di `settings-fingerprint-changes.jsonl`, o `None`
/// se il file non c'è o non si legge — «non lo so», mai «non è mai cambiata».
fn last_fingerprint_line() -> Option<String> {
    let text = fs::read_to_string(state_dir().join("settings-fingerprint-changes.jsonl")).ok()?;
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
}

fn binary_commit() -> String {
    read_trim(&state_dir().join("hooks-binary-commit")).unwrap_or_default()
}

fn head_commit() -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(claude_dir())
        .args(["rev-parse", "HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn mandate_is_open(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|t| judge::already_open(&t))
        .unwrap_or(false)
}

/// Innesco A: versione nuova di Claude Code. `None` se non c'è niente da
/// dire, `Some(riga)` quando ha appena scritto un mandato.
fn handle_version_trigger() -> Option<String> {
    let changelog = changelog_text()?;
    let current = judge::latest_version(&changelog)?;
    let seen_path = state_dir().join(VERSION_SEEN);
    let seen = read_trim(&seen_path);
    match judge::check_version(&current, seen.as_deref()) {
        judge::VersionCheck::FirstRun => {
            let _ = fs::write(&seen_path, &current);
            None
        }
        judge::VersionCheck::Unchanged => None,
        judge::VersionCheck::Changed { previous, current } => {
            let mandate_path = queue_dir().join(MANDATE_A);
            if mandate_is_open(&mandate_path) {
                return None; // già in coda: non se ne apre un secondo
            }
            let cooldown_path = state_dir().join(COOLDOWN_A);
            let today = today();
            if judge::in_cooldown(&today, read_trim(&cooldown_path).as_deref()) {
                return None;
            }
            let body = judge::mandate_body_a(&now_stamp(), &previous, &current, SETTINGS_LINE);
            if fs::create_dir_all(queue_dir()).is_err() {
                return None;
            }
            if fs::write(&mandate_path, body).is_err() {
                return None;
            }
            // Aggiornati solo a mandato scritto davvero: un tentativo che il
            // cooldown o la voce già aperta hanno fermato non deve far
            // sparire la versione dietro `ronda-versione-vista`, o la
            // prossima sessione smetterebbe di vederla come «cambiata».
            let _ = fs::write(&seen_path, &current);
            let _ = fs::write(&cooldown_path, &today);
            Some(judge::additional_context(
                "A",
                &mandate_path.display().to_string(),
            ))
        }
    }
}

/// Innesco B: configurazione cambiata senza prova.
fn handle_drift_trigger() -> Option<String> {
    let fp_seen_path = state_dir().join(FINGERPRINT_SEEN);
    let last_line = last_fingerprint_line();
    let fp_check =
        judge::check_fingerprint(last_line.as_deref(), read_trim(&fp_seen_path).as_deref());

    // Prima esecuzione del gancio: si registra la riga di adesso come punto
    // di partenza senza innescare, altrimenti la cronologia accumulata prima
    // che questo gancio esistesse scatterebbe a vuoto alla prima sessione
    // dopo il rilascio — lo stesso motivo di `VERSION_SEEN` qui sopra.
    if fp_check == judge::FingerprintCheck::FirstRun {
        if let Some(line) = &last_line {
            let _ = fs::write(&fp_seen_path, line);
        }
    }

    let binary = binary_commit();
    let head = head_commit();
    let verdict = judge::check_drift(&fp_check, &binary, &head);
    if !verdict.fires {
        return None;
    }
    let mandate_path = queue_dir().join(MANDATE_B);
    if mandate_is_open(&mandate_path) {
        return None;
    }
    let cooldown_path = state_dir().join(COOLDOWN_B);
    let today = today();
    if judge::in_cooldown(&today, read_trim(&cooldown_path).as_deref()) {
        return None;
    }
    let body = judge::mandate_body_b(&now_stamp(), &verdict, &binary, &head, SETTINGS_LINE);
    if fs::create_dir_all(queue_dir()).is_err() {
        return None;
    }
    if fs::write(&mandate_path, body).is_err() {
        return None;
    }
    // Stesso motivo del ramo A: il watermark della riga di deriva avanza solo
    // quando il mandato è stato scritto davvero.
    if let judge::FingerprintCheck::Changed(line) = &fp_check {
        let _ = fs::write(&fp_seen_path, line);
    }
    let _ = fs::write(&cooldown_path, &today);
    Some(judge::additional_context(
        "B",
        &mandate_path.display().to_string(),
    ))
}

pub fn run() -> i32 {
    if Mode::from_env("RONDA_TRIGGER") == Mode::Off {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    let source = payload.get("source").and_then(|v| v.as_str()).unwrap_or("");
    if !judge::wants_run(source) {
        return 0;
    }
    let mut said: Vec<String> = Vec::new();
    if let Some(line) = handle_version_trigger() {
        said.push(line);
    }
    if let Some(line) = handle_drift_trigger() {
        said.push(line);
    }
    if !said.is_empty() {
        println!("{}", said.join("\n"));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    fn write_changelog(home: &HomeIsolata, body: &str) {
        let dir = home.dir.join(".claude/cache");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("changelog.md"), body).unwrap();
    }

    /// Un repo `.claude` vero e minimo, per poter chiedere `git rev-parse
    /// HEAD` come fa il codice di produzione — senza, il ramo binario non si
    /// può provare dal vivo.
    fn init_claude_repo(home: &HomeIsolata) -> String {
        let dir = home.dir.join(".claude");
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap()
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "prova@esempio.test"]);
        run(&["config", "user.name", "prova"]);
        run(&["commit", "--allow-empty", "-q", "-m", "iniziale"]);
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string()
    }

    fn queue_file(home: &HomeIsolata, name: &str) -> PathBuf {
        home.stato().join("plancia").join("segnalazioni").join(name)
    }

    // ─── Innesco A ────────────────────────────────────────────────────────

    #[test]
    fn a_first_run_primes_the_seen_version_without_firing() {
        let home = HomeIsolata::nuova("ronda-a-primo-giro");
        write_changelog(&home, "# Changelog\n\n## 2.1.241\n\n- note\n");
        let out = handle_version_trigger();
        assert_eq!(out, None, "il primo giro non deve parlare");
        assert_eq!(
            fs::read_to_string(home.stato().join(VERSION_SEEN)).unwrap(),
            "2.1.241"
        );
        assert!(!queue_file(&home, MANDATE_A).exists());
    }

    #[test]
    fn a_new_version_writes_one_mandate_and_speaks_once() {
        let home = HomeIsolata::nuova("ronda-a-versione-nuova");
        write_changelog(&home, "# Changelog\n\n## 2.1.240\n\n- vecchia\n");
        assert_eq!(handle_version_trigger(), None); // prima esecuzione: prime
        write_changelog(&home, "# Changelog\n\n## 2.1.241\n\n- nuova\n");
        let out = handle_version_trigger().expect("deve parlare: la versione e' cambiata");
        assert!(out.contains("innesco A"), "{out}");
        let mandate = queue_file(&home, MANDATE_A);
        let body = fs::read_to_string(&mandate).unwrap();
        assert!(body.contains("**Prima**: `2.1.240`"), "{body}");
        assert!(body.contains("**Dopo**: `2.1.241`"), "{body}");
        assert!(body.contains(SETTINGS_LINE), "{body}");
        assert_eq!(
            fs::read_to_string(home.stato().join(VERSION_SEEN)).unwrap(),
            "2.1.241"
        );
    }

    #[test]
    fn an_unchanged_version_stays_silent() {
        let home = HomeIsolata::nuova("ronda-a-versione-uguale");
        write_changelog(&home, "# Changelog\n\n## 2.1.241\n\n- x\n");
        assert_eq!(handle_version_trigger(), None); // prime
        assert_eq!(
            handle_version_trigger(),
            None,
            "stessa versione: nessuna riga"
        );
        assert!(!queue_file(&home, MANDATE_A).exists());
    }

    #[test]
    fn version_trigger_respects_the_daily_cooldown() {
        let home = HomeIsolata::nuova("ronda-a-cooldown");
        write_changelog(&home, "# Changelog\n\n## 1.0.0\n");
        assert_eq!(handle_version_trigger(), None); // prime
        write_changelog(&home, "# Changelog\n\n## 2.0.0\n");
        assert!(
            handle_version_trigger().is_some(),
            "prima volta oggi: deve scattare"
        );
        // Si chiude la voce e si offre una terza versione, sempre oggi.
        let mandate = queue_file(&home, MANDATE_A);
        fs::write(
            &mandate,
            fs::read_to_string(&mandate)
                .unwrap()
                .replace("stato: aperta", "stato: chiusa"),
        )
        .unwrap();
        write_changelog(&home, "# Changelog\n\n## 3.0.0\n");
        assert_eq!(
            handle_version_trigger(),
            None,
            "il cooldown vince anche a voce chiusa, stesso giorno"
        );
    }

    #[test]
    fn version_trigger_never_opens_a_second_mandate_while_one_is_open() {
        let home = HomeIsolata::nuova("ronda-a-gia-aperta");
        write_changelog(&home, "# Changelog\n\n## 1.0.0\n");
        assert_eq!(handle_version_trigger(), None); // prime
        write_changelog(&home, "# Changelog\n\n## 2.0.0\n");
        assert!(handle_version_trigger().is_some());
        // Il cooldown da solo basterebbe a fermarla: lo si aggira spostando
        // la data indietro, cosi' la prova isola davvero la voce gia' aperta.
        fs::write(home.stato().join(COOLDOWN_A), "2000-01-01").unwrap();
        write_changelog(&home, "# Changelog\n\n## 3.0.0\n");
        assert_eq!(
            handle_version_trigger(),
            None,
            "la voce e' ancora aperta: non se ne scrive una seconda"
        );
        let body = fs::read_to_string(queue_file(&home, MANDATE_A)).unwrap();
        assert!(
            body.contains("2.0.0"),
            "il file aperto non va sovrascritto: {body}"
        );
    }

    // ─── Innesco B ────────────────────────────────────────────────────────

    #[test]
    fn drift_trigger_fires_on_a_misaligned_binary() {
        let home = HomeIsolata::nuova("ronda-b-binario");
        let head = init_claude_repo(&home);
        fs::write(home.stato().join("hooks-binary-commit"), "commit-vecchio").unwrap();
        let out = handle_drift_trigger().expect("il binario e' disallineato: deve scattare");
        assert!(out.contains("innesco B"), "{out}");
        let body = fs::read_to_string(queue_file(&home, MANDATE_B)).unwrap();
        assert!(body.contains("commit-vecchio"), "{body}");
        assert!(body.contains(&head), "{body}");
    }

    #[test]
    fn drift_trigger_is_silent_when_the_binary_matches_head() {
        let home = HomeIsolata::nuova("ronda-b-allineato");
        let head = init_claude_repo(&home);
        fs::write(home.stato().join("hooks-binary-commit"), &head).unwrap();
        assert_eq!(handle_drift_trigger(), None);
        assert!(!queue_file(&home, MANDATE_B).exists());
    }

    #[test]
    fn drift_trigger_primes_the_fingerprint_watermark_on_first_run() {
        let home = HomeIsolata::nuova("ronda-b-primo-giro-impronta");
        let head = init_claude_repo(&home);
        fs::write(home.stato().join("hooks-binary-commit"), &head).unwrap(); // allineato
        fs::write(
            home.stato().join("settings-fingerprint-changes.jsonl"),
            "{\"when\":\"2026-08-22T01:00:00+0200\",\"before\":\"a\",\"after\":\"b\"}\n",
        )
        .unwrap();
        assert_eq!(
            handle_drift_trigger(),
            None,
            "prima esecuzione: si registra il watermark, non si innesca"
        );
        assert!(fs::read_to_string(home.stato().join(FINGERPRINT_SEEN))
            .unwrap()
            .contains("\"after\":\"b\""));
    }

    #[test]
    fn drift_trigger_fires_when_the_fingerprint_log_grows_a_new_line() {
        let home = HomeIsolata::nuova("ronda-b-impronta-nuova");
        let head = init_claude_repo(&home);
        fs::write(home.stato().join("hooks-binary-commit"), &head).unwrap();
        let log = home.stato().join("settings-fingerprint-changes.jsonl");
        fs::write(&log, "{\"before\":\"a\",\"after\":\"b\"}\n").unwrap();
        assert_eq!(handle_drift_trigger(), None); // prime
        fs::write(
            &log,
            "{\"before\":\"a\",\"after\":\"b\"}\n{\"before\":\"b\",\"after\":\"c\"}\n",
        )
        .unwrap();
        let out = handle_drift_trigger().expect("nuova riga di deriva: deve scattare");
        assert!(out.contains("innesco B"), "{out}");
    }

    #[test]
    fn drift_trigger_respects_the_daily_cooldown_and_the_open_entry() {
        let home = HomeIsolata::nuova("ronda-b-cooldown-e-gia-aperta");
        let head = init_claude_repo(&home);
        fs::write(home.stato().join("hooks-binary-commit"), "vecchio-1").unwrap();
        assert!(
            handle_drift_trigger().is_some(),
            "prima volta oggi: deve scattare"
        );
        // Stesso giorno, binario ancora diverso: il cooldown vince.
        fs::write(home.stato().join("hooks-binary-commit"), "vecchio-2").unwrap();
        assert_eq!(handle_drift_trigger(), None, "cooldown dello stesso giorno");
        // Si sposta il cooldown fuori dal giorno per isolare la voce gia' aperta.
        fs::write(home.stato().join(COOLDOWN_B), "2000-01-01").unwrap();
        assert_eq!(
            handle_drift_trigger(),
            None,
            "la voce e' ancora aperta: non se ne scrive una seconda"
        );
        let body = fs::read_to_string(queue_file(&home, MANDATE_B)).unwrap();
        assert!(
            body.contains("vecchio-1"),
            "il file aperto non va sovrascritto: {body}"
        );
        let _ = head; // usato solo per costruire il repo
    }

    // ─── L'evento e il ramo attorno ──────────────────────────────────────

    #[test]
    fn the_valve_silences_everything_before_stdin_is_even_read() {
        let home = HomeIsolata::nuova("ronda-valvola");
        write_changelog(&home, "# Changelog\n\n## 9.9.9\n");
        std::env::set_var("RONDA_TRIGGER", "off");
        // `run()` controlla la valvola come prima riga, prima di leggere
        // stdin: con la valvola spesa esce 0 anche nel processo della
        // batteria, dove lo stdin vero non è il payload di un gancio.
        let code = run();
        std::env::remove_var("RONDA_TRIGGER");
        assert_eq!(code, 0);
        assert!(
            !home.stato().join(VERSION_SEEN).exists(),
            "la valvola non deve aver toccato niente"
        );
    }

    #[test]
    fn wants_run_is_checked_before_touching_any_state() {
        // Il filtro sulla sorgente vive in `guards::ronda_trigger::wants_run`,
        // già provato lì per ogni valore: qui si conferma solo che `clear` è
        // fra i respinti, cioè la premessa che rende sensato non chiamare
        // `handle_*_trigger` da `run()` su quella sorgente.
        assert!(!judge::wants_run("clear"));
        assert!(!judge::wants_run("compact"));
    }
}
