//! `PreToolUse` su Bash: il governo della memoria, deciso da Theo il
//! 24/08/2026 dopo tre chiusure forzate in una notte. Il giudizio puro sta in
//! `guards::memory_governor`; qui c'è solo ciò che tocca la macchina — la
//! lettura di `memory_pressure`, l'elenco dei file di swap, la cache della
//! misura e il registro.
//!
//! COSA RIUSA, E PERCHÉ NON BASTAVA. Le stesse grandezze le annota già
//! `scripts/registro-swap.sh` ogni cinque minuti, e `scripts/guardiano-macchina.sh`
//! guarda già i processi che gonfiano. Nessuno dei due frena — il primo lo dice
//! di sé, «questo script NON frena niente» — e nessuno dei due gira dentro una
//! sessione: girano sotto `launchctl`, dove `sysctl` risponde. Qui serve la
//! stessa misura **dentro il sandbox**, dove `sysctl vm.swapusage` è negato, e
//! serve nell'istante in cui la spesa si decide. Le soglie di quegli script
//! restano la fonte della taratura, non si riscrivono da capo.
//!
//! PERCHÉ È UN GANCIO E NON UN DEMONE. Un processo che consuma per sorvegliare
//! i consumi è una barzelletta, e un demone che frena arriva comunque dopo, a
//! memoria già chiesta. Qui il freno sta all'ingresso del nostro lavoro
//! pesante, e a macchina ferma non gira niente: la misura si rilegge a finestre
//! che si stringono con la pressione (`recheck_after_secs`), quindi in calma
//! cento comandi di fila costano una lettura sola.
//!
//! COSA NON FA MAI: chiudere qualcosa. Non uccide processi, non ferma
//! container, non tocca le applicazioni di Theo. Rifiuta comandi nostri e
//! parla; il resto è una decisione sua.
//!
//! Valvola: `MEMORY_GOVERNOR=off`.

use guards::memory_governor as judge;
use std::path::PathBuf;
use std::process::Command;

/// La riga che Theo dovrà aggiungere a `settings.json` — mai scritta lì da qui.
pub const SETTINGS_LINE: &str = r#"{"type": "command", "command": "/Users/theo/.claude/rust/target/release/claude-hooks memory-governor", "timeout": 5}"#;

/// La cache dell'ultima misura: epoch, livello, i quattro numeri, le fonti
/// cieche. `-` sta per «non misurato», che non è zero.
const CACHE: &str = "memory-governor-last";

fn state_dir() -> PathBuf {
    crate::register_session::state_dir()
}

/// Dove il kernel tiene i file di swap. macOS ne aggiunge uno solo quando
/// quelli che ha sono pieni, quindi lo spazio allocato qui dentro è il segnale
/// di saturazione meno ambiguo che questa macchina offra — ed è l'unico che,
/// al contrario di `sysctl vm.swapusage`, il sandbox lascia leggere.
const SWAP_DIR: &str = "/System/Volumes/VM";

/// Le soglie, con l'ambiente che può stringerle. Serve per collaudare il freno
/// senza saturare davvero i 18 GB: è lo stesso espediente di `GM_ETA_MIN` in
/// `guardiano-macchina.sh`, dove una regola che chiude processi dopo due ore
/// non si poteva provare aspettando due ore.
fn thresholds() -> judge::Thresholds {
    let base = judge::Thresholds::default();
    let get = |name: &str, fallback: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(fallback)
    };
    judge::Thresholds {
        available_tight_mb: get("MEMGOV_AVAILABLE_TIGHT_MB", base.available_tight_mb),
        available_critical_mb: get("MEMGOV_AVAILABLE_CRITICAL_MB", base.available_critical_mb),
        free_tight_mb: get("MEMGOV_FREE_TIGHT_MB", base.free_tight_mb),
        free_critical_mb: get("MEMGOV_FREE_CRITICAL_MB", base.free_critical_mb),
        compressor_tight_pct: get("MEMGOV_COMPRESSOR_TIGHT_PCT", base.compressor_tight_pct),
        compressor_critical_pct: get(
            "MEMGOV_COMPRESSOR_CRITICAL_PCT",
            base.compressor_critical_pct,
        ),
        swap_tight_mb: get("MEMGOV_SWAP_TIGHT_MB", base.swap_tight_mb),
        swap_critical_mb: get("MEMGOV_SWAP_CRITICAL_MB", base.swap_critical_mb),
    }
}

