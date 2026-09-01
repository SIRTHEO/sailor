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

/// Le forme di `sailor release`, una per riga. Vedi `flow_cmd::USAGE`.
///
/// **QUI NON SI RICOPIANO I NOMI DEI BERSAGLI.** Nasceva
/// `<notte|hooks|sailor>`: due dei tre erano binari cancellati dal repo il
/// 28/08/2026, e questa riga — scritta il 01/09 su un altro ramo, mentre qui i
/// fossili venivano tolti — li ha riportati sotto gli occhi di chi digita
/// `sailor --help`. È il guasto 10 in miniatura: l'elenco vero è
/// `release::TARGETS`, e chi sbaglia nome se lo sente dire da `target_names()`
/// con la tabella di adesso, non con quella di allora.
pub const USAGE: &[&str] = &["sailor release <target> [--dry-run] [--skip-tests] [--wait-secs N]"];

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut args = args.iter().cloned();
    let target_name = args
        .next()
        .ok_or_else(|| format!("no target given (usage: {})", USAGE[0]))?;
    if target_name.starts_with('-') {
        return Err(format!("no target before '{target_name}'"));
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
                    .ok_or_else(|| "--wait-secs needs a number".to_string())?;
                wait_secs = value
                    .parse::<u64>()
                    .map_err(|_| format!("not a number for --wait-secs: '{value}'"))?;
            }
            _ => return Err(format!("unknown option '{arg}'")),
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
    let root = sources_root()?;
    let head_rev = git_text(&root, &["rev-parse", "HEAD"])?;
    let head_short = git_text(&root, &["rev-parse", "--short", "HEAD"])?;
    let source_rev = {
        let revision = git_text(&root, &["log", "-1", "--format=%H", "--", "crates/"])?;
        if revision.is_empty() {
            head_rev.clone()
        } else {
            revision
        }
    };

    let dirty = git_output(&root, &["status", "--porcelain", "--", "crates"])?;
    let dirty_count = String::from_utf8_lossy(&dirty.stdout).lines().count();
    if dirty_count > 0 {
        println!(
            "note: {dirty_count} uncommitted file(s) under crates/ stay out of service, by construction"
        );
    }

    let temporary = make_temporary_tree()?;
    let repository = temporary.path.join("repo");
    println!("== cloning HEAD ({head_short}) into a throwaway tree ==");
    clone_repository(&root, &repository)?;
    command_success(
        Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "--quiet"])
            .arg(&head_rev),
        "checking HEAD out in the clone failed",
    )?;

    let build_target = root.join("target/from-head");
    // I crate stanno alla radice dell'albero dal trasloco del 27/08/2026: non
    // c'è più un sottoalbero da cui compilare.
    let cloned_rust = repository.clone();
    println!("== compilo da quell'albero ==");
    let build = Command::new("cargo")
        .current_dir(&cloned_rust)
        .env("CARGO_TARGET_DIR", &build_target)
        .args(["build", "--release", "--bin", selected.bin])
        .output()
        .map_err(|error| format!("cannot start cargo: {error}"))?;
    print_tail(&combined_output(&build), 5);
    if !build.status.success() {
        return Err(
            "HEAD does not compile: nothing was replaced and the binary in service is intact"
                .to_string(),
        );
    }

    let fresh = build_target.join("release").join(selected.bin);
    if !fresh.is_file() {
        return Err(format!(
            "the build succeeded and produced no {}",
            fresh.display()
        ));
    }

    if options.skip_tests {
        println!("== TESTS SKIPPED (--skip-tests): this release was NOT tested ==");
    } else {
        println!("== running the whole suite on HEAD ==");
        let suite_path = temporary.path.join("suite.txt");
        let suite_file = File::create(&suite_path)
            .map_err(|error| format!("cannot create {}: {error}", suite_path.display()))?;
        let suite_error = suite_file
            .try_clone()
            .map_err(|error| format!("cannot duplicate the suite file: {error}"))?;
        let suite_status = Command::new("cargo")
            .current_dir(&cloned_rust)
            .env("CARGO_TARGET_DIR", &build_target)
            // `--no-fail-fast` OR CARGO STOPS AT THE FIRST RED BINARY, and the
            // ones after it do not fail: they never start. A release that reads
            // "the suite is red" would name one binary while ten more were
            // never attempted, and whoever repairs that one releases blind.
            .args(["test", "--release", "--no-fail-fast", "--", "--nocapture"])
            .stdout(Stdio::from(suite_file))
            .stderr(Stdio::from(suite_error))
            .status()
            .map_err(|error| format!("cannot start the suite: {error}"))?;
        let suite = fs::read(&suite_path)
            .map_err(|error| format!("cannot read {} back: {error}", suite_path.display()))?;
        print_tail(&suite, 25);
        if !suite_status.success() {
            eprintln!("sailor release: the suite is red on HEAD: nothing was replaced and the binary in service is intact.");
            eprintln!("   Green commits apart do not add up to a green whole: this is that case.");
            eprintln!("   If the binary in service is broken and waiting costs more, run again with --skip-tests.");
            return Ok(1);
        }
        // THE MARKER THIS COUNTS IS WRITTEN BY NOBODY, so the number is always
        // zero and reads as "everything ran". The tests that printed it were in
        // the crates deleted with everything that was not Sailor; the counter
        // stayed. Kept, and named, because the question it asks is the right
        // one — a test the sandbox denied is not a test that passed — and
        // whoever restores a marker restores an answer, not a branch.
        let not_run = String::from_utf8_lossy(&suite)
            .matches(TEST_DID_NOT_RUN)
            .count();
        if not_run > 0 {
            println!("== {not_run} test(s) NOT RUN (not failed) ==");
            println!(
                "   Green here does not mean tested there: the sandbox denied what they asked for."
            );
        }
    }

    // TWO ROOTS, AND THEY ARE NOT THE SAME THING. `root` is what gets built;
    // `home` is what gets installed into. They coincided once, and moving the
    // first moved the second by accident: the binary landed beside the sources
    // while the hooks went on running the old one from where it had always been.
    let home = install_root()?;
    let live = root.join(selected.live_rel);
    let stamp = home.join(selected.stamp_rel);
    // The house itself has moved since. A machine that released before then has
    // no stamp here yet and a real one back there.
    let stamp_to_read = if stamp.is_file() {
        stamp.clone()
    } else {
        stamp_left_behind(selected.stamp_rel).unwrap_or_else(|| stamp.clone())
    };
    if live.is_file() && files_equal(&fresh, &live)? {
        println!("== nothing to do: the binary in service already matches HEAD ({head_short}) ==");
        if !options.dry_run {
            write_stamp(&stamp, &source_rev, &head_short);
        }
        return Ok(0);
    }

    print_changes(&root, &stamp_to_read, &head_rev, &head_short)?;
    if options.dry_run {
        println!("== dry run: the binary in service was NOT replaced ==");
        return Ok(0);
    }

    if let Some(service) = selected.service {
        let ready = wait_until_ready(&root, service, options.wait_secs)?;
        if !ready {
            println!("sailor release: release postponed: the service is still working; nothing was replaced");
            return Ok(3);
        }
    }

    println!("== replacing the binary in service ==");
    atomic_copy(&fresh, &live)?;
    println!("   in service: {head_short}");

    let safe = home.join(selected.safe_rel);
    match atomic_copy(&fresh, &safe) {
        Ok(()) => println!("   also outside target/: {}", safe.display()),
        Err(error) => {
            eprintln!("== WARNING: the safety copy could NOT be written ==");
            eprintln!("   {error}");
            eprintln!(
                "   The binary in service is the new one, but anybody's next build can still overwrite it."
            );
        }
    }

    write_stamp(&stamp, &source_rev, &head_short);

    if let Some(service) = selected.service {
        // The service runs every 90 seconds: between the first check and this
        // point it may have taken another job, which the restart would cut off.
        if !wait_until_ready(&root, service, 0)? {
            let domain = service_domain(service);
            println!("sailor release: release postponed: the service has started another job; the new binary and the stamp are in place, but the service is still running the old one.");
            println!(
                "   When that job ends, close the release with: launchctl kickstart -k {domain}"
            );
            return Ok(3);
        }
        restart_service(service);
    }
    Ok(0)
}

