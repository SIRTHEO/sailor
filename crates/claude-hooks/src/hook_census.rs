//! Chi lancia cosa nella configurazione, nelle due direzioni.
//!
//! Porta di `scripts/hook-census.py`. Le due domande restano quelle:
//! 1. ogni gancio di `settings.json` punta a uno script che esiste?
//! 2. ogni script sotto `~/.claude` è richiamato da qualcuno?
//!
//! PERCHÉ QUESTO PORTO NON GUADAGNA TEMPO D'AVVIO, e va detto. Gli altri ganci
//! si portano perché stanno su `PreToolUse`/`PostToolUse` e pagano un avvio di
//! interprete a ogni chiamata; questo gira **una volta a fine sessione, in
//! sottofondo**. Qui il Python costa ~50 ms su un lavoro da due minuti. Si porta
//! perché il mandato vuole fuori la logica operativa da Python, non perché sia
//! caldo — e il costo vero, la passata di `grep` su trenta radici, resta identico
//! perché è lo stesso `grep`.
//!
//! LA RICERCA RESTA `grep`, di proposito. Riscriverla come cammino del
//! filesystem in Rust sarebbe più veloce, ma cambierebbe *quali* file vengono
//! letti (binari, collegamenti, cartelle escluse) e con essi il risultato: non
//! sarebbe più un porto, sarebbe uno strumento nuovo che va giudicato da capo.
//! Il confronto di equivalenza pretende lo stesso oracolo.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Cartelle che non sono un punto d'esecuzione: nessuno lancia niente da lì, ci
/// si scrive soltanto. Escluderle serve alla correttezza prima che alla
/// velocità — `file-history/` e `homunculus/` sono derivate dalle sessioni, e
/// uno script si dichiarava vivo perché una sessione morta l'aveva nominato una
/// volta.
const NOT_RUN: &[&str] = &[
    "projects",
    "file-history",
    "homunculus",
    "security",
    "telemetry",
    "shell-snapshots",
    "session-env",
    "paste-cache",
    "todos",
    "history",
    "statsig",
    "backups",
    "state",
];

/// Estensioni che *eseguono* ciò che nominano. Essere nominato non è essere
/// lanciato: un documento che cita uno script non lo tiene vivo. `CLAUDE.md` è
/// l'eccezione motivata — è la fonte delle procedure, e il comando scritto lì
/// viene digitato davvero.
const RUNS: &[&str] = &[
    ".py",
    ".sh",
    ".mjs",
    ".js",
    ".json",
    ".yml",
    ".yaml",
    ".plist",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".bashrc",
    "CLAUDE.md",
    "pre-commit",
    "pre-push",
];

/// Esegue un comando con una scadenza, e restituisce il suo stdout.
///
/// `Command::output()` non sa scadere, e `wait_timeout` sarebbe una dipendenza
/// in più per una sola chiamata: si sorveglia il figlio con `try_wait`. Il testo
/// dell'errore è quello che il Python stampa (`TimeoutExpired`), perché è la
/// riga che finisce nel rapporto e che il confronto legge.
fn run_with_timeout(cmd: &mut Command, seconds: u64) -> Result<String, String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{e}"))?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("TimeoutExpired".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("{e}")),
        }
    }
    let mut out = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read as _;
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        out = String::from_utf8_lossy(&buf).into_owned();
    }
    Ok(out)
}

fn home() -> PathBuf {
    // La HOME si inietta per le prove: l'originale ha il percorso scritto in
    // chiaro, e un confronto che non può spostarla misurerebbe il disco vero.
    match std::env::var("CLAUDE_CENSUS_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from("/home/someone/.claude"),
    }
}

