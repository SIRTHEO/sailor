//! `sailor release`: mette in servizio un binario costruito da `HEAD`, mai
//! dall'albero di lavoro.
//!
//! Questo modulo contiene soltanto i gesti che toccano disco e processi. Le
//! decisioni — quali bersagli esistono, come si legge un timbro e quando un
//! servizio è occupato — stanno nella libreria `release`, dove si possono
//! provare senza cambiare il mondo. Prima del 27/08/2026 questo era il
//! `main.rs` di un binario a sé (`release`): confluito qui perché nessuno
//! aveva mai deciso più di un binario di sistema, e il crate `release` che
//! resta serve solo più come libreria.

use release::{read_stamp, readiness, target, target_names, Service, Target};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Options {
    target_name: String,
    dry_run: bool,
    skip_tests: bool,
    wait_secs: u64,
}

struct TemporaryTree {
    path: PathBuf,
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("sailor release: {message}");
            eprintln!("bersagli disponibili: {}", target_names());
            return 2;
        }
    };
    let Some(selected) = target(&options.target_name) else {
        eprintln!(
            "sailor release: bersaglio sconosciuto '{}'; bersagli disponibili: {}",
            options.target_name,
            target_names()
        );
        return 2;
    };

    match release(selected, &options) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("sailor release: {message}");
            1
        }
    }
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut args = args.iter().cloned();
    let target_name = args.next().ok_or_else(|| {
        "manca il bersaglio (uso: sailor release <bersaglio> [--dry-run] [--skip-tests] [--wait-secs N])"
            .to_string()
    })?;
    if target_name.starts_with('-') {
        return Err(format!("manca il bersaglio prima di '{target_name}'"));
    }

    let mut dry_run = false;
    let mut skip_tests = false;
    let mut wait_secs = 600;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--skip-tests" => skip_tests = true,
            "--wait-secs" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--wait-secs richiede un numero".to_string())?;
                wait_secs = value
                    .parse::<u64>()
                    .map_err(|_| format!("valore non valido per --wait-secs: '{value}'"))?;
            }
            _ => return Err(format!("opzione sconosciuta '{arg}'")),
        }
    }
    Ok(Options {
        target_name,
        dry_run,
        skip_tests,
        wait_secs,
    })
}

