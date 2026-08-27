//! `guasti` — rompe il codice appena scritto, un pezzo alla volta, e riferisce
//! quali rotture nessuna prova ha notato.
//!
//! Uso tipico, dal deposito che ha appena ricevuto la lavorazione:
//!
//!     guasti --repo ~/personal/socraticode --base origin/main \
//!            --test 'npx vitest run tests/unit/graph-resolution.test.ts' \
//!            --build 'npx tsc --noEmit'
//!
//! QUESTO FILE FA SOLO I/O E PROCESSI: chiama `git`, copia alberi, lancia la
//! batteria, ripristina. Ogni giudizio — quali guasti esistono, cosa vuol dire
//! un esito, come si conta — sta in `lib.rs` e nei suoi moduli, dove le prove
//! lo controllano senza toccare né disco né rete.
//!
//! LA COPIA. Si guasta sempre una copia usa-e-getta, mai l'albero vero: una
//! batteria interrotta a metà lascerebbe il deposito guastato, e il 26/08/2026
//! un lavoro dentro un albero vivo ha già fermato un servizio per 72 minuti.
//! Le cartelle pesanti (`node_modules`) si collegano invece di copiarle.

use guasti::diff::{is_test_path, parse_unified_diff};
use guasti::mutations::{faults_for_file, DEFAULT_PER_LINE_CAP};
use guasti::report::{exit_code, render, tally, Outcome};
use guasti::{apply, classify, Fault, Verdict};
use std::collections::VecDeque;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const USAGE: &str = "\
guasti — i guasti li sceglie il diff, non chi ha scritto il codice.

  guasti --test '<comando>' [opzioni]

Da dove vengono i guasti (una sola forma, la prima che c'è):
  --range <A..B>     il codice cambiato fra due revisioni
  --base <riferimento>   l'albero contro il ramo d'integrazione (A...HEAD)
  --worktree         le modifiche non ancora committate (predefinito)

Cosa si esegue:
  --repo <cartella>  il deposito da guastare (predefinito: la cartella corrente)
  --test '<comando>' la batteria; rosso = guasto ucciso. Obbligatorio
  --build '<comando>' il compilatore; rosso = guasto non vitale, non contato
  --timeout <secondi> il tetto per ogni esecuzione (predefinito 900)

Quanto grande:
  --jobs <n>         quante copie in parallelo (predefinito 1)
  --max <n>          quanti guasti al massimo
  --per-line <n>     quanti guasti al massimo da una riga sola (predefinito 4)
  --only <pezzo>     solo i file il cui percorso contiene questo testo
  --lines <a,b,c>    solo i guasti su queste righe (per riprovare i
                     sopravvissuti con una batteria più larga)
  --tests-too        guasta anche i file di prova (predefinito: no)

Dove:
  --work <cartella>  dove tenere le copie (predefinito: sotto $TMPDIR)
  --link <nome>      cartella da collegare invece di copiare (ripetibile;
                     predefinito: node_modules)
  --skip <nome>      cartella da non copiare (ripetibile; predefinito:
                     .git, target, dist, coverage, .next, .venv, .socraticode)
  --keep             non cancellare le copie alla fine

Solo guardare:
  --list             elenca i guasti che il diff propone e non esegue niente

Uscita: 0 nessun sopravvissuto, 1 almeno un sopravvissuto, 2 il giro non ha
misurato niente (base rossa, nessun guasto, nessun file toccato).
";

struct Options {
    repo: PathBuf,
    range: Option<String>,
    base: Option<String>,
    test: Option<String>,
    build: Option<String>,
    timeout: u64,
    jobs: usize,
    max: Option<usize>,
    per_line: usize,
    only: Option<String>,
    lines: Option<Vec<usize>>,
    tests_too: bool,
    work: Option<PathBuf>,
    link: Vec<String>,
    skip: Vec<String>,
    keep: bool,
    list: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            repo: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            range: None,
            base: None,
            test: None,
            build: None,
            timeout: 900,
            jobs: 1,
            max: None,
            per_line: DEFAULT_PER_LINE_CAP,
            only: None,
            lines: None,
            tests_too: false,
            work: None,
            link: vec!["node_modules".to_string()],
            skip: [
                ".git",
                "target",
                "dist",
                "coverage",
                ".next",
                ".venv",
                ".socraticode",
            ]
            .iter()
            .map(|name| name.to_string())
            .collect(),
            keep: false,
            list: false,
        }
    }
}

