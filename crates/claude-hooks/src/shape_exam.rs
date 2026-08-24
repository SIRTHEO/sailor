//! L'esame della forma, lato disco: cammina l'albero, legge `ganci.jsonl` e
//! `settings.json`, interroga `git`, conserva la serie storica. Il giudizio —
//! sonde, soglie, tendenza, verdetto singolo — sta tutto in
//! `guards::shape_exam`, dove si prova senza filesystem.
//!
//! NON CORREGGE NIENTE: osserva e dice. Non apre pannelli, non lancia agenti,
//! non tocca `settings.json` — se serve una riga la scrive in coda.
//!
//! Uso:
//!     claude-hooks shape [--report] [--dry] [--if-moved] [--queue]

use guards::shape_exam as judge;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// La riga che Theo dovrà aggiungere a `settings.json` — mai scritta lì da qui,
/// solo citata nella voce che finisce in coda.
const SETTINGS_LINE: &str = r#"{"type": "command", "command": "/home/someone/.claude/rust/target/release/claude-hooks shape --if-moved --queue", "timeout": 20}"#;

const SERIES: &str = "forma.jsonl";
const SAID: &str = "forma-detti.jsonl";
const WATERMARK: &str = "forma-ultimo-commit";
const QUEUE_ENTRY: &str = "AUTO-esame-della-forma.md";

/// Quanti giorni di registro guardare. Un mese è la finestra in cui «questo
/// freno non ha mai negato» smette di essere un caso e diventa un fatto.
const JOURNAL_DAYS: i64 = 30;

/// Le cartelle che non sono codice di questa casa: generate dall'harness,
/// installate da terzi, o archivi. Contarle direbbe che la casa è dieci volte
/// più grande di quanto sia, e la misura che ne uscirebbe non guiderebbe niente.
const SKIP: &[&str] = &[
    "plugins",
    "session-env",
    "shell-snapshots",
    "file-history",
    "backups",
    "security",
    "projects",
    "node_modules",
    "paste-cache",
    "cache",
    "downloads",
    "debug",
    "patches",
    ".git",
    "__pycache__",
];

fn state_dir() -> PathBuf {
    crate::register_session::state_dir()
}

fn claude_dir() -> PathBuf {
    state_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn queue_dir() -> PathBuf {
    state_dir().join("plancia").join("segnalazioni")
}

fn today() -> String {
    hook_io::local_time::now_local_iso8601()
        .chars()
        .take(10)
        .collect()
}

/// Una cartella si salta se è nell'elenco, se è un archivio, o se è un albero
/// di compilazione. `target` e `target-verifica` sono due, e prenderli col
/// prefisso evita che il terzo sfugga.
fn skip_dir(name: &str) -> bool {
    SKIP.contains(&name)
        || name.starts_with("target")
        || name.starts_with("archive-")
        || name.starts_with("potatura-")
        || name.contains("-ritirati-")
        || name.contains("-rimosse-")
}

/// I collegamenti simbolici non si seguono: sotto `~/.claude` ce ne sono che
/// espongono alberi installati altrove, e seguirli conterebbe due volte lo
/// stesso codice — o girerebbe in tondo.
fn walk_code(root: &Path, out: &mut Vec<judge::FileShape>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if meta.is_dir() {
            if !skip_dir(&name) {
                walk_code(&path, out);
            }
            continue;
        }
        let display = path.to_string_lossy().to_string();
        let Some(lang) = judge::language_of(&display) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        out.push(judge::measure_file(&display, lang, &text));
    }
}

/// Le righe vere del registro degli ultimi `JOURNAL_DAYS` giorni. Il file è
/// grande e ordinato nel tempo, ma non si può tagliare a metà: una rotazione
/// lascia in testa righe più vecchie, quindi si filtra per data, non per
/// posizione.
fn read_journal(cutoff: &str) -> Vec<judge::JournalRow> {
    let path = state_dir().join("ganci.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(judge::parse_journal_line)
        .filter(|r| r.day.as_str() >= cutoff)
        .collect()
}

fn wired_hooks() -> BTreeSet<String> {
    let path = claude_dir().join("settings.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return BTreeSet::new();
    };
    let mut commands = Vec::new();
    collect_commands(value.get("hooks").unwrap_or(&serde_json::Value::Null), &mut commands);
    commands
        .iter()
        .filter_map(|c| judge::wired_slug(c))
        .collect()
}

fn collect_commands(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(serde_json::Value::as_str) == Some("command") {
                if let Some(c) = map.get("command").and_then(serde_json::Value::as_str) {
                    out.push(c.to_string());
                }
            }
            for v in map.values() {
                collect_commands(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_commands(v, out);
            }
        }
        _ => {}
    }
}