/// Where the sources sit below the home.
///
/// A constant because the tests name it: one that spelled `personal/sailor` out
/// by hand would stay green with the function returned to cloning the
/// configuration directory, which is the release fault back on its feet.
const SOURCES_BELOW_HOME: &str = "personal/sailor";

/// What a test prints instead of failing when it could not run at all.
///
/// A contract with no party on the other side today: the tests that wrote it
/// were deleted along with everything that was not Sailor. It is a constant so
/// that whoever writes the next one has a name to write, rather than guessing
/// the spelling of a string buried in a counter.
const TEST_DID_NOT_RUN: &str = "TEST DID NOT RUN";

/// The house the binary is installed into, and where the stamp lives.
///
/// Sailor's own home now; it used to be another product's, for no reason but
/// habit. Whoever moves it re-runs `sailor session install`: the hooks name the
/// binary by absolute path, so installing elsewhere leaves the old one in
/// service with nobody noticing.
fn install_root() -> Result<PathBuf, String> {
    ledger::sailor_home().ok_or_else(|| "no house to install into: HOME is not set".to_owned())
}

/// The stamp left in the previous house, if this house has none yet.
///
/// A missing stamp is not a harmless zero: the release stops being able to say
/// which commits enter service and prints its widest answer instead. On a
/// machine that released before the move the stamp is real, only elsewhere:
/// read there once, written here from then on.
fn stamp_left_behind(stamp_rel: &str) -> Option<PathBuf> {
    let previous = previous_stamp_path(env::var_os("HOME"), stamp_rel)?;
    previous.is_file().then_some(previous)
}