fn main() {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return;
        }
        Err(message) => {
            eprintln!("guasti: {message}\n");
            print!("{USAGE}");
            std::process::exit(2);
        }
    };
    match run(options) {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("guasti: {message}");
            std::process::exit(2);
        }
    }
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = args.peekable();
    let mut link_seen = false;
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{argument} vuole un valore"))
        };
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--repo" => options.repo = PathBuf::from(value()?),
            "--range" => options.range = Some(value()?),
            "--base" => options.base = Some(value()?),
            "--test" => options.test = Some(value()?),
            "--build" => options.build = Some(value()?),
            "--timeout" => {
                options.timeout = value()?.parse().map_err(|_| "--timeout vuole un numero")?
            }
            "--jobs" => {
                options.jobs = value()?
                    .parse::<usize>()
                    .map_err(|_| "--jobs vuole un numero")?
                    .max(1)
            }
            "--max" => {
                options.max = Some(value()?.parse().map_err(|_| "--max vuole un numero")?)
            }
            "--per-line" => {
                options.per_line = value()?
                    .parse::<usize>()
                    .map_err(|_| "--per-line vuole un numero")?
                    .max(1)
            }
            "--only" => options.only = Some(value()?),
            "--lines" => {
                let raw = value()?;
                let mut lines = Vec::new();
                for part in raw.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    lines.push(
                        part.parse::<usize>()
                            .map_err(|_| format!("--lines: «{part}» non è un numero di riga"))?,
                    );
                }
                options.lines = Some(lines);
            }
            "--tests-too" => options.tests_too = true,
            "--work" => options.work = Some(PathBuf::from(value()?)),
            "--link" => {
                if !link_seen {
                    options.link.clear();
                    link_seen = true;
                }
                options.link.push(value()?);
            }
            "--skip" => options.skip.push(value()?),
            "--keep" => options.keep = true,
            "--worktree" => {
                options.range = None;
                options.base = None;
            }
            "--list" => options.list = true,
            other => return Err(format!("opzione sconosciuta: {other}")),
        }
    }
    if options.test.is_none() && !options.list {
        return Err("serve --test '<comando>': senza batteria non si misura niente".to_string());
    }
    Ok(Some(options))
}