fn release(selected: &Target, options: &Options) -> Result<i32, String> {
    let root = claude_root()?;
    let head_rev = git_text(&root, &["rev-parse", "HEAD"])?;
    let head_short = git_text(&root, &["rev-parse", "--short", "HEAD"])?;
    let source_rev = {
        let revision = git_text(&root, &["log", "-1", "--format=%H", "--", "rust/"])?;
        if revision.is_empty() {
            head_rev.clone()
        } else {
            revision
        }
    };

    let dirty = git_output(&root, &["status", "--porcelain", "--", "rust"])?;
    let dirty_count = String::from_utf8_lossy(&dirty.stdout).lines().count();
    if dirty_count > 0 {
        println!(
            "nota: {dirty_count} file non committati sotto rust/ restano fuori dal servizio, per costruzione"
        );
    }

    let temporary = make_temporary_tree()?;
    let repository = temporary.path.join("repo");
    println!("== clono HEAD ({head_short}) in un albero usa-e-getta ==");
    clone_repository(&root, &repository)?;
    command_success(
        Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "--quiet"])
            .arg(&head_rev),
        "il checkout di HEAD nel clone è fallito",
    )?;

    let build_target = root.join("rust/target/from-head");
    let cloned_rust = repository.join("rust");
    println!("== compilo da quell'albero ==");
    let build = Command::new("cargo")
        .current_dir(&cloned_rust)
        .env("CARGO_TARGET_DIR", &build_target)
        .args(["build", "--release", "--bin", selected.bin])
        .output()
        .map_err(|error| format!("non posso avviare cargo: {error}"))?;
    print_tail(&combined_output(&build), 5);
    if !build.status.success() {
        return Err(
            "HEAD non compila: nulla è stato sostituito e il binario in servizio è intatto"
                .to_string(),
        );
    }

    let fresh = build_target.join("release").join(selected.bin);
    if !fresh.is_file() {
        return Err(format!(
            "la compilazione è riuscita ma non ha prodotto {}",
            fresh.display()
        ));
    }

    if options.skip_tests {
        println!("== PROVE SALTATE (--skip-tests): questo rilascio NON è stato provato ==");
    } else {
        println!("== lancio l'intera batteria su HEAD ==");
        let suite_path = temporary.path.join("suite.txt");
        let suite_file = File::create(&suite_path)
            .map_err(|error| format!("non posso creare {}: {error}", suite_path.display()))?;
        let suite_error = suite_file
            .try_clone()
            .map_err(|error| format!("non posso duplicare il file della batteria: {error}"))?;
        let suite_status = Command::new("cargo")
            .current_dir(&cloned_rust)
            .env("CARGO_TARGET_DIR", &build_target)
            .args(["test", "--release", "--", "--nocapture"])
            .stdout(Stdio::from(suite_file))
            .stderr(Stdio::from(suite_error))
            .status()
            .map_err(|error| format!("non posso avviare la batteria: {error}"))?;
        let suite = fs::read(&suite_path)
            .map_err(|error| format!("non posso rileggere {}: {error}", suite_path.display()))?;
        print_tail(&suite, 25);
        if !suite_status.success() {
            eprintln!("sailor release: la batteria è rossa su HEAD: nulla è stato sostituito e il binario in servizio è intatto.");
            eprintln!("   Commit verdi separati non fanno una somma verde: questo è quel caso.");
            eprintln!("   Se il binario in servizio è rotto e aspettare costa di più, rilancia con --skip-tests.");
            return Ok(1);
        }
        let not_run = String::from_utf8_lossy(&suite)
            .matches("PROVA NON ESEGUITA")
            .count();
        if not_run > 0 {
            println!("== {not_run} prove NON ESEGUITE (non fallite) ==");
            println!(
                "   Verde qui non vuol dire provato là: il perimetro ha negato ciò che chiedevano."
            );
        }
    }

    let live = root.join(selected.live_rel);
    let stamp = root.join(selected.stamp_rel);
    if live.is_file() && files_equal(&fresh, &live)? {
        println!(
            "== niente da fare: il binario in servizio corrisponde già a HEAD ({head_short}) =="
        );
        if !options.dry_run {
            write_stamp(&stamp, &source_rev, &head_short);
        }
        return Ok(0);
    }

    print_changes(&root, &stamp, &head_rev, &head_short)?;
    if options.dry_run {
        println!("== prova a secco: il binario in servizio NON è stato sostituito ==");
        return Ok(0);
    }

    if let Some(service) = selected.service {
        let ready = wait_until_ready(&root, service, options.wait_secs)?;
        if !ready {
            println!("sailor release: rilascio rimandato: il servizio sta ancora lavorando; nulla è stato sostituito");
            return Ok(3);
        }
    }

    println!("== sostituisco il binario in servizio ==");
    atomic_copy(&fresh, &live)?;
    println!("   in servizio: {head_short}");

    let safe = root.join(selected.safe_rel);
    match atomic_copy(&fresh, &safe) {
        Ok(()) => println!("   anche fuori da target/: {}", safe.display()),
        Err(error) => {
            eprintln!("== AVVISO: la copia di sicurezza NON si è potuta scrivere ==");
            eprintln!("   {error}");
            eprintln!(
                "   Il binario in servizio è quello nuovo, ma la prossima compilazione di chiunque può ancora riscriverlo."
            );
        }
    }

    write_stamp(&stamp, &source_rev, &head_short);

    if let Some(service) = selected.service {
        // Il servizio gira ogni 90 secondi: fra la prima verifica e questo
        // punto può aver preso un altro compito, che il riavvio troncherebbe.
        if !wait_until_ready(&root, service, 0)? {
            let domain = service_domain(service);
            println!("sailor release: rilascio rimandato: il servizio ha iniziato una nuova lavorazione; il binario nuovo e il timbro sono al loro posto, ma il servizio esegue ancora quello vecchio.");
            println!("   Quando la lavorazione finisce, chiudi il rilascio con: launchctl kickstart -k {domain}");
            return Ok(3);
        }
        restart_service(service);
    }
    Ok(0)
}