/// Where it would be, without asking the disk. Separated so the rule can be
/// tested on a declared home rather than on the machine running the test,
/// which is fault 5.
fn previous_stamp_path(home: Option<OsString>, stamp_rel: &str) -> Option<PathBuf> {
    Some(
        PathBuf::from(home.filter(|value| !value.is_empty())?)
            .join(release::PREVIOUS_INSTALL_BELOW_HOME)
            .join(stamp_rel),
    )
}

/// The sources tree what goes into service is built from.
///
/// It once pointed at the configuration directory and put the past back into
/// service. It no longer reads `SAILOR_HOME` either: everywhere else that
/// variable means Sailor's *configuration* home, so reading it here as the
/// sources was the same fault waiting behind a second door.
fn sources_root() -> Result<PathBuf, String> {
    root_under(
        env::var_os("SAILOR_SOURCES"),
        env::var_os("HOME"),
        SOURCES_BELOW_HOME,
        "SAILOR_SOURCES",
    )
}

/// La radice dichiarata, o quella dedotta dalla casa: la regola, senza l'ambiente.
///
/// **RICEVE I DUE VALORI INVECE DI LEGGERLI, ED È IL GUASTO 5.** `HOME` è
/// globale al processo: una prova che lo scrivesse per provare questa regola
/// rovinerebbe le altre a caso, e una che lo *legge* diventa rossa su una
/// macchina diversa a codice invariato — misurato il 01/09/2026, casa vuota e
/// `the_release_builds_from_the_sources_and_not_from_the_configuration` rossa
/// senza che nessuno avesse toccato una riga. I due chiamanti qui sopra restano
/// gli unici a guardare l'ambiente, e non decidono niente.
///
/// Una variabile dichiarata **vuota** non vale come dichiarazione: sarebbe una
/// radice alla cartella corrente, cioè il guasto 25 travestito.
fn root_under(
    declared: Option<OsString>,
    home: Option<OsString>,
    below: &str,
    declared_name: &str,
) -> Result<PathBuf, String> {
    if let Some(root) = declared.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    home.map(|home| PathBuf::from(home).join(below))
        .ok_or_else(|| format!("neither {declared_name} nor HOME is set"))
}

fn git_output(root: &Path, args: &[&str]) -> Result<Output, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("cannot start git: {error}"))?;
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
        .map_err(|error| format!("cannot start git clone: {error}"))?;
    if first.status.success() {
        return Ok(());
    }

    // The sandbox denies hardlinks to `.git` objects: the clone stays local and
    // stays a real repository, but has to copy the objects instead of linking
    // them. Outside the sandbox the short road above still costs practically no
    // space.
    if repository.exists() {
        fs::remove_dir_all(repository).map_err(|error| {
            format!(
                "the local clone failed and the partial clone {} cannot be removed: {error}",
                repository.display()
            )
        })?;
    }
    println!("note: hardlinks for the local clone are denied; copying the git objects");
    let second = Command::new("git")
        .args(["clone", "--local", "--no-hardlinks", "--quiet"])
        .arg(root)
        .arg(repository)
        .output()
        .map_err(|error| format!("cannot retry git clone without hardlinks: {error}"))?;
    if second.status.success() {
        Ok(())
    } else {
        let first_detail = String::from_utf8_lossy(&first.stderr);
        let second_detail = String::from_utf8_lossy(&second.stderr);
        Err(format!(
            "the local clone of HEAD failed (first: {}; without hardlinks: {})",
            first_detail.trim(),
            second_detail.trim()
        ))
    }
}