fn real(p: &Path) -> String {
    fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Il fiuto che riconosce «qui c'è un percorso a uno script», ovunque nella
/// riga di comando — non solo al primo token. È la correzione che rende
/// innocui `nohup`, `if [ -f … ]` e `cd "$VAR" 2>/dev/null;`: un censimento
/// che guardasse solo il primo token dichiarerebbe rotto un gancio il cui
/// eseguibile vero (spesso il binario Rust, senza estensione tracciata) non è
/// nemmeno il bersaglio di questa ricerca.
fn hook_path_regex() -> Regex {
    Regex::new(r#"["']?(/[\w./-]+\.(?:py|sh|mjs|js))["']?"#).expect("regex valida")
}

/// Dai comandi dichiarati estrae i percorsi con estensione tracciata e li
/// smista: `wired` è l'insieme canonico di ciò che i ganci nominano, `dead`
/// sono le comparse che puntano a un file assente — una per ogni comparsa, non
/// deduplicate, perché è lo stesso comando a ripetere il percorso più volte
/// (un `if [ -f … ]` lo nomina nel test e di nuovo nella chiamata).
fn scan_hooks(
    hooks: &[(String, String, String)],
    re: &Regex,
) -> (BTreeSet<String>, Vec<(String, String, String)>) {
    let mut wired: BTreeSet<String> = BTreeSet::new();
    let mut dead: Vec<(String, String, String)> = Vec::new();
    for (event, matcher, cmd) in hooks {
        for c in re.captures_iter(cmd) {
            let p = c[1].to_string();
            wired.insert(real(Path::new(&p)));
            if !Path::new(&p).is_file() {
                dead.push((event.clone(), matcher.clone(), p));
            }
        }
    }
    (wired, dead)
}

/// L'output di `grep -rHo -f needle_file` riga per riga: ogni riga è
/// `percorso:ago`, e l'ultimo `:` separa i due perché l'ago non ne contiene
/// mai (sono nomi di file) mentre il percorso, sotto macOS, in teoria sì.
fn parse_named_in(raw: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut named_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in raw.lines() {
        let Some(pos) = line.rfind(':') else { continue };
        let (where_, needle) = (&line[..pos], &line[pos + 1..]);
        if where_.is_empty() || needle.is_empty() {
            continue;
        }
        named_in
            .entry(needle.to_string())
            .or_default()
            .insert(where_.to_string());
    }
    named_in
}

/// Per ogni script da controllare decide se è raggiungibile: assente da ogni
/// parte tranne se stesso («nobody names it»), oppure nominato solo da chi non
/// esegue — un `.md` fuori da `RUNS`, mai da uno script o un lanciatore vero
/// («only mentioned in prose»). Uno script nominato da un file in `RUNS` non
/// finisce in questo elenco.
fn find_orphans<'a>(
    to_search: &'a [PathBuf],
    needles_of: &BTreeMap<PathBuf, Vec<String>>,
    named_in: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<(&'a PathBuf, String)> {
    let mut orphans: Vec<(&PathBuf, String)> = Vec::new();
    for p in to_search {
        let mut found: BTreeSet<String> = BTreeSet::new();
        for needle in &needles_of[p] {
            if let Some(files) = named_in.get(needle) {
                found.extend(files.iter().cloned());
            }
        }
        let mine = real(p);
        let others: Vec<&String> = found.iter().filter(|r| real(Path::new(r)) != mine).collect();
        let raises = others
            .iter()
            .any(|a| RUNS.iter().any(|suffix| a.ends_with(suffix)));
        if others.is_empty() {
            orphans.push((p, "nobody names it".into()));
        } else if !raises {
            let names: BTreeSet<String> = others
                .iter()
                .map(|a| {
                    Path::new(a)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            let three: Vec<String> = names.into_iter().take(3).collect();
            orphans.push((p, format!("only mentioned in prose ({})", three.join(", "))));
        }
    }
    orphans
}

/// Una sola passata di `grep` sulle radici indicate: le cartelle in
/// `exclude_dirs` non contano come punti d'esecuzione (essere *nominato* lì
/// non è essere *lanciato*), e la scadenza è la stessa della produzione.
/// Isolata per poterla puntare a un albero di prova senza pagare i trenta
/// posti veri — la stessa ragione per cui `home()` e le radici si iniettano
/// da variabile d'ambiente.
fn search_named_in(
    roots: &[String],
    needle_file: &Path,
    exclude_dirs: &[&str],
    timeout_sec: u64,
) -> String {
    let mut cmd = Command::new("grep");
    cmd.arg("-rHo")
        .arg("--exclude-dir=node_modules")
        .arg("--exclude-dir=.git");
    for d in exclude_dirs {
        cmd.arg(format!("--exclude-dir={d}"));
    }
    cmd.arg("-F").arg("-f").arg(needle_file);
    for r in roots {
        cmd.arg(r);
    }
    match run_with_timeout(&mut cmd, timeout_sec) {
        Ok(out) => out,
        Err(kind) => {
            println!("  search did not answer ({kind}): census incomplete");
            String::new()
        }
    }
}

/// I ganci dichiarati: (evento, matcher, comando).
fn declared_hooks(settings: &serde_json::Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) else {
        return out;
    };
    for (event, blocks) in hooks {
        let Some(blocks) = blocks.as_array() else { continue };
        for b in blocks {
            let matcher = b
                .get("matcher")
                .and_then(|m| m.as_str())
                .unwrap_or("*")
                .to_string();
            let Some(inner) = b.get("hooks").and_then(|h| h.as_array()) else {
                continue;
            };
            for h in inner {
                let cmd = h
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push((event.clone(), matcher.clone(), cmd));
            }
        }
    }
    out
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let fast = args.iter().any(|a| a == "--fast");
    let update = args.iter().any(|a| a == "--update");
    let h = home();

    let settings = match fs::read_to_string(h.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    {
        Some(v) => v,
        None => {
            eprintln!("hook-census: settings.json illeggibile");
            return 0;
        }
    };

    let hooks = declared_hooks(&settings);

    // Stesso schema dell'originale: un percorso assoluto che finisce con
    // un'estensione eseguibile, con o senza virgolette attorno.
    let re = hook_path_regex();
    let (wired, dead) = scan_hooks(&hooks, &re);
    let broken = dead.len();

    // `--fast`: la sola direzione 1, per `SessionStart`. Tace quando va tutto
    // bene — un avviso a ogni apertura si smette di leggere in due giorni.
    if fast {
        if !dead.is_empty() {
            println!(
                "{} ganci registrati puntano a file che non esistono. \
Un PreToolUse in errore rifiuta OGNI comando: se in elenco c'e' un gancio Bash, \
i comandi non partono finche' la riga di settings.json non e' corretta (il file \
e' protetto: la mette Theo).",
                dead.len()
            );
            for (event, matcher, p) in &dead {
                println!("  {event}/{matcher}: {p}");
            }
        }
        return 0;
    }

    println!("═══ 1. hooks declared: {}", hooks.len());
    for (event, matcher, p) in &dead {
        println!("  DEAD   {event}/{matcher}: {p}");
    }
    if broken > 0 {
        println!("  → {broken} broken");
    } else {
        println!("  → every hook script exists");
    }

    // --- 2. script → chi li chiama ------------------------------------------
    let mut scripts: Vec<PathBuf> = Vec::new();
    for base in ["scripts", "skills/hooks", "hooks"] {
        let dir = h.join(base);
        if !dir.is_dir() {
            continue;
        }
        let mut here: Vec<PathBuf> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        here.sort();
        for p in here {
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext, "py" | "sh" | "mjs" | "js") || !p.is_file() {
                continue;
            }
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if name.starts_with("prova-") || name.starts_with("test-") || name.contains(".test.") {
                continue;
            }
            scripts.push(p);
        }
    }

    // I posti da cui qualcosa parte davvero. Le ultime righe sono arrivate il
    // 14/08/2026, dopo che il censimento aveva dichiarato orfani quattro
    // strumenti su sette che orfani non erano: un censimento cieco
    // sull'esecutore non è inutile, è peggio — fa cancellare roba viva.
    let user = std::env::var("HOME").unwrap_or_else(|_| "/home/someone".into());
    // Anche le radici si iniettano per le prove, e per la stessa ragione della
    // HOME: senza, un confronto camminerebbe sui repo veri — due minuti a giro e
    // un risultato che cambia mentre lo misuri, perché quei file sono di altri.
    let injected = std::env::var("CLAUDE_CENSUS_ROOTS").unwrap_or_default();
    let mut roots: Vec<String> = if !injected.is_empty() {
        injected.split(':').map(String::from).collect()
    } else {
        vec![
        h.to_string_lossy().into_owned(),
        "/home/someone/other-repo/work/.claude".into(),
        format!("{user}/Library/LaunchAgents"),
        "/home/someone/other-repo/work/CLAUDE.md".into(),
        ]
    };
    if injected.is_empty() {
        for n in [".zshrc", ".zprofile", ".zshenv", ".bashrc"] {
            roots.push(format!("{user}/{n}"));
        }
        for r in ["suite", "a-service", "a-client"] {
            roots.push(format!("/home/someone/other-repo/work/{r}/.claude"));
            roots.push(format!("/home/someone/other-repo/work/{r}/.github"));
            roots.push(format!("/home/someone/other-repo/work/{r}/package.json"));
            roots.push(format!("/home/someone/other-repo/work/{r}/CLAUDE.md"));
        }
    }
    roots.retain(|r| Path::new(r).exists());

    println!(
        "\n═══ 2. scripts under ~/.claude: {} (looking in {} places that can launch them)",
        scripts.len(),
        roots.len()
    );

    // Una passata sola di `grep`, non una per script: prima erano ~46 passate
    // sulle stesse radici, 122 secondi, e uno strumento che costa due minuti non
    // viene lanciato — quindi non misura niente.
    let to_search: Vec<PathBuf> = scripts
        .iter()
        .filter(|p| !wired.contains(&real(p)))
        .cloned()
        .collect();
    let mut needles_of: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for p in &to_search {
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut n = vec![name];
        // Un modulo Python si importa col nome nudo (`import hook_log`): cercare
        // solo `hook_log.py` lo dichiarava orfano mentre due ganci lo usavano.
        if p.extension().and_then(|e| e.to_str()) == Some("py") && stem.contains('_') {
            n.push(stem);
        }
        needles_of.insert(p.clone(), n);
    }

    let needle_file = h.join("state").join("census-needles.txt");
    let _ = fs::create_dir_all(needle_file.parent().unwrap_or(Path::new(".")));
    let all: BTreeSet<String> = needles_of.values().flatten().cloned().collect();
    let _ = fs::write(
        &needle_file,
        all.iter().cloned().collect::<Vec<_>>().join("\n") + "\n",
    );

    // IL TIMEOUT SI RIPRODUCE, e non perché sia giusto: perché è il
    // comportamento vero dell'originale, e un porto si misura su quello.
    //
    // IL DIFETTO, misurato il 17/08/2026 eseguendo le due parti sul disco vero.
    // Il `grep` su queste radici impiega **1.008 secondi**; il Python lo uccide a
    // 600, stampa «census incomplete» e prosegue con `raw` vuoto — cioè dichiara
    // orfano **tutto ciò che non ha fatto in tempo a cercare**. Risultato di quel
    // giro: 26 orfani su 53, di cui 18 falsi — `hook_log.py`, `relay.py`,
    // `handoff_common.py`, `guardiano-macchina.sh`, `hunt.sh` sono tutti vivi e
    // hanno un chiamante. Lasciando finire la ricerca gli orfani veri sono 8.
    //
    // È esattamente il guasto che il commento dell'originale dice di voler
    // evitare: «un censimento cieco sull'esecutore non è inutile, è peggio: fa
    // cancellare roba viva». Alzare o togliere il limite cambia cosa dichiara lo
    // strumento, quindi è una decisione di chi lo usa, non di chi lo porta.
    const SEARCH_TIMEOUT_SEC: u64 = 600;
    let raw = search_named_in(&roots, &needle_file, NOT_RUN, SEARCH_TIMEOUT_SEC);

    // ago → i file che lo nominano
    let named_in = parse_named_in(&raw);

    let orphans = find_orphans(&to_search, &needles_of, &named_in);

    for (p, why) in &orphans {
        let rel = p.strip_prefix(&h).unwrap_or(p).to_string_lossy().into_owned();
        let kb = p.metadata().map(|m| m.len() / 1024).unwrap_or(0);
        println!("  ORPHAN  {rel:42} {kb:3} KB  — {why}");
    }
    println!("  → {} orphans out of {}", orphans.len(), scripts.len());

    // --- 3. la linea di base, perché nove orfani noti non sono una notizia ---
    let baseline = h.join("state").join("hook-census.baseline.json");
    let current: Vec<String> = {
        let mut v: Vec<String> = orphans
            .iter()
            .map(|(p, _)| p.strip_prefix(&h).unwrap_or(p).to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };

    if update {
        let _ = fs::create_dir_all(baseline.parent().unwrap_or(Path::new(".")));
        // `json.dumps({"orfani": …}, indent=2)`: la chiave resta in italiano
        // perché il file su disco è già scritto così, e rinominarla renderebbe
        // «nuovi» tutti gli orfani conosciuti al primo giro dopo il porto.
        let payload = serde_json::json!({ "orfani": current });
        let _ = fs::write(
            &baseline,
            serde_json::to_string_pretty(&payload).unwrap_or_default() + "\n",
        );
        println!("\nbaseline updated: {} known orphans.", current.len());
        return 0;
    }

    let known: BTreeSet<String> = fs::read_to_string(&baseline)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.get("orfani")
                .and_then(|o| o.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect())
        })
        .unwrap_or_default();

    let fresh: Vec<&String> = current.iter().filter(|o| !known.contains(*o)).collect();
    if !fresh.is_empty() {
        println!("\n═══ NEW since the baseline: {}", fresh.len());
        for o in &fresh {
            println!("  {o}");
        }
        println!(
            "A tool nobody runs never fails and nobody notices. Wire it up or \
delete it; if this is fine, run `hook-census.py --update`."
        );
    } else if !known.is_empty() {
        println!("\nno new orphans (baseline: {}).", known.len());
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_io::testing::test_root;

    /// Una cartella di prova vuota, unica per caso — il pid la isola dagli
    /// altri processi (`test_root`), il nome dal resto dei casi nello stesso
    /// processo. Nessuna variabile d'ambiente tocca lo stato globale qui
    /// dentro, quindi non serve un lucchetto: `scan_hooks`, `parse_named_in`,
    /// `find_orphans` e `search_named_in` ricevono tutto per argomento.
    fn test_scratch_dir(name: &str) -> PathBuf {
        let dir = test_root().join("hook_census").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("cartella di prova");
        dir
    }

    // --- scan_hooks: direzione 1, gancio → file ------------------------

    #[test]
    fn scan_hooks_does_not_flag_a_hook_pointing_to_an_existing_file() {
        let dir = test_scratch_dir("scan_existing");
        let script = dir.join("present.sh");
        fs::write(&script, "#!/bin/sh\necho ok\n").expect("scrittura script");

        let cmd = format!("{} --flag", script.display());
        let hooks = vec![("PreToolUse".to_string(), "Bash".to_string(), cmd)];
        let (wired, dead) = scan_hooks(&hooks, &hook_path_regex());

        assert!(dead.is_empty(), "un file che esiste non è la ragione d'essere dell'allarme");
        assert_eq!(wired.len(), 1);
    }

    #[test]
    fn scan_hooks_flags_a_hook_pointing_to_a_missing_file() {
        let dir = test_scratch_dir("scan_missing");
        let script = dir.join("gone.sh"); // non lo scrivo: deve mancare davvero

        let cmd = format!("{} --flag", script.display());
        let hooks = vec![("SessionEnd".to_string(), "*".to_string(), cmd)];
        let (_, dead) = scan_hooks(&hooks, &hook_path_regex());

        assert_eq!(dead.len(), 1, "è questa la riga che il rapporto deve poter mostrare");
        assert_eq!(dead[0].2, script.to_string_lossy());
    }

    #[test]
    fn scan_hooks_ignores_the_extensionless_binary_behind_nohup_and_cd() {
        // Le due forme vere di settings.json: il bersaglio reale è il binario
        // Rust, che non ha un'estensione tracciata — nessuna comparsa, né
        // morta né viva. Un censimento che leggesse il primo token del
        // comando invece li avrebbe dichiarati rotti.
        let hooks = vec![
            (
                "SessionEnd".to_string(),
                "*".to_string(),
                "nohup /home/someone/.claude/rust/target/release/claude-hooks work-status --riconcilia > /dev/null 2>&1 &".to_string(),
            ),
            (
                "PreToolUse".to_string(),
                "Bash".to_string(),
                "cd \"$CLAUDE_PROJECT_DIR\" 2>/dev/null; /home/someone/.claude/rust/target/release/claude-hooks orca-cleanup --close --quiet 2>/dev/null || true".to_string(),
            ),
        ];
        let (wired, dead) = scan_hooks(&hooks, &hook_path_regex());
        assert!(wired.is_empty());
        assert!(dead.is_empty());
    }

    #[test]
    fn scan_hooks_is_not_confused_inside_an_if_test_f_for_an_existing_script() {
        let dir = test_scratch_dir("scan_if_existing");
        let script = dir.join("claude-hook.sh");
        fs::write(&script, "#!/bin/sh\n").expect("scrittura script");
        let p = script.display();
        let cmd = format!("if [ -f '{p}' ] && [ -x '{p}' ]; then /bin/sh '{p}'; else :; fi");
        let hooks = vec![("Stop".to_string(), "*".to_string(), cmd)];
        let (wired, dead) = scan_hooks(&hooks, &hook_path_regex());

        assert!(dead.is_empty(), "il percorso ripetuto tre volte resta un file vero");
        assert_eq!(wired.len(), 1, "le tre comparse dello stesso file si deduplicano in wired");
    }

    #[test]
    fn scan_hooks_flags_a_missing_script_even_inside_an_if_test_f() {
        let dir = test_scratch_dir("scan_if_missing");
        let script = dir.join("claude-hook.sh"); // non scritto
        let p = script.display();
        let cmd = format!("if [ -f '{p}' ] && [ -x '{p}' ]; then /bin/sh '{p}'; else :; fi");
        let hooks = vec![("Stop".to_string(), "*".to_string(), cmd)];
        let (_, dead) = scan_hooks(&hooks, &hook_path_regex());

        assert!(!dead.is_empty(), "un gancio rotto non deve sfuggire perché è annidato in un `if`");
    }

    // --- declared_hooks: lettura di settings.json -----------------------

    #[test]
    fn declared_hooks_reads_event_matcher_and_command() {
        let settings: serde_json::Value = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": "/tmp/uno.sh" }]
                    }
                ],
                "SessionEnd": [
                    {
                        "hooks": [{ "type": "command", "command": "/tmp/due.sh" }]
                    }
                ]
            }
        });
        let hooks = declared_hooks(&settings);
        assert_eq!(hooks.len(), 2);
        assert!(hooks.contains(&("PreToolUse".to_string(), "Bash".to_string(), "/tmp/uno.sh".to_string())));
        // Un blocco senza "matcher" vale per ogni strumento: il valore di
        // riserva è "*", non una stringa vuota.
        assert!(hooks.contains(&("SessionEnd".to_string(), "*".to_string(), "/tmp/due.sh".to_string())));
    }

    // --- parse_named_in: dall'output grezzo di grep ----------------------

    #[test]
    fn parse_named_in_groups_multiple_files_under_the_same_needle() {
        let raw = "/a/one.sh:needle.sh\n/b/two.sh:needle.sh\n";
        let named_in = parse_named_in(raw);
        let files = named_in.get("needle.sh").expect("l'ago deve comparire");
        assert_eq!(files.len(), 2);
        assert!(files.contains("/a/one.sh"));
        assert!(files.contains("/b/two.sh"));
    }

    #[test]
    fn parse_named_in_discards_lines_without_needle_or_path() {
        let raw = "line without a colon\n:orphan-without-a-path\n/path/without/needle:\n/a/valid.sh:needle.sh\n";
        let named_in = parse_named_in(raw);
        assert_eq!(named_in.len(), 1, "solo la riga con percorso e ago entrambi non vuoti conta");
        assert!(named_in.contains_key("needle.sh"));
    }

    // --- find_orphans: la seconda direzione, script → chi lo chiama -----

    #[test]
    fn find_orphans_flags_a_script_nobody_names() {
        let script = PathBuf::from("/fake/scripts/isolated.sh");
        let to_search = vec![script.clone()];
        let mut needles_of = BTreeMap::new();
        needles_of.insert(script.clone(), vec!["isolated.sh".to_string()]);
        let named_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        let orphans = find_orphans(&to_search, &needles_of, &named_in);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].0, &script);
        assert_eq!(orphans[0].1, "nobody names it");
    }

    #[test]
    fn find_orphans_does_not_flag_a_script_named_by_a_launcher() {
        let script = PathBuf::from("/fake/scripts/called.sh");
        let to_search = vec![script.clone()];
        let mut needles_of = BTreeMap::new();
        needles_of.insert(script.clone(), vec!["called.sh".to_string()]);
        let mut named_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        named_in.insert(
            "called.sh".to_string(),
            BTreeSet::from(["/fake/launcher.sh".to_string()]),
        );

        let orphans = find_orphans(&to_search, &needles_of, &named_in);
        assert!(orphans.is_empty(), "un .sh è in RUNS: lo tiene vivo");
    }

    #[test]
    fn find_orphans_flags_a_script_named_only_in_prose() {
        let script = PathBuf::from("/fake/scripts/mentioned.sh");
        let to_search = vec![script.clone()];
        let mut needles_of = BTreeMap::new();
        needles_of.insert(script.clone(), vec!["mentioned.sh".to_string()]);
        let mut named_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        named_in.insert(
            "mentioned.sh".to_string(),
            BTreeSet::from(["/fake/note.md".to_string()]),
        );

        let orphans = find_orphans(&to_search, &needles_of, &named_in);
        assert_eq!(orphans.len(), 1);
        assert!(orphans[0].1.starts_with("only mentioned in prose"));
        assert!(orphans[0].1.contains("note.md"));
    }

    #[test]
    fn find_orphans_ignores_self_mention() {
        let script = PathBuf::from("/fake/scripts/reflexive.sh");
        let to_search = vec![script.clone()];
        let mut needles_of = BTreeMap::new();
        needles_of.insert(script.clone(), vec!["reflexive.sh".to_string()]);
        let mut named_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        // Il grep trova l'ago dentro lo script stesso (si nomina in un
        // commento): non conta come chiamante, o ogni script si terrebbe
        // vivo da solo.
        named_in.insert(
            "reflexive.sh".to_string(),
            BTreeSet::from([script.to_string_lossy().into_owned()]),
        );

        let orphans = find_orphans(&to_search, &needles_of, &named_in);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].1, "nobody names it");
    }

    // --- search_named_in: la singola passata di grep, e NOT_RUN ---------

    #[test]
    fn search_named_in_finds_the_needle_in_a_normal_folder() {
        let dir = test_scratch_dir("grep_normal");
        fs::create_dir_all(dir.join("normal")).expect("sottocartella");
        fs::write(dir.join("normal").join("caller.sh"), "target.sh\n").expect("scrittura");

        // Fuori dalla radice cercata: dentro ci finirebbe anche se stesso,
        // visto che il suo contenuto è proprio l'ago che cerca.
        let needle_file = dir.with_file_name("grep_normal-needles.txt");
        fs::write(&needle_file, "target.sh\n").expect("scrittura aghi");

        let roots = vec![dir.to_string_lossy().into_owned()];
        let raw = search_named_in(&roots, &needle_file, &[], 5);
        assert!(raw.contains("caller.sh:target.sh"), "raw={raw}");
    }

    #[test]
    fn search_named_in_does_not_find_the_needle_inside_an_excluded_folder() {
        let dir = test_scratch_dir("grep_excluded");
        fs::create_dir_all(dir.join("state")).expect("sottocartella");
        fs::write(dir.join("state").join("log.txt"), "target.sh\n").expect("scrittura");

        // Fuori dalla radice cercata, per lo stesso motivo dell'altra prova.
        let needle_file = dir.with_file_name("grep_excluded-needles.txt");
        fs::write(&needle_file, "target.sh\n").expect("scrittura aghi");

        let roots = vec![dir.to_string_lossy().into_owned()];
        // NOT_RUN è la lista vera usata in produzione: "state" ci sta dentro
        // apposta, perché è dove il censimento stesso scrive.
        let raw = search_named_in(&roots, &needle_file, NOT_RUN, 5);
        assert!(!raw.contains("target.sh"), "essere nominato in una cartella esclusa non è essere lanciato: raw={raw}");
    }
}