fn run(options: Options) -> Result<i32, String> {
    let repo = options
        .repo
        .canonicalize()
        .map_err(|error| format!("{}: {error}", options.repo.display()))?;

    // 1. Il perimetro: le righe che il diff dichiara nuove.
    let diff_text = git_diff(&repo, &options)?;
    let touched = parse_unified_diff(&diff_text);
    if touched.is_empty() {
        println!("Nessuna riga toccata: niente da guastare.");
        return Ok(2);
    }

    // 2. I guasti, ricavati da quelle righe e da nessun elenco.
    let mut faults: Vec<Fault> = Vec::new();
    for file in &touched {
        if !options.tests_too && is_test_path(&file.path) {
            continue;
        }
        if let Some(only) = &options.only {
            if !file.path.contains(only.as_str()) {
                continue;
            }
        }
        let full = repo.join(&file.path);
        let Ok(source) = fs::read_to_string(&full) else {
            continue;
        };
        faults.extend(faults_for_file(
            &file.path,
            &source,
            &file.lines,
            options.per_line,
        ));
    }
    // Le righe dichiarate: serve a riprovare con una batteria più larga i
    // guasti sopravvissuti a una più stretta, senza rifare tutto il giro.
    if let Some(lines) = &options.lines {
        faults.retain(|fault| lines.contains(&fault.line));
    }
    if let Some(max) = options.max {
        faults.truncate(max);
    }
    println!(
        "{} file toccati, {} guasti proposti dal codice modificato.",
        touched.len(),
        faults.len()
    );
    if options.list {
        for fault in &faults {
            println!("  {}", fault.name());
        }
        return Ok(if faults.is_empty() { 2 } else { 0 });
    }
    if faults.is_empty() {
        println!("Nessun guasto proposto: il giro non misura niente.");
        return Ok(2);
    }

    let test = options.test.clone().expect("--test è obbligatorio");

    // 3. Le copie usa-e-getta. Il deposito vero non si tocca mai.
    let work = match &options.work {
        Some(path) => path.clone(),
        None => std::env::temp_dir().join(format!("guasti-{}", std::process::id())),
    };
    fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    if work.starts_with(&repo) {
        return Err(format!(
            "la cartella di lavoro {} sta dentro il deposito: la copia copierebbe se stessa",
            work.display()
        ));
    }
    let jobs = options.jobs.min(faults.len());
    let mut copies = Vec::new();
    for index in 0..jobs {
        let copy = work.join(format!("copia-{index}"));
        if copy.exists() {
            fs::remove_dir_all(&copy).ok();
        }
        eprintln!("copio l'albero in {} …", copy.display());
        copy_tree(&repo, &copy, &options.skip, &options.link)?;
        copies.push(copy);
    }

    // 4. La base verde. Una batteria già rossa non può arrossire di più: ogni
    //    guasto risulterebbe ucciso senza che nessuno l'abbia notato.
    eprintln!("provo la base senza guasti …");
    let (base_ok, base_output) = run_command(&test, &copies[0], options.timeout);
    if !base_ok {
        eprintln!("{}", tail(&base_output, 30));
        cleanup(&copies, options.keep);
        return Err("la base non è verde: questo giro non misurerebbe niente".to_string());
    }
    if let Some(build) = &options.build {
        let (build_ok, build_output) = run_command(build, &copies[0], options.timeout);
        if !build_ok {
            eprintln!("{}", tail(&build_output, 30));
            cleanup(&copies, options.keep);
            return Err("la base non compila: il verdetto «non vitale» sarebbe falso".to_string());
        }
    }

    // 5. Un guasto alla volta per copia, in parallelo fra copie.
    let queue = Arc::new(Mutex::new(faults.into_iter().collect::<VecDeque<Fault>>()));
    let outcomes: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
    let total = queue.lock().unwrap().len();
    let done = Arc::new(Mutex::new(0usize));
    std::thread::scope(|scope| {
        for copy in &copies {
            let queue = Arc::clone(&queue);
            let outcomes = Arc::clone(&outcomes);
            let done = Arc::clone(&done);
            let test = test.clone();
            let build = options.build.clone();
            let timeout = options.timeout;
            let repo = repo.clone();
            scope.spawn(move || loop {
                let Some(fault) = queue.lock().unwrap().pop_front() else {
                    return;
                };
                let outcome = run_one_fault(&repo, copy, &fault, &test, build.as_deref(), timeout);
                {
                    let mut counter = done.lock().unwrap();
                    *counter += 1;
                    eprintln!(
                        "[{}/{total}] {} — {}",
                        counter,
                        fault.name(),
                        outcome.verdict.label()
                    );
                }
                outcomes.lock().unwrap().push(outcome);
            });
        }
    });

    let mut outcomes = Arc::try_unwrap(outcomes)
        .map_err(|_| "un lavoro è rimasto appeso".to_string())?
        .into_inner()
        .map_err(|error| error.to_string())?;
    outcomes.sort_by(|a, b| {
        (&a.fault.file, a.fault.line, a.fault.offset).cmp(&(&b.fault.file, b.fault.line, b.fault.offset))
    });

    cleanup(&copies, options.keep);
    println!();
    print!("{}", render(&outcomes));
    Ok(exit_code(tally(&outcomes)))
}