/// Il numero che segue un'etichetta in una riga di `memory_pressure`.
fn field(text: &str, label: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.trim_start().starts_with(label))?;
    line[line.find(label)? + label.len()..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Il primo numero lungo di una riga, per la prima di `memory_pressure`:
/// «The system has 19327352832 (1179648 pages with a page size of 16384)».
fn first_long_number(line: &str) -> Option<u64> {
    line.split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() > 6)?
        .parse()
        .ok()
}

/// La misura vera. Una fonte che non risponde finisce in `unreadable`, non in
/// uno zero: `sysctl vm.swapusage`, `kern.memorystatus_vm_pressure_level`, `ps`
/// e `pgrep` sono negati dentro il sandbox di Claude Code, e un diniego preso
/// per «tutto bene» è il controllo che mente.
pub fn measure() -> judge::Reading {
    let mut r = judge::Reading::default();

    // `memory_pressure` dice in un colpo solo la RAM totale, la dimensione di
    // pagina, le pagine libere e quelle del compressore, e — a differenza di
    // `sysctl` — risponde anche dentro il sandbox. ~6 ms a chiamata, misurato.
    match Command::new("/usr/bin/memory_pressure").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let page = text
                .split("page size of")
                .nth(1)
                .and_then(|rest| {
                    rest.chars()
                        .skip_while(|c| !c.is_ascii_digit())
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(16384);
            r.total_mb = text
                .lines()
                .next()
                .and_then(first_long_number)
                .map(|bytes| bytes / 1_048_576);
            r.free_mb = field(&text, "Pages free:").map(|p| p * page / 1_048_576);
            r.compressor_mb =
                field(&text, "Pages used by compressor:").map(|p| p * page / 1_048_576);
            // LE PAGINE LIBERE NON SONO LA MEMORIA DISPONIBILE, e su macOS non
            // ci somigliano nemmeno: il kernel tiene la RAM piena di cache di
            // proposito e la libera quando qualcuno la chiede, quindi «free»
            // resta vicino a zero anche su una macchina scarica. Misurato il
            // 28/08/2026 alle 21:00: 88 MB liberi, **4.701 MB disponibili**, e
            // il kernel che alla domanda diretta rispondeva «pressione
            // normale». Su quel numero il freno negava ogni compilazione.
            //
            // Disponibile è quello che il kernel restituisce se glielo chiedi:
            // le pagine libere più le tre famiglie che sa recuperare senza
            // scrivere su disco.
            let recoverable = ["Pages free:", "Pages inactive:", "Pages speculative:", "Pages purgeable:"]
                .iter()
                .filter_map(|name| field(&text, name))
                .sum::<u64>();
            if recoverable > 0 {
                r.available_mb = Some(recoverable * page / 1_048_576);
            }
        }
        _ => r.unreadable.push("memory_pressure".into()),
    }
    if r.free_mb.is_none() && !r.unreadable.iter().any(|s| s == "memory_pressure") {
        r.unreadable.push("pagine libere".into());
    }

    // Lo swap allocato, contato sui file. Una cartella che non si legge è
    // cecità su quel segnale, non uno swap vuoto.
    match std::fs::read_dir(SWAP_DIR) {
        Ok(entries) => {
            let mut bytes = 0u64;
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("swapfile") {
                    if let Ok(m) = e.metadata() {
                        bytes += m.len();
                    }
                }
            }
            r.swap_allocated_mb = Some(bytes / 1_048_576);
        }
        Err(_) => r.unreadable.push(SWAP_DIR.into()),
    }

    r
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn num(v: Option<u64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "-".into())
}

fn parse_num(s: &str) -> Option<u64> {
    if s == "-" {
        return None;
    }
    s.parse().ok()
}

fn level_name(level: judge::Pressure) -> &'static str {
    match level {
        judge::Pressure::Calm => "calm",
        judge::Pressure::Tight => "tight",
        judge::Pressure::Critical => "critical",
        judge::Pressure::Unknown => "unknown",
    }
}

fn level_from_name(name: &str) -> judge::Pressure {
    match name {
        "calm" => judge::Pressure::Calm,
        "tight" => judge::Pressure::Tight,
        "critical" => judge::Pressure::Critical,
        _ => judge::Pressure::Unknown,
    }
}