/// Le righe aggiunte negli ultimi sette giorni, per cartella di primo livello.
fn weekly_growth() -> Vec<judge::Growth> {
    let out = Command::new("git")
        .arg("-C")
        .arg(claude_dir())
        .args([
            "log",
            "--since=7 days ago",
            "--numstat",
            "--format=",
            "--no-renames",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut per_folder: BTreeMap<String, i64> = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(added), Some(_), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Ok(added) = added.parse::<i64>() else {
            continue; // `-` è un file binario: non ha righe da contare
        };
        let folder = path.split('/').next().unwrap_or("(radice)");
        let folder = if path.contains('/') { folder } else { "(radice)" };
        *per_folder.entry(folder.to_string()).or_default() += added;
    }
    per_folder
        .into_iter()
        .map(|(folder, added)| judge::Growth { folder, added })
        .collect()
}

// ─── La serie storica: senza, questo strumento è un rapporto in più ──────────

fn read_series() -> BTreeMap<String, Vec<i64>> {
    let mut history: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(state_dir().join(SERIES)) else {
        return history;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        let Some(measures) = value.get("misure").and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (probe, number) in measures {
            if let Some(n) = number.as_i64() {
                history.entry(probe.clone()).or_default().push(n);
            }
        }
    }
    history
}

fn append_series(day: &str, measures: &BTreeMap<String, i64>) {
    let mut object = serde_json::Map::new();
    object.insert("t".into(), day.into());
    object.insert("commit".into(), head_commit().into());
    let mut inner = serde_json::Map::new();
    for (probe, number) in measures {
        inner.insert(probe.clone(), (*number).into());
    }
    object.insert("misure".into(), serde_json::Value::Object(inner));
    append_line(&state_dir().join(SERIES), &serde_json::Value::Object(object).to_string());
}

fn read_said() -> Vec<judge::Said> {
    let Ok(text) = std::fs::read_to_string(state_dir().join(SAID)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
            let field = |k: &str| {
                value
                    .get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            let day = field("t");
            if day.len() < 10 {
                return None;
            }
            Some(judge::Said {
                day: day[..10].to_string(),
                probe: field("sonda"),
                subject: field("soggetto"),
                number: value.get("numero").and_then(serde_json::Value::as_i64)?,
            })
        })
        .collect()
}

fn append_said(day: &str, finding: &judge::Finding) {
    let line = serde_json::json!({
        "t": day,
        "sonda": finding.probe,
        "soggetto": finding.subject,
        "numero": finding.number,
    });
    append_line(&state_dir().join(SAID), &line.to_string());
}

fn append_line(path: &Path, line: &str) {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

// ─── L'innesco: una soglia, non un orologio ──────────────────────────────────

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

/// Quante righe si sono mosse dall'ultimo esame. `None` quando non lo sappiamo
/// — nessun watermark, o un commit che non esiste più dopo uno schiacciamento —
/// e un «non lo so» non sveglia niente: si registra il punto e si tace.
fn lines_moved() -> Option<i64> {
    let mark = std::fs::read_to_string(state_dir().join(WATERMARK)).ok()?;
    let mark = mark.trim().to_string();
    if mark.is_empty() {
        return None;
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(claude_dir())
        .args(["diff", "--numstat", &mark, "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut moved = 0;
    for line in text.lines() {
        let mut parts = line.split('\t');
        if let (Some(a), Some(d)) = (parts.next(), parts.next()) {
            moved += a.parse::<i64>().unwrap_or(0) + d.parse::<i64>().unwrap_or(0);
        }
    }
    Some(moved)
}

fn write_watermark() {
    let head = head_commit();
    if !head.is_empty() {
        let _ = std::fs::create_dir_all(state_dir());
        let _ = std::fs::write(state_dir().join(WATERMARK), head);
    }
}

fn entry_is_open() -> bool {
    std::fs::read_to_string(queue_dir().join(QUEUE_ENTRY))
        .map(|t| guards::ronda_trigger::already_open(&t))
        .unwrap_or(false)
}

/// Una voce sola, con il numero, la direzione e cosa succede se resta com'è.
fn write_queue_entry(day: &str, verdict: &judge::Verdict) {
    let judge::Verdict::One {
        finding,
        trend: direction,
        ..
    } = verdict
    else {
        return;
    };
    let body = format!(
        "---\n\
sessione: esame-della-forma (automazione, nessuna sessione)\n\
albero: -\n\
quando: {day}\n\
stato: aperta\n\
per: la sessione generale — esame della forma\n\
---\n\
\n\
# {}\n\
\n\
**Chi ha scritto questa voce**: nessuno. `claude-hooks shape` si è svegliato\n\
perché da un esame all'altro si sono mosse almeno {} righe, ha misurato la\n\
forma di questa casa e questa è la cosa sola che vale di più adesso.\n\
\n\
- **Misura**: {} {}\n\
- **Soggetto**: {}\n\
- **{}**\n\
- **Se resta com'è**: {}\n\
\n\
Il rapporto intero: `claude-hooks shape --report --dry`. La serie storica sta\n\
in `state/{SERIES}`, quello che questo esame ha già detto in `state/{SAID}`.\n\
\n\
**Riga in `settings.json`** (classe MAI: la aggiunge solo Theo), nel gruppo\n\
`SessionStart` con `\"matcher\": \"startup|resume\"`:\n\
\n\
    {SETTINGS_LINE}\n",
        finding.headline,
        judge::MOVEMENT_THRESHOLD,
        finding.number,
        finding.unit,
        finding.subject,
        direction.describe(),
        finding.consequence,
    );
    if std::fs::create_dir_all(queue_dir()).is_err() {
        return;
    }
    let _ = std::fs::write(queue_dir().join(QUEUE_ENTRY), body);
}

// ─── L'esame ─────────────────────────────────────────────────────────────────

/// Tutte le sonde, nell'ordine in cui compaiono nel rapporto lungo.
fn examine(day: &str) -> (Vec<judge::Finding>, BTreeMap<String, i64>) {
    let mut files = Vec::new();
    walk_code(&claude_dir(), &mut files);

    // Senza data di taglio leggibile si legge tutto il registro: una finestra
    // troppo larga fa sembrare mordace un gate spento da settimane, ma tacere
    // del tutto sarebbe peggio.
    let cutoff = shift_days_back(day, JOURNAL_DAYS).unwrap_or_default();
    let rows = read_journal(&cutoff);

    let mut findings = vec![
        judge::probe_deciding_outside(&files),
        judge::probe_unproven_bulk(&files),
        judge::probe_mute_guards(&wired_hooks(), &rows),
        judge::probe_repeated_denials(&rows),
        judge::probe_fastest_growth(&weekly_growth()),
    ];

    // Le misure di adesso servono a `probe_unheeded` per sapere se una vecchia
    // indicazione ha mosso qualcosa: si costruiscono prima di aggiungerla.
    let measures: BTreeMap<String, i64> = findings
        .iter()
        .map(|f| (f.probe.to_string(), f.number))
        .collect();
    if let Some(unheeded) = judge::probe_unheeded(&read_said(), day, &measures) {
        findings.push(unheeded);
    }
    (findings, measures)
}

/// La data di `n` giorni fa, nella forma con cui il registro scrive i propri
/// giorni. Un passo alla volta: la finestra è di un mese, e trenta passi
/// costano meno di un secondo algoritmo del calendario da tenere allineato.
fn shift_days_back(day: &str, n: i64) -> Option<String> {
    let mut current = day.to_string();
    for _ in 0..n {
        current = shift_one_day_back(&current)?;
    }
    Some(current)
}

fn shift_one_day_back(day: &str) -> Option<String> {
    let y = day[..4].parse::<i64>().ok()?;
    let m = day[5..7].parse::<u32>().ok()?;
    let d = day[8..10].parse::<u32>().ok()?;
    if d > 1 {
        return Some(format!("{y:04}-{m:02}-{:02}", d - 1));
    }
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    // L'ultimo giorno del mese precedente si trova provando all'indietro: 31,
    // 30, 29, 28 — quattro tentativi al massimo, e nessuna tabella dei bisestili.
    for last in (28..=31).rev() {
        let candidate = format!("{py:04}-{pm:02}-{last:02}");
        if judge::day_number(&candidate).is_some_and(|n| {
            judge::day_number(day).is_some_and(|today| n == today - 1)
        }) {
            return Some(candidate);
        }
    }
    None
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let dry = has("--dry");
    let wants_report = has("--report");

    if has("--if-moved") {
        let moved = lines_moved();
        if !judge::should_examine(moved) {
            if !dry {
                // Anche quando non si esamina il punto si registra: senza,
                // la prima esecuzione resterebbe «prima esecuzione» per sempre.
                if moved.is_none() {
                    write_watermark();
                }
            }
            return 0;
        }
    }

    let day = today();
    let (findings, measures) = examine(&day);
    let history = read_series();
    let verdict = judge::rank(findings.clone(), &history);

    println!("{}", judge::render_verdict(&day, &verdict));
    if wants_report {
        println!("{}", judge::render_report(&findings, &history));
    }

    if dry {
        return 0;
    }
    append_series(&day, &measures);
    write_watermark();
    if let judge::Verdict::One { finding, .. } = &verdict {
        // Il detto è la traccia su cui, fra due settimane, `probe_unheeded`
        // giudica se qualcuno ha fatto qualcosa: si legge PRIMA di scrivere,
        // o il confronto sarebbe con la riga appena messa.
        let last = read_said().pop();
        if judge::should_record_said(last.as_ref(), &day, finding) {
            append_said(&day, finding);
        }
        if has("--queue") && judge::should_file_entry(&verdict, entry_is_open(), last.as_ref()) {
            write_queue_entry(&day, &verdict);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    #[test]
    fn generated_and_vendored_trees_are_never_counted() {
        // Sono le cartelle che direbbero che questa casa è dieci volte più
        // grande di quanto sia: 1.894 file shell contro i 55 scritti a mano.
        assert!(skip_dir("plugins"));
        assert!(skip_dir("shell-snapshots"));
        assert!(skip_dir("session-env"));
        assert!(skip_dir("target"));
        assert!(skip_dir("target-verifica"));
        assert!(skip_dir("archive-commands"));
        assert!(skip_dir("agents-ritirati-2026-08-23"));
        assert!(!skip_dir("scripts"));
        assert!(!skip_dir("rust"));
        assert!(!skip_dir("skills"));
    }

    #[test]
    fn the_walk_reads_code_and_ignores_everything_else() {
        let home = HomeIsolata::nuova("forma-camminata");
        let root = home.dir.join(".claude");
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("plugins/altrui")).unwrap();
        std::fs::write(root.join("scripts/a.sh"), "if [ -f x ]; then\n echo\nfi\n").unwrap();
        std::fs::write(root.join("scripts/note.md"), "# non e codice\n").unwrap();
        std::fs::write(root.join("plugins/altrui/b.sh"), "if true; then\n echo\nfi\n").unwrap();
        let mut files = Vec::new();
        walk_code(&root, &mut files);
        assert_eq!(files.len(), 1, "un solo file di codice: {files:?}");
        assert!(files[0].path.ends_with("scripts/a.sh"));
    }

    #[test]
    fn the_wired_hooks_come_out_of_settings_json() {
        let home = HomeIsolata::nuova("forma-cablati");
        let root = home.dir.join(".claude");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[
                {"type":"command","command":"/x/claude-hooks cd-guard"},
                {"type":"command","command":"/x/claude-hooks code-language pre"},
                {"type":"command","command":"/x/altro-script.sh"}
            ]}]}}"#,
        )
        .unwrap();
        let wired = wired_hooks();
        assert!(wired.contains("cd-guard"), "{wired:?}");
        assert!(wired.contains("code-language"), "{wired:?}");
        assert_eq!(wired.len(), 2, "uno script che non è del binario non è un gancio");
    }

    #[test]
    fn the_series_survives_from_one_exam_to_the_next() {
        let home = HomeIsolata::nuova("forma-serie");
        let mut first = BTreeMap::new();
        first.insert("deciding-outside".to_string(), 16_000_i64);
        append_series("2026-08-22", &first);
        let mut second = BTreeMap::new();
        second.insert("deciding-outside".to_string(), 16_300_i64);
        append_series("2026-08-23", &second);
        let history = read_series();
        assert_eq!(
            history.get("deciding-outside"),
            Some(&vec![16_000, 16_300]),
            "in ordine, dalla più vecchia"
        );
        assert!(home.stato().join(SERIES).exists());
    }

    #[test]
    fn what_it_said_is_written_down_so_it_can_be_judged_later() {
        let _home = HomeIsolata::nuova("forma-detti");
        let finding = judge::Finding {
            probe: "deciding-outside",
            subject: "scripts/queue-patrol.sh".into(),
            number: 16_304,
            unit: "righe",
            floor: 2_000,
            headline: "h".into(),
            consequence: "c".into(),
        };
        append_said("2026-08-24", &finding);
        let said = read_said();
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].probe, "deciding-outside");
        assert_eq!(said[0].number, 16_304);
        assert_eq!(said[0].day, "2026-08-24");
    }

    #[test]
    fn a_missing_watermark_never_wakes_the_exam() {
        let _home = HomeIsolata::nuova("forma-primo-giro");
        assert_eq!(lines_moved(), None);
        assert!(!judge::should_examine(lines_moved()));
    }

    #[test]
    fn it_walks_the_calendar_backwards_across_a_month_boundary() {
        assert_eq!(shift_one_day_back("2026-08-24").as_deref(), Some("2026-08-23"));
        assert_eq!(shift_one_day_back("2026-08-01").as_deref(), Some("2026-07-31"));
        assert_eq!(shift_one_day_back("2026-03-01").as_deref(), Some("2026-02-28"));
        assert_eq!(shift_one_day_back("2024-03-01").as_deref(), Some("2024-02-29"));
        assert_eq!(shift_one_day_back("2026-01-01").as_deref(), Some("2025-12-31"));
    }
}