/// Un guasto: si scrive, si prova, si rimette a posto sempre — anche quando la
/// batteria muore a metà.
fn run_one_fault(
    repo: &Path,
    copy: &Path,
    fault: &Fault,
    test: &str,
    build: Option<&str>,
    timeout: u64,
) -> Outcome {
    let started = Instant::now();
    let target = copy.join(&fault.file);
    let original = match fs::read_to_string(repo.join(&fault.file)) {
        Ok(text) => text,
        Err(_) => {
            return Outcome {
                fault: fault.clone(),
                verdict: Verdict::NotApplied,
                seconds: 0,
            }
        }
    };
    let Some(mutated) = apply(&original, fault) else {
        return Outcome {
            fault: fault.clone(),
            verdict: Verdict::NotApplied,
            seconds: 0,
        };
    };
    if fs::write(&target, &mutated).is_err() {
        return Outcome {
            fault: fault.clone(),
            verdict: Verdict::NotApplied,
            seconds: 0,
        };
    }

    let build_ok = build.map(|command| run_command(command, copy, timeout).0);
    let test_ok = if build_ok == Some(false) {
        // Non compila: la batteria non direbbe niente e costerebbe un giro.
        false
    } else {
        run_command(test, copy, timeout).0
    };

    // Il ripristino non è opzionale: la copia serve al guasto successivo.
    let _ = fs::write(&target, &original);

    Outcome {
        fault: fault.clone(),
        verdict: classify(true, build_ok, test_ok),
        seconds: started.elapsed().as_secs(),
    }
}

/// Il diff, nella forma senza contorno: ogni riga `+` è una riga da guastare.
fn git_diff(repo: &Path, options: &Options) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo).arg("diff").arg("-U0");
    match (&options.range, &options.base) {
        (Some(range), _) => {
            command.arg(range);
        }
        (None, Some(base)) => {
            command.arg(format!("{base}...HEAD"));
        }
        (None, None) => {
            command.arg("HEAD");
        }
    }
    let output = command
        .output()
        .map_err(|error| format!("git diff non parte: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff è uscito male: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Esegue un comando nella copia. Vero se è uscito zero.
///
/// Il tetto di tempo esiste perché un guasto può fare girare a vuoto un ciclo:
/// senza, il giro si ferma lì per sempre. Alla scadenza si uccide il gruppo di
/// processi, non solo la shell — i figli le sopravvivrebbero.
fn run_command(command: &str, directory: &Path, timeout: u64) -> (bool, String) {
    let mut child = match Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return (false, format!("il comando non parte: {error}")),
    };
    let pid = child.id();
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = Command::new("/bin/kill")
                        .arg("-TERM")
                        .arg(format!("-{pid}"))
                        .status();
                    std::thread::sleep(Duration::from_secs(2));
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, format!("scaduto dopo {timeout}s"));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return (false, format!("attesa fallita: {error}")),
        }
    }
    match child.wait_with_output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.success(), text)
        }
        Err(error) => (false, format!("uscita illeggibile: {error}")),
    }
}