/// La misura in cache, con la sua età.
fn cached() -> Option<(judge::Reading, judge::Pressure, u64)> {
    let raw = std::fs::read_to_string(state_dir().join(CACHE)).ok()?;
    let f: Vec<&str> = raw.trim_end_matches('\n').split('\t').collect();
    if f.len() < 7 {
        return None;
    }
    let stamp: u64 = f[0].parse().ok()?;
    let reading = judge::Reading {
        free_mb: parse_num(f[2]),
        // L'ottavo campo è arrivato il 28/08/2026. Una riga scritta prima non
        // ce l'ha, e non è un errore: vale `None`, cioè il ripiego sulle pagine
        // libere, finché la prossima misura non riscrive la riga intera.
        available_mb: f.get(7).copied().and_then(parse_num),
        compressor_mb: parse_num(f[3]),
        total_mb: parse_num(f[4]),
        swap_allocated_mb: parse_num(f[5]),
        unreadable: f[6]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    };
    Some((
        reading,
        level_from_name(f[1]),
        now_epoch().saturating_sub(stamp),
    ))
}

fn store(reading: &judge::Reading, level: judge::Pressure) {
    // La memoria disponibile va **in fondo**, non al suo posto logico: le righe
    // già scritte hanno sette campi e chi le rilegge conta le posizioni. Un
    // campo aggiunto in mezzo le farebbe leggere sbagliate senza un errore.
    let line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        now_epoch(),
        level_name(level),
        num(reading.free_mb),
        num(reading.compressor_mb),
        num(reading.total_mb),
        num(reading.swap_allocated_mb),
        reading.unreadable.join(","),
        num(reading.available_mb)
    );
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(CACHE), line);
}

/// La pressione di adesso, misurata o ripresa dalla cache. Il secondo valore
/// dice se è stata riletta: chi scrive un rapporto deve poter dichiarare se
/// sta guardando la macchina o un ricordo di un minuto fa.
pub fn pressure_now() -> (judge::PressureVerdict, bool) {
    let t = thresholds();
    let last = cached();
    let stale = judge::should_remeasure(last.as_ref().map(|(_, l, age)| (*l, *age)));
    if !stale {
        if let Some((reading, _, _)) = last {
            return (judge::classify(&reading, &t), false);
        }
    }
    let reading = measure();
    let verdict = judge::classify(&reading, &t);
    store(&reading, verdict.level);
    (verdict, true)
}

/// Il gancio. Fail-open ovunque: nel dubbio il comando passa.
pub fn run() -> i32 {
    if std::env::var("MEMORY_GOVERNOR").is_ok_and(|v| v == "off") {
        return 0;
    }
    let Some(input) = hook_io::read_input() else {
        return 0;
    };
    let command = input.bash_command();
    if command.trim().is_empty() {
        return 0;
    }
    // Il governo non frena sé stesso: un rapporto bloccato a macchina carica
    // sarebbe il controllo che si spegne proprio quando serve.
    if command.contains("memory-governor") {
        return 0;
    }

    let (pressure, _) = pressure_now();
    let decision = match judge::judge(&command, &pressure) {
        judge::Verdict::Pass => hook_io::Decision::Pass,
        judge::Verdict::Notice(m) => hook_io::Decision::Warn(m),
        judge::Verdict::Refuse(m) => hook_io::Decision::Deny(m),
    };
    hook_io::emit("memory-governor", &decision)
}