fn claude_root() -> Result<PathBuf, String> {
    if let Some(root) = env::var_os("CLAUDE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".claude"))
        .ok_or_else(|| "né CLAUDE_HOME né HOME sono impostate".to_string())
}

fn git_output(root: &Path, args: &[&str]) -> Result<Output, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("non posso avviare git: {error}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git {} è fallito: {}",
            args.join(" "),
            detail.trim()
        ))
    }
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_output(root, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_success(command: &mut Command, context: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{context}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{context} ({})", status_description(status)))
    }
}

fn clone_repository(root: &Path, repository: &Path) -> Result<(), String> {
    let first = Command::new("git")
        .args(["clone", "--local", "--quiet"])
        .arg(root)
        .arg(repository)
        .output()
        .map_err(|error| format!("non posso avviare git clone: {error}"))?;
    if first.status.success() {
        return Ok(());
    }

    // Il sandbox del 27/08/2026 nega gli hardlink agli oggetti di `.git`: il
    // clone resta locale e resta un repository vero, ma deve copiare gli
    // oggetti invece di collegarli. Fuori dal sandbox la strada corta sopra
    // continua a costare praticamente zero spazio.
    if repository.exists() {
        fs::remove_dir_all(repository).map_err(|error| {
            format!(
                "il clone locale è fallito e non posso togliere il clone parziale {}: {error}",
                repository.display()
            )
        })?;
    }
    println!("nota: gli hardlink del clone locale sono negati; copio gli oggetti git");
    let second = Command::new("git")
        .args(["clone", "--local", "--no-hardlinks", "--quiet"])
        .arg(root)
        .arg(repository)
        .output()
        .map_err(|error| format!("non posso ripetere git clone senza hardlink: {error}"))?;
    if second.status.success() {
        Ok(())
    } else {
        let first_detail = String::from_utf8_lossy(&first.stderr);
        let second_detail = String::from_utf8_lossy(&second.stderr);
        Err(format!(
            "il clone locale di HEAD è fallito (prima: {}; senza hardlink: {})",
            first_detail.trim(),
            second_detail.trim()
        ))
    }
}

fn status_description(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("uscita {code}"))
        .unwrap_or_else(|| "terminato da un segnale".to_string())
}