/// Copia l'albero, saltando le cartelle dichiarate e collegando le pesanti.
fn copy_tree(
    source: &Path,
    destination: &Path,
    skip: &[String],
    link: &[String],
) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| format!("{}: {error}", destination.display()))?;
    let entries =
        fs::read_dir(source).map_err(|error| format!("{}: {error}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let from = entry.path();
        let to = destination.join(&name);
        if skip.contains(&name) {
            continue;
        }
        if link.contains(&name) {
            // Collegata, non copiata: `node_modules` da solo pesa più di tutto
            // il resto messo insieme, e nessun guasto lo tocca.
            std::os::unix::fs::symlink(&from, &to)
                .map_err(|error| format!("{}: {error}", to.display()))?;
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", from.display()))?;
        if kind.is_symlink() {
            if let Ok(target) = fs::read_link(&from) {
                let _ = std::os::unix::fs::symlink(target, &to);
            }
        } else if kind.is_dir() {
            copy_tree(&from, &to, skip, link)?;
        } else {
            fs::copy(&from, &to).map_err(|error| format!("{}: {error}", from.display()))?;
        }
    }
    Ok(())
}

fn cleanup(copies: &[PathBuf], keep: bool) {
    if keep {
        for copy in copies {
            eprintln!("copia tenuta: {}", copy.display());
        }
        return;
    }
    for copy in copies {
        let _ = fs::remove_dir_all(copy);
    }
}

/// Le ultime righe di un'uscita, per dire cosa è andato storto senza
/// riversare a schermo una batteria intera.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_read_a_range_and_a_command() {
        let args = ["--repo", "/tmp/x", "--range", "a..b", "--test", "npm t"]
            .iter()
            .map(|part| part.to_string());
        let options = parse_options(args).unwrap().unwrap();
        assert_eq!(options.repo, PathBuf::from("/tmp/x"));
        assert_eq!(options.range.as_deref(), Some("a..b"));
        assert_eq!(options.test.as_deref(), Some("npm t"));
        assert_eq!(options.jobs, 1);
    }

    /// Senza batteria non si misura niente, e un giro che non misura non deve
    /// nemmeno partire.
    #[test]
    fn a_run_without_a_test_command_is_refused() {
        let args = ["--repo", "/tmp/x"].iter().map(|part| part.to_string());
        assert!(parse_options(args).is_err());
    }

    #[test]
    fn listing_does_not_need_a_test_command() {
        let args = ["--list"].iter().map(|part| part.to_string());
        assert!(parse_options(args).unwrap().unwrap().list);
    }

    /// Il primo `--link` scritto a mano sostituisce il valore predefinito
    /// invece di aggiungersi: chi dichiara le sue cartelle non se ne ritrova
    /// una che non ha chiesto.
    #[test]
    fn a_declared_link_replaces_the_default_one() {
        let args = ["--list", "--link", "vendor", "--link", "cache"]
            .iter()
            .map(|part| part.to_string());
        let options = parse_options(args).unwrap().unwrap();
        assert_eq!(options.link, vec!["vendor", "cache"]);
    }

    #[test]
    fn the_declared_lines_are_read_as_a_list() {
        let args = ["--list", "--lines", "12, 34,56"]
            .iter()
            .map(|part| part.to_string());
        let options = parse_options(args).unwrap().unwrap();
        assert_eq!(options.lines, Some(vec![12, 34, 56]));
    }

    /// Un numero di riga scritto male non deve restringere in silenzio: chi
    /// legge il rapporto crederebbe di aver riprovato guasti che nessuno ha
    /// eseguito.
    #[test]
    fn a_malformed_line_number_is_refused() {
        let args = ["--list", "--lines", "12,dodici"]
            .iter()
            .map(|part| part.to_string());
        assert!(parse_options(args).is_err());
    }

    #[test]
    fn an_unknown_option_is_refused_instead_of_ignored() {
        let args = ["--test", "x", "--turbo"].iter().map(|part| part.to_string());
        assert!(parse_options(args).is_err());
    }

    #[test]
    fn a_command_that_fails_is_not_green() {
        let (ok, _) = run_command("exit 3", Path::new("/tmp"), 30);
        assert!(!ok);
        let (ok, output) = run_command("echo ciao", Path::new("/tmp"), 30);
        assert!(ok);
        assert!(output.contains("ciao"));
    }

    /// Il tetto di tempo deve mordere: senza, un guasto che fa girare a vuoto
    /// un ciclo ferma il giro per sempre.
    #[test]
    fn a_command_that_never_ends_is_cut_off() {
        let started = Instant::now();
        let (ok, output) = run_command("sleep 60", Path::new("/tmp"), 1);
        assert!(!ok);
        assert!(output.contains("scaduto"), "{output}");
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn the_tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(tail("a", 5), "a");
    }
}