/// `claude-hooks memory-governor report`: la fotografia onesta, per chi la
/// guarda da fuori — Theo, o il guardiano periodico sotto `launchctl`.
///
/// Nomina i processi più grossi **senza proporre di chiuderli**. E se `ps` non
/// risponde lo scrive a lettere grosse invece di stampare un elenco vuoto: il
/// 24/08/2026 un `pgrep` negato dal sandbox ha risposto «nessuno» invece di
/// «non posso», e per venti minuti due banchi sono girati insieme sulla stessa
/// cartella di uscita sulla fede di quella conferma falsa.
pub fn report() -> i32 {
    let t = thresholds();
    let reading = measure();
    let pressure = judge::classify(&reading, &t);

    println!(
        "governo della memoria — {}",
        hook_io::journal::now_iso8601_seconds()
    );
    println!(
        "  pressione: {}{}",
        level_name(pressure.level),
        if pressure.because.is_empty() {
            String::new()
        } else {
            format!(" — {}", pressure.because.join(", "))
        }
    );
    println!(
        "  libere {} MB · compressore {} MB · RAM {} MB · swap allocato {} MB",
        num(reading.free_mb),
        num(reading.compressor_mb),
        num(reading.total_mb),
        num(reading.swap_allocated_mb)
    );
    if !reading.unreadable.is_empty() {
        println!(
            "  CIECO SU: {} — questi numeri sono parziali",
            reading.unreadable.join(", ")
        );
    }

    match top_processes(8) {
        Ok(top) if !top.is_empty() => {
            println!("  i più grossi:");
            for (name, mb) in &top {
                println!("    {mb:>6} MB  {name}");
            }
            if let Some(line) = judge::heavy_neighbours_notice(&pressure, &top) {
                println!("  {line}");
            }
        }
        Ok(_) => println!("  NON HO LETTO NESSUN PROCESSO: `ps` ha risposto, ma a vuoto."),
        Err(e) => println!(
            "  NON POSSO LEGGERE I PROCESSI ({e}). Non è «nessuno»: è cecità. \
             Dentro il sandbox di Claude Code `ps` e `pgrep` sono negati — \
             questo comando va lanciato da fuori."
        ),
    }
    // Il livello 2 della gerarchia: ciò che è nostro e dorme. Anche qui il
    // rapporto propone un pid e non lo chiude — e distingue chi non ha più
    // padrone da chi ne ha uno vivo, che è la distinzione che il 24/08 ha
    // salvato una sessione da undici ore.
    match our_processes() {
        Ok(procs) => {
            let found = judge::sleeping_work(&procs);
            if found.is_empty() {
                println!("  roba nostra che dorme: nessuna ({} processi nostri visti)", procs.len());
            } else {
                println!("  roba nostra che dorme:");
                for r in &found {
                    match r {
                        judge::Reclaim::OrphanedByUs(m) => println!("    [si può chiudere] {m}"),
                        judge::Reclaim::LoudButOwned(m) => println!("    [solo da dire]    {m}"),
                    }
                }
            }
        }
        Err(e) => println!("  NON POSSO CENSIRE I PROCESSI NOSTRI ({e}): cecità, non «nessuno»."),
    }

    // La riga per accendere il freno viaggia col rapporto: chi guarda i numeri
    // è chi deve decidere se metterla, e cercarla altrove è un passaggio in più
    // che nessuno fa.
    println!("  per accendere il freno, in `settings.json` sotto `PreToolUse`/`Bash`:");
    println!("    {SETTINGS_LINE}");
    // Esce 0 anche a macchina in ginocchio: è un rapporto, non un collaudo, e
    // chi lo chiama non deve confondere «carica» con «rotto».
    0
}

/// I processi più grossi per memoria residente, sommati per nome — Arc gira su
/// venticinque processi, e un elenco che li tratta uno per uno non dice mai che
/// il browser sta a 3,5 GB.
fn top_processes(limit: usize) -> Result<Vec<(String, u64)>, String> {
    let out = Command::new("/bin/ps")
        .args(["-Ao", "rss=,comm="])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut totals: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for line in text.lines() {
        let mut parts = line.trim().splitn(2, char::is_whitespace);
        let Some(rss) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Some(path) = parts.next().map(str::trim) else {
            continue;
        };
        *totals.entry(app_name(path)).or_insert(0) += rss / 1024;
    }
    let mut v: Vec<(String, u64)> = totals.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(limit);
    Ok(v)
}

/// Ciò che riconosciamo come nostro fra i processi della macchina: i server
/// che una sessione avvia e che nessuno spegne. Volutamente stretto — un
/// `node` qualunque può essere il server di sviluppo di Theo, e proporre di
/// chiuderlo sarebbe il guasto peggiore di quello che si cura.
const OURS: &[&str] = &["socraticode", "harness-mem", "mcp-server", "claude-hooks"];

/// L'età di `ps`, nel formato `[[gg-]hh:]mm:ss`. `etimes` (secondi netti) è una
/// colonna GNU: su macOS non esiste, e `ps` risponde stampando l'elenco dei
/// formati validi — un confronto numerico contro quella stringa fallisce senza
/// dire niente. Stessa trappola già pagata in `guardiano-macchina.sh`.
fn parse_etime(s: &str) -> u64 {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().unwrap_or(0), r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest.split(':').map(|p| p.parse().unwrap_or(0)).collect();
    let hms = match parts.len() {
        3 => parts[0] * 3600 + parts[1] * 60 + parts[2],
        2 => parts[0] * 60 + parts[1],
        _ => 0,
    };
    days * 86400 + hms
}