fn make_temporary_tree() -> Result<TemporaryTree, String> {
    let parent = env::var_os("TMPDIR")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("/tmp"));
    let template = PathBuf::from(parent).join("release.XXXXXX");
    let output = Command::new("mktemp")
        .arg("-d")
        .arg(&template)
        .output()
        .map_err(|error| format!("non posso avviare mktemp: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "mktemp non ha creato la cartella temporanea: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let path = PathBuf::from(OsStr::new(String::from_utf8_lossy(&output.stdout).trim()));
    if path.as_os_str().is_empty() {
        return Err("mktemp ha restituito un percorso vuoto".to_string());
    }
    Ok(TemporaryTree { path })
}

fn combined_output(output: &Output) -> Vec<u8> {
    let mut bytes = output.stdout.clone();
    bytes.extend_from_slice(&output.stderr);
    bytes
}

fn print_tail(contents: &[u8], count: usize) {
    let text = String::from_utf8_lossy(contents);
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().skip(lines.len().saturating_sub(count)) {
        println!("{line}");
    }
}

fn files_equal(left: &Path, right: &Path) -> Result<bool, String> {
    let left_file = File::open(left)
        .map_err(|error| format!("non posso leggere {}: {error}", left.display()))?;
    let right_file = File::open(right)
        .map_err(|error| format!("non posso leggere {}: {error}", right.display()))?;
    let left_len = left_file
        .metadata()
        .map_err(|error| format!("non posso misurare {}: {error}", left.display()))?
        .len();
    let right_len = right_file
        .metadata()
        .map_err(|error| format!("non posso misurare {}: {error}", right.display()))?
        .len();
    if left_len != right_len {
        return Ok(false);
    }

    let mut left_reader = BufReader::new(left_file);
    let mut right_reader = BufReader::new(right_file);
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left_reader
            .read(&mut left_buffer)
            .map_err(|error| format!("non posso confrontare {}: {error}", left.display()))?;
        let right_read = right_reader
            .read(&mut right_buffer)
            .map_err(|error| format!("non posso confrontare {}: {error}", right.display()))?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn print_changes(
    root: &Path,
    stamp: &Path,
    head_rev: &str,
    head_short: &str,
) -> Result<(), String> {
    println!("== cosa entra in servizio ==");
    let previous = fs::read_to_string(stamp)
        .ok()
        .and_then(|contents| read_stamp(&contents));
    if let Some(previous) = previous {
        let exists = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "-e"])
            .arg(format!("{previous}^{{commit}}"))
            .status()
            .map_err(|error| format!("non posso verificare il vecchio timbro con git: {error}"))?;
        if exists.success() {
            let range = format!("{previous}..{head_rev}");
            let log = git_output(root, &["log", "--oneline", &range, "--", "rust"])?;
            print!("{}", String::from_utf8_lossy(&log.stdout));
            let count = git_text(root, &["rev-list", "--count", &range, "--", "rust"])?;
            println!(
                "   ({count} commit che toccano rust/, da {} a {head_short})",
                short_revision(&previous)
            );
            return Ok(());
        }
    }

    println!("   non si sa quale commit abbia prodotto il binario in servizio.");
    println!(
        "   È previsto soltanto la prima volta; da ora la risposta sarà scritta in {}.",
        stamp.display()
    );
    Ok(())
}

fn short_revision(revision: &str) -> &str {
    revision.get(..7).unwrap_or(revision)
}

fn wait_until_ready(root: &Path, service: Service, wait_secs: u64) -> Result<bool, String> {
    let directory = root.join(service.in_progress_rel);
    let started = Instant::now();
    loop {
        let names = receipt_names(&directory)?;
        let state = readiness(&names, &notte::process_exists);
        for name in &state.unknown {
            println!(
                "avviso: ricevuta senza pid '{}' (non blocca il rilascio)",
                name
            );
        }
        if state.is_ready() {
            return Ok(true);
        }
        for busy in &state.busy {
            println!(
                "attendo: il servizio lavora su '{}' con pid {}",
                busy.task, busy.pid
            );
        }
        let elapsed = started.elapsed();
        if elapsed >= Duration::from_secs(wait_secs) {
            return Ok(false);
        }
        let remaining = Duration::from_secs(wait_secs).saturating_sub(elapsed);
        thread::sleep(remaining.min(Duration::from_secs(10)));
    }
}

fn receipt_names(directory: &Path) -> Result<Vec<String>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "non posso leggere le lavorazioni in corso da {}: {error}",
                directory.display()
            ))
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "non posso leggere una ricevuta in {}: {error}",
                directory.display()
            )
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} non ha una cartella padre", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("non posso creare {}: {error}", parent.display()))?;
    let mut staging_name = destination.as_os_str().to_os_string();
    staging_name.push(".new");
    let staging = PathBuf::from(staging_name);
    if let Err(error) = fs::copy(source, &staging) {
        let message = format!(
            "non posso copiare {} in {}: {error}",
            source.display(),
            staging.display()
        );
        let _ = fs::remove_file(&staging);
        return Err(message);
    }
    if let Err(error) = fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)) {
        let message = format!(
            "non posso impostare i permessi di {}: {error}",
            staging.display()
        );
        let _ = fs::remove_file(&staging);
        return Err(message);
    }
    if let Err(error) = fs::rename(&staging, destination) {
        let message = format!(
            "non posso rinominare {} sopra {}: {error}",
            staging.display(),
            destination.display()
        );
        let _ = fs::remove_file(&staging);
        return Err(message);
    }
    Ok(())
}

