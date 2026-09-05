//! Real processes: starting them, writing them in the ledger, stopping them.
//!
//! **EVERY PROCESS STARTED HERE IS IN THE LEDGER BEFORE IT IS USED** — fault 4
//! cured where it is born. `Process::start` is the one road, it records, and
//! it opens only to the supervisor's `StartToken`: a second road does not
//! compile.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ledger::{Ledger, ProcessEndRecord, ProcessRecord};

use crate::{now, BuildOutcome, Running, StartToken};

/// Cosa accendere.
#[derive(Debug, Clone)]
pub struct Spec {
    /// Il nome stabile con cui lo si ritrova dopo un riavvio. Non è il pid.
    pub process_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    /// La porta che occuperà, se ne occupa una. Va dichiarata **prima**
    /// dell'avvio: è ciò che permette a chi arriva dopo di chiedere «chi tiene
    /// la 5183» invece di scoprirlo quando il proprio avvio fallisce.
    pub port: Option<u16>,
    pub purpose: String,
    pub started_by: String,
    /// What is laid over the environment of whoever lights it. **Empty means
    /// «whatever the parent has»**, which a development server and the window
    /// both want; it also means a profile never arrives by accident.
    pub environment: Vec<(String, String)>,
}

/// Un processo acceso da Sailor, che sa di esserlo.
pub struct Process {
    spec: Spec,
    child: std::process::Child,
    store: Option<Ledger>,
    stopped: bool,
}

impl Process {
    /// Nasce in un **gruppo di processi suo**, perché spegnerlo spenga anche
    /// chi ha acceso lui.
    ///
    /// `sailor-live` avvia `cargo` e il server della finestra, che di figli ne
    /// fanno: senza il gruppo, `kill` arriva al capostipite e il nipote resta
    /// vivo con la porta in mano. Il gemello sta in `actions::run_with_timeout`.
    fn in_its_own_group(command: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(not(unix))]
        let _ = command;
    }

    /// Manda il segnale al **gruppo**, che porta il numero del capogruppo: il
    /// segno meno lo dice a `kill`. Limite noto: un nipote che si stacca da
    /// solo con `setsid` esce dal gruppo e sopravvive.
    fn signal_the_whole_group(pid: u32) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
        #[cfg(not(unix))]
        let _ = pid;
    }

    /// Spawns, then records the real pid: a record that fails stops what it
    /// started rather than leave an orphan (fault 4). Without the token:
    /// ```compile_fail
    /// # use supervisor::child::{Process, Spec}; fn spec() -> Spec { unimplemented!() }
    /// let started = Process::start(spec(), None);
    /// ```
    pub fn start(spec: Spec, token: &StartToken) -> Result<Self, String> {
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .current_dir(&spec.working_directory)
            .envs(spec.environment.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null());
        Self::in_its_own_group(&mut command);
        let child = command
            .spawn()
            .map_err(|error| format!("avviare {}: {error}", spec.command))?;

        let mut process = Self {
            spec,
            child,
            store: token.ledger().cloned(),
            stopped: false,
        };

        if let Some(store) = process.store.as_ref() {
            let record = ProcessRecord {
                process_id: process.spec.process_id.clone(),
                pid: process.child.id(),
                command: process.spec.command.clone(),
                args: process.spec.args.clone(),
                working_directory: process.spec.working_directory.display().to_string(),
                port: process.spec.port,
                purpose: process.spec.purpose.clone(),
                started_by: process.spec.started_by.clone(),
                run_id: None,
                started_at: now(),
            };
            if let Err(error) = store.record_process_started(&record) {
                Self::signal_the_whole_group(process.child.id());
                let _ = process.child.kill();
                let _ = process.child.wait();
                return Err(format!(
                    "il processo è partito ma il deposito non l'ha accettato, \
                     quindi è stato spento invece di restare orfano: {error}"
                ));
            }
        }

        Ok(process)
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn process_id(&self) -> &str {
        &self.spec.process_id
    }

    /// È uscito da solo? Non è una domanda al sistema operativo per nome: si
    /// interroga **questo** figlio, che è nostro.
    pub fn exited(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        }
    }

    /// Scrive nel deposito che è finito. Chiamarla due volte non fa danno: la
    /// seconda scrive la stessa chiusura.
    pub fn record_end(&mut self, exit_code: Option<i32>) {
        if let Some(store) = self.store.as_ref() {
            let _ = store.record_process_ended(&ProcessEndRecord {
                process_id: self.spec.process_id.clone(),
                exit_code,
                ended_at: now(),
            });
        }
        self.stopped = true;
    }
}

impl Running for Process {
    fn stop(&mut self) -> Result<(), String> {
        Process::signal_the_whole_group(self.child.id());
        let outcome = self.child.kill();
        let code = self.child.wait().ok().and_then(|status| status.code());
        self.record_end(code);
        // Un figlio già uscito da solo fa fallire `kill`: non è un guasto, è la
        // condizione che si voleva ottenere.
        match outcome {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// **CHI NON LO SPEGNE ESPLICITAMENTE LO SPEGNE COMUNQUE.** Senza questo, ogni
/// strada di errore che abbandona un `Process` — un `?`, un panico — lascia
/// acceso un processo che il deposito continua a dare per vivo. È il guasto 4
/// che rinasce da una porta che nessuno guarda.
impl Drop for Process {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.stop();
        }
    }
}

/// Costruisce, e riporta cosa ha detto il compilatore.
///
/// **L'USCITA D'ERRORE SI TIENE INTERA.** `cargo` scrive le diagnosi su stderr;
/// buttarle e riportare solo «fallita» costringerebbe chi guarda a tornare nel
/// terminale, cioè a fare a mano il lavoro che questa modalità dovrebbe
/// togliere.
pub fn cargo_build(manifest: &Path, jobs: Option<u32>) -> BuildOutcome {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .stdin(Stdio::null());
    if let Some(jobs) = jobs {
        command.arg("-j").arg(jobs.to_string());
    }
    match command.output() {
        Ok(output) if output.status.success() => BuildOutcome::Succeeded,
        Ok(output) => BuildOutcome::Failed {
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => BuildOutcome::Failed {
            message: format!("`cargo build` non è nemmeno partito: {error}"),
        },
    }
}

/// L'istante dell'ultima modifica sotto queste radici, in secondi.
///
/// **UN SONDAGGIO E NON UN OSSERVATORE, E IL PERCHÉ È DICHIARATO.** Un
/// osservatore vero (`notify`) vorrebbe una dipendenza nuova, e le dipendenze
/// di questo albero sono tenute al minimo per scelta scritta in `Cargo.toml`.
/// La differenza che si paga è un ritardo di mezzo secondo su una ricostruzione
/// che ne dura decine: non si sente. La differenza che si guadagna è che questa
/// funzione si può leggere tutta.
pub fn newest_change(roots: &[PathBuf]) -> u64 {
    fn walk(directory: &Path, newest: &mut u64) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                // `target` cambia a ogni costruzione: guardarlo vorrebbe dire
                // che ogni ricostruzione ne chiede un'altra, per sempre.
                if matches!(name.as_str(), "target" | "node_modules" | ".git" | "dist") {
                    continue;
                }
                walk(&path, newest);
                continue;
            }
            if !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs") | Some("toml") | Some("json")
            ) {
                continue;
            }
            let seen = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_secs());
            if seen > *newest {
                *newest = seen;
            }
        }
    }

    let mut newest = 0;
    for root in roots {
        walk(root, &mut newest);
    }
    newest
}