/// I processi nostri, con la risposta a «qualcuno lo rivendica ancora?».
///
/// La catena si risale fino a trovare un `claude` vivo, e non ci si ferma al
/// genitore: misurato il 24/08/2026, il server che bruciava un core da 11h39
/// aveva `npm exec` come genitore e il `claude` vivo **due gradini più su**.
fn our_processes() -> Result<Vec<judge::OurProcess>, String> {
    let out = Command::new("/bin/ps")
        .args(["-Ao", "pid=,ppid=,rss=,%cpu=,etime=,args="])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // Prima passata: la mappa dei genitori e chi è un `claude`. Serve intera
    // prima di giudicare chiunque, perché il padrone può stare più in basso
    // nell'elenco di chi lo cerca.
    let mut parent: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut is_claude: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let (Ok(pid), Ok(ppid)) = (f[0].parse::<u32>(), f[1].parse::<u32>()) else {
            continue;
        };
        parent.insert(pid, ppid);
        let args = f[5..].join(" ");
        // `claude` nudo, non un comando che lo nomina: `grep claude` sarebbe
        // il caso già pagato del 21/08, dove cercare un identificativo fra i
        // processi trovava il proprio comando di ricerca.
        if app_name(f[5]) == "claude" {
            is_claude.insert(pid);
        }
        rows.push((pid, f[2], f[3], f[4], args));
    }

    let owner_alive = |start: u32| {
        let mut p = start;
        // Tetto ai passi: una catena di genitori corrotta non deve girare
        // all'infinito dentro un rapporto.
        for _ in 0..12 {
            let Some(&up) = parent.get(&p) else { return false };
            if is_claude.contains(&up) {
                return true;
            }
            if up <= 1 {
                return false;
            }
            p = up;
        }
        false
    };

    let mut procs = Vec::new();
    for (pid, rss, cpu, etime, args) in rows {
        if !OURS.iter().any(|o| args.contains(o)) {
            continue;
        }
        procs.push(judge::OurProcess {
            pid,
            name: app_name(args.split_whitespace().next().unwrap_or("?")).to_string(),
            rss_mb: rss.parse::<u64>().unwrap_or(0) / 1024,
            cpu_pct: cpu.parse::<f64>().unwrap_or(0.0) as u64,
            age_secs: parse_etime(etime),
            owner_alive: owner_alive(pid),
        });
    }
    Ok(procs)
}

/// Il nome dell'applicazione, non quello dell'eseguibile: dentro
/// `Arc.app/Contents/…` vivono `Arc Helper (Renderer)` e altri quattro nomi,
/// e sommarli sotto nomi diversi è il modo di non vedere mai i 3,5 GB.
fn app_name(path: &str) -> String {
    if let Some(before) = path.split(".app/").next() {
        if before.len() < path.len() {
            return before.rsplit('/').next().unwrap_or(before).to_string();
        }
    }
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_gives_up_its_number() {
        let text = "Stats: \nPages free: 4076 \nPages purgeable: 535 \n";
        assert_eq!(field(text, "Pages free:"), Some(4076));
        assert_eq!(field(text, "Pages wired down:"), None);
    }

    #[test]
    fn the_first_line_gives_up_the_machine_size() {
        let line = "The system has 19327352832 (1179648 pages with a page size of 16384).";
        assert_eq!(first_long_number(line), Some(19327352832));
    }

    #[test]
    fn a_dash_is_not_measured_and_not_zero() {
        assert_eq!(parse_num("-"), None);
        assert_eq!(parse_num("0"), Some(0));
        assert_eq!(parse_num("4096"), Some(4096));
        // Un campo illeggibile non diventa zero per sbaglio.
        assert_eq!(parse_num("boh"), None);
    }

    #[test]
    fn helper_processes_are_summed_under_the_application() {
        assert_eq!(
            app_name("/Applications/Arc.app/Contents/Frameworks/Arc Helper (Renderer)"),
            "Arc"
        );
        assert_eq!(app_name("/opt/homebrew/bin/node"), "node");
        assert_eq!(app_name("claude"), "claude");
    }

    #[test]
    fn the_age_is_read_in_the_format_macos_actually_prints() {
        assert_eq!(parse_etime("11:39:43"), 41983);
        assert_eq!(parse_etime("32:35"), 1955);
        assert_eq!(parse_etime("02-11:15:26"), 213326);
        // `ps` che risponde con l'elenco dei formati invece di un'età non
        // diventa un numero per sbaglio.
        assert_eq!(parse_etime("etimes: keyword not found"), 0);
    }

    #[test]
    fn the_real_machine_answers_something() {
        // Non si asserisce un numero: cambia a ogni istante. Si asserisce che
        // la fonte scelta risponda davvero su questa macchina — se un giorno
        // `memory_pressure` sparisse, questa prova lo direbbe invece di
        // lasciare il governo cieco in silenzio.
        let r = measure();
        assert!(
            r.free_mb.is_some() || !r.unreadable.is_empty(),
            "misura muta: né un numero né una fonte cieca dichiarata"
        );
        assert!(
            r.swap_allocated_mb.is_some(),
            "i file di swap sono elencabili anche dentro il sandbox: {:?}",
            r.unreadable
        );
    }
}