fn write_stamp(path: &Path, revision: &str, head_short: &str) {
    let result = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "percorso senza padre"))
        .and_then(fs::create_dir_all)
        .and_then(|_| fs::write(path, format!("{revision}\n")));
    if let Err(error) = result {
        let old = fs::read_to_string(path)
            .ok()
            .and_then(|contents| read_stamp(&contents))
            .unwrap_or_else(|| "nulla".to_string());
        eprintln!("== AVVISO: il timbro NON si è potuto scrivere ==");
        eprintln!(
            "   {} nomina {old}; il binario in servizio è {head_short}.",
            path.display()
        );
        eprintln!("   Chi lo legge nominerà il commit sbagliato ({error}).");
        eprintln!("   Scrivi a mano questa riga esatta in {}:", path.display());
        eprintln!("     {revision}");
    }
}

fn restart_service(service: Service) {
    let domain = service_domain(service);
    let result = Command::new("launchctl")
        .args(["kickstart", "-k", &domain])
        .status();
    match result {
        Ok(status) if status.success() => println!("   servizio riavviato: {domain}"),
        Ok(status) => {
            eprintln!(
                "sailor release: il binario è a posto, ma il servizio esegue ancora quello vecchio ({}).",
                status_description(status)
            );
            eprintln!("   Per chiudere il buco esegui: launchctl kickstart -k {domain}");
        }
        Err(error) => {
            eprintln!("sailor release: il binario è a posto, ma il servizio esegue ancora quello vecchio ({error}).");
            eprintln!("   Per chiudere il buco esegui: launchctl kickstart -k {domain}");
        }
    }
}

/// Il dominio launchd da riavviare.
///
/// L'ETICHETTA VIENE DALL'AMBIENTE QUANDO C'È. Il nome di un servizio è una
/// proprietà dell'installazione, non del codice, e questo crate va in un
/// deposito pubblico: chi installa `notte` altrove non si chiama Theo. Il valore
/// scritto nella tabella resta il predefinito di questa macchina.
fn service_domain(service: Service) -> String {
    let label = std::env::var("RELEASE_SERVICE_LABEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| service.label.to_string());
    format!("gui/{}/{}", current_uid(), label)
}

// `getuid` viene dal kernel ed è infallibile; un crate intero per questa sola
// firma allargherebbe dipendenze e tempi di compilazione senza aggiungere nulla.
unsafe extern "C" {
    fn getuid() -> u32;
}

fn current_uid() -> u32 {
    // SAFETY: `getuid` non prende puntatori, non modifica memoria Rust e non
    // può fallire; la firma coincide con `uid_t` sulle piattaforme Unix target.
    unsafe { getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn a_missing_target_is_refused_with_the_usage() {
        assert!(parse_options(&a(&[])).is_err());
    }

    #[test]
    fn dry_run_and_skip_tests_are_read_as_flags() {
        let options = parse_options(&a(&["notte", "--dry-run", "--skip-tests"])).unwrap();
        assert_eq!(options.target_name, "notte");
        assert!(options.dry_run);
        assert!(options.skip_tests);
        assert_eq!(options.wait_secs, 600);
    }

    #[test]
    fn wait_secs_reads_its_number() {
        let options = parse_options(&a(&["notte", "--wait-secs", "30"])).unwrap();
        assert_eq!(options.wait_secs, 30);
    }

    #[test]
    fn an_unknown_option_is_refused() {
        assert!(parse_options(&a(&["notte", "--turbo"])).is_err());
    }
}