fn status_description(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "ended by a signal".to_string())
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
        .map_err(|error| format!("cannot start mktemp: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "mktemp created no temporary directory: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let path = PathBuf::from(OsStr::new(String::from_utf8_lossy(&output.stdout).trim()));
    if path.as_os_str().is_empty() {
        return Err("mktemp returned an empty path".to_string());
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
    let left_file =
        File::open(left).map_err(|error| format!("cannot read {}: {error}", left.display()))?;
    let right_file =
        File::open(right).map_err(|error| format!("cannot read {}: {error}", right.display()))?;
    let left_len = left_file
        .metadata()
        .map_err(|error| format!("cannot measure {}: {error}", left.display()))?
        .len();
    let right_len = right_file
        .metadata()
        .map_err(|error| format!("cannot measure {}: {error}", right.display()))?
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
            .map_err(|error| format!("cannot compare {}: {error}", left.display()))?;
        let right_read = right_reader
            .read(&mut right_buffer)
            .map_err(|error| format!("cannot compare {}: {error}", right.display()))?;
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
            .map_err(|error| format!("cannot check the old stamp with git: {error}"))?;
        if exists.success() {
            let range = format!("{previous}..{head_rev}");
            let log = git_output(root, &["log", "--oneline", &range, "--", "crates"])?;
            print!("{}", String::from_utf8_lossy(&log.stdout));
            let count = git_text(root, &["rev-list", "--count", &range, "--", "crates"])?;
            println!(
                "   ({count} commit(s) touching crates/, from {} to {head_short})",
                short_revision(&previous)
            );
            return Ok(());
        }
    }

    println!("   which commit produced the binary in service is not known.");
    println!(
        "   That is expected the first time only; from now on the answer is written in {}.",
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
        let state = readiness(&names, &release::process_exists);
        for name in &state.unknown {
            println!(
                "warning: receipt with no pid '{}' (does not block the release)",
                name
            );
        }
        if state.is_ready() {
            return Ok(true);
        }
        for busy in &state.busy {
            println!(
                "waiting: the service is working on '{}' with pid {}",
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
                "cannot read the jobs in progress from {}: {error}",
                directory.display()
            ))
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("cannot read a receipt in {}: {error}", directory.display())
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let mut staging_name = destination.as_os_str().to_os_string();
    staging_name.push(".new");
    let staging = PathBuf::from(staging_name);
    if let Err(error) = fs::copy(source, &staging) {
        let message = format!(
            "cannot copy {} to {}: {error}",
            source.display(),
            staging.display()
        );
        let _ = fs::remove_file(&staging);
        return Err(message);
    }
    if let Err(error) = fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)) {
        let message = format!(
            "cannot set the permissions of {}: {error}",
            staging.display()
        );
        let _ = fs::remove_file(&staging);
        return Err(message);
    }
    if let Err(error) = fs::rename(&staging, destination) {
        let message = format!(
            "cannot rename {} over {}: {error}",
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
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path with no parent"))
        .and_then(fs::create_dir_all)
        .and_then(|_| fs::write(path, format!("{revision}\n")));
    if let Err(error) = result {
        let old = fs::read_to_string(path)
            .ok()
            .and_then(|contents| read_stamp(&contents))
            .unwrap_or_else(|| "nothing".to_string());
        eprintln!("== WARNING: the stamp could NOT be written ==");
        eprintln!(
            "   {} names {old}; the binary in service is {head_short}.",
            path.display()
        );
        eprintln!("   Whoever reads it will name the wrong commit ({error}).");
        eprintln!("   Write this exact line by hand in {}:", path.display());
        eprintln!("     {revision}");
    }
}

fn restart_service(service: Service) {
    let domain = service_domain(service);
    let result = Command::new("launchctl")
        .args(["kickstart", "-k", &domain])
        .status();
    match result {
        Ok(status) if status.success() => println!("   service restarted: {domain}"),
        Ok(status) => {
            eprintln!(
                "sailor release: the binary is in place, but the service is still running the old one ({}).",
                status_description(status)
            );
            eprintln!("   To close the gap run: launchctl kickstart -k {domain}");
        }
        Err(error) => {
            eprintln!("sailor release: the binary is in place, but the service is still running the old one ({error}).");
            eprintln!("   To close the gap run: launchctl kickstart -k {domain}");
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

    /// Da dove si costruisce ciò che va in servizio.
    ///
    /// IL BRACCIO CHE CONTA È IL SECONDO: la cartella della configurazione è
    /// anche un repo git, e finché il rilascio clonava quella rimetteva in
    /// servizio l'albero da cui i sorgenti se n'erano andati — un rilascio
    /// verde che disinstallava il lavoro della mattina.
    ///
    /// **NON LEGGE PIÙ LA MACCHINA DI CHI LA ESEGUE, ED È IL GUASTO 5.** Fino
    /// al 01/09/2026 chiedeva `sources_root()`, cioè `$HOME`, e poi guardava
    /// sul disco se quella cartella conteneva `crates/sailor`. Misurato quel
    /// giorno con una casa vuota: rossa, a codice invariato — che è
    /// letteralmente la riga del guasto 5. Adesso la regola si prova con valori
    /// dichiarati; la forma dell'albero la prova il caso qui sotto, sull'albero
    /// da cui questa prova è compilata.
    #[test]
    fn the_release_builds_from_the_sources_and_not_from_the_configuration() {
        let home = Some(OsString::from("/casa/di-chiunque"));
        let sources = root_under(None, home, SOURCES_BELOW_HOME, "SAILOR_SOURCES").unwrap();
        let house = ledger::sailor_home_in(None, None, PathBuf::from("/casa/di-chiunque"));

        // Spelled out on purpose: were the constant returned to the
        // configuration directory, this line would go red.
        assert!(sources.ends_with("personal/sailor"), "{sources:?}");
        assert!(
            !sources.ends_with(".config/sailor"),
            "the release is cloning the configuration home again: {sources:?}"
        );
        assert_ne!(sources, house);
    }

    /// The stamp of a machine that released before the house moved.
    ///
    /// It is looked for under the previous house, with the target's own
    /// relative path — not a second string saying where stamps go. A stamp read
    /// from the wrong place is worse than none: the release would name commits
    /// that never entered service, and say it with a straight face.
    #[test]
    fn the_stamp_of_the_previous_house_keeps_the_target_s_own_path() {
        let home = Some(OsString::from("/casa/di-chiunque"));
        let target = release::target("sailor").expect("the table names it");

        assert_eq!(
            previous_stamp_path(home, target.stamp_rel),
            Some(
                PathBuf::from("/casa/di-chiunque")
                    .join(release::PREVIOUS_INSTALL_BELOW_HOME)
                    .join("state/sailor-binary-commit")
            )
        );

        // A home exported empty by a script that could not find it would put the
        // stamp at the root of the disk. It is not a home.
        assert_eq!(previous_stamp_path(Some(OsString::new()), "state/x"), None);
        assert_eq!(previous_stamp_path(None, "state/x"), None);
    }

    /// A declared root beats the home, and a declared empty one does not.
    ///
    /// The second arm is the one that gets lost: a variable exported empty by a
    /// script that could not find it would give `personal/sailor` **relative**,
    /// a clone wherever the process happens to stand — fault 25 dressed up as
    /// configuration.
    #[test]
    fn a_declared_root_wins_over_the_home_but_an_empty_one_does_not() {
        let home = Some(OsString::from("/casa/di-chiunque"));
        let declared = root_under(
            Some(OsString::from("/altrove/sailor")),
            home.clone(),
            SOURCES_BELOW_HOME,
            "SAILOR_SOURCES",
        )
        .unwrap();
        assert_eq!(declared, PathBuf::from("/altrove/sailor"));

        let empty = root_under(
            Some(OsString::new()),
            home,
            SOURCES_BELOW_HOME,
            "SAILOR_SOURCES",
        )
        .unwrap();
        assert_eq!(empty, PathBuf::from("/casa/di-chiunque/personal/sailor"));

        assert!(root_under(None, None, "personal/sailor", "SAILOR_SOURCES").is_err());
    }

    /// I sorgenti di Sailor portano il crate che dà il nome al binario.
    ///
    /// IL PUNTO DI RIFERIMENTO È CAMBIATO IL 29/08/2026, e vale la pena dire
    /// perché: qui c'era `crates/claude-hooks`, cancellato insieme a tutto ciò
    /// che non era Sailor. Il rilascio non è cambiato di una riga — è cambiato
    /// il segnale con cui questa prova lo riconosceva.
    ///
    /// **L'ALBERO GUARDATO È QUELLO DA CUI QUESTA PROVA È COMPILATA**, non
    /// quello che vive nella casa di chi la esegue: `CARGO_MANIFEST_DIR` è
    /// versionato e c'è sempre, `$HOME/personal/sailor` è una scommessa sulla
    /// macchina.
    #[test]
    fn the_sources_of_this_checkout_carry_the_crate_that_names_the_binary() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|crates| crates.parent())
            .expect("il crate sta in <radice>/crates/sailor")
            .to_path_buf();
        assert!(root.join("crates").join("sailor").is_dir(), "{root:?}");
        assert!(!root.join("rust").exists(), "{root:?}");
    }

    /// Le due radici sono due, e il binario va installato nella seconda.
    ///
    /// IL BRACCIO CHE CONTA è che non coincidano: quando la sorgente si è
    /// spostata portandosi dietro l'installazione, il rilascio ha scritto il
    /// binario accanto ai sorgenti e i ganci hanno continuato a eseguire quello
    /// vecchio — con uscita 0 e nessun avviso.
    /// **ANCHE QUESTA RICEVE LA CASA INVECE DI LEGGERLA** (guasto 5): non
    /// falliva a casa vuota — confronta percorsi e non tocca il disco — ma
    /// `HOME` assente la faceva morire su un `unwrap`, e un caso che dipende
    /// dall'ambiente per **partire** dipende dall'ambiente.
    ///
    /// **GUARDA TUTTI I BERSAGLI, NON UNO SCELTO A MANO.** Fino al 01/09/2026
    /// questa prova chiedeva `target("hooks")` — un bersaglio che nominava un
    /// binario cancellato dal repo il 28/08 — e con la sua rimozione sarebbe
    /// morta su un `expect`. Un caso scritto su un nome è una prova che va
    /// aggiornata a ogni tabella; scritta sulla tabella, copre anche il
    /// bersaglio che qualcuno aggiungerà domani.
    #[test]
    fn the_binary_is_installed_in_the_home_and_not_next_to_the_sources() {
        let declared_home = Some(OsString::from("/casa/di-chiunque"));
        let sources =
            root_under(None, declared_home, SOURCES_BELOW_HOME, "SAILOR_SOURCES").unwrap();
        let home = ledger::sailor_home_in(None, None, PathBuf::from("/casa/di-chiunque"));
        assert_ne!(sources, home);
        assert!(home.ends_with(".config/sailor"), "{home:?}");

        for candidate in release::TARGETS {
            let installed = home.join(candidate.safe_rel);
            assert!(
                installed.starts_with(&home),
                "{}: il binario finirebbe fuori dalla casa: {installed:?}",
                candidate.name
            );
            assert!(
                !installed.starts_with(&sources),
                "{}: il binario finirebbe accanto ai sorgenti, dove nessuno lo esegue: {installed:?}",
                candidate.name
            );
        }
    }

    // I tre casi qui sotto nominavano `notte` fino al 01/09/2026. `parse_options`
    // non giudica il nome — chi non esiste lo scarta `target()` più tardi —
    // quindi restavano verdi su un bersaglio cancellato: un dato di prova che
    // racconta un mondo sparito non rompe niente, e per questo invecchia.
    #[test]
    fn dry_run_and_skip_tests_are_read_as_flags() {
        let options = parse_options(&a(&["sailor", "--dry-run", "--skip-tests"])).unwrap();
        assert_eq!(options.target_name, "sailor");
        assert!(options.dry_run);
        assert!(options.skip_tests);
        assert_eq!(options.wait_secs, 600);
    }

    #[test]
    fn wait_secs_reads_its_number() {
        let options = parse_options(&a(&["sailor", "--wait-secs", "30"])).unwrap();
        assert_eq!(options.wait_secs, 30);
    }

    #[test]
    fn an_unknown_option_is_refused() {
        assert!(parse_options(&a(&["sailor", "--turbo"])).is_err());
    }
}
