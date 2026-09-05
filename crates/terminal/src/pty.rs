//! Lo pseudo-terminale: la sola parte di questo crate che tocca il sistema
//! operativo.
//!
//! **PERCHÉ UNO PSEUDO-TERMINALE E NON UNA PIPE.** Una pipe basterebbe a leggere
//! l'uscita di un comando, e infatti `actions` la usa. Non basta a un terminale:
//! un programma che scopre di non parlare a un terminale cambia comportamento —
//! niente colori, niente domande interattive, `git` non pagina, la shell non
//! stampa il proprio invito. Un terminale che si comporta diversamente da un
//! terminale non è il prodotto.
//!
//! **I DUE CAPI SI CHIAMANO `leader` E `follower`.** È la coppia di nomi che
//! POSIX e i sistemi operativi hanno adottato per quelli che le pagine di manuale
//! vecchie chiamano master e slave. Il capo `leader` resta a noi: ci si scrive
//! ciò che l'utente digita e ci si legge ciò che il terminale mostra. Il capo
//! `follower` diventa i tre descrittori del processo figlio.
//!
//! **`posix_openpt` E NON `openpty`.** Fanno la stessa cosa, ma `openpty` su
//! Linux sta in `libutil` e vuole una riga di collegamento in più, mentre le
//! quattro chiamate POSIX (`posix_openpt`, `grantpt`, `unlockpt`, `ptsname`)
//! stanno nella libreria di sistema ovunque. Meno da spiegare a chi compila
//! altrove.

use crate::{locked, Workspace};
use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Ciò che può andare storto aprendo o guidando uno pseudo-terminale.
///
/// **OGNI VOCE DICE QUALE GESTO È FALLITO**, non solo che qualcosa è fallito:
/// «non si è aperto» manda a cercare in quattro posti diversi, e i quattro
/// hanno riparazioni diverse.
pub enum PtyError {
    /// Il sistema operativo non ha dato un terminale nuovo.
    NotOpened(io::Error),
    /// Il capo del figlio non si è potuto preparare o aprire.
    FollowerNotReady(io::Error),
    /// Il programma non è partito: binario assente, non eseguibile, cartella
    /// sparita fra il controllo e l'avvio.
    NotStarted(io::Error),
    /// Scrittura, lettura o ridimensionamento su un terminale già chiuso o
    /// rotto.
    Broken(io::Error),
    /// The terminal opened, but its letterbox or its count could not be put
    /// where whoever types from outside will look for them.
    NotRegistered(io::Error),
}

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** A derived one showed
/// the `io::Error` bare and hid the gesture that failed.
impl std::fmt::Debug for PtyError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, out)
    }
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyError::NotOpened(error) => {
                write!(out, "il sistema non ha dato uno pseudo-terminale: {error}")
            }
            PtyError::FollowerNotReady(error) => write!(
                out,
                "lo pseudo-terminale si è aperto, ma il capo del figlio non si è potuto preparare: {error}"
            ),
            PtyError::NotStarted(error) => {
                write!(out, "il programma del terminale non è partito: {error}")
            }
            PtyError::Broken(error) => write!(out, "il terminale non risponde più: {error}"),
            PtyError::NotRegistered(error) => write!(
                out,
                "the terminal opened, but its letterbox could not be registered: {error}"
            ),
        }
    }
}

impl std::error::Error for PtyError {}

/// Quanto è grande la finestra del terminale, in caratteri.
///
/// **NON HA UN VALORE PREDEFINITO NASCOSTO NEL SISTEMA.** Uno pseudo-terminale
/// nasce a zero righe e zero colonne, e un programma che chiede quanto è largo
/// lo schermo si sente rispondere zero: `less` non pagina, un editor si disegna
/// su una riga sola. La misura si dichiara aprendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub columns: u16,
}

impl Default for Size {
    /// Le ventiquattro righe per ottanta colonne che ogni terminale assume
    /// quando nessuno gliene ha dette altre.
    fn default() -> Size {
        Size {
            rows: 24,
            columns: 80,
        }
    }
}

/// Uno pseudo-terminale aperto, col processo che ci gira dentro.
pub struct Pty {
    /// Il capo che resta a noi. Sotto un lucchetto perché chi scrive e chi
    /// ridimensiona possono essere due fili diversi.
    leader: Mutex<File>,
    child: Mutex<Child>,
    /// The device the program inside is attached to, kept because it is the
    /// name that program reports as its own terminal.
    device: String,
}

/// Il numero non trattabile che `ptsname` restituisce da un buffer statico
/// condiviso: due terminali aperti insieme da due fili si porterebbero via il
/// nome l'uno dell'altro.
///
/// **`ptsname_r` NON È PORTABILE**: esiste su Linux e non su macOS. Un lucchetto
/// di trenta microsecondi attorno alla chiamata costa meno di due strade da
/// mantenere.
static PTSNAME_LOCK: Mutex<()> = Mutex::new(());

impl Pty {
    /// Apre uno pseudo-terminale e ci avvia `program` **dentro** `workspace`.
    ///
    /// La cartella non è un'opzione fra le altre: è il primo argomento perché
    /// un terminale senza spazio di lavoro non è una cosa che questo crate sa
    /// fabbricare.
    pub fn open(
        workspace: &Workspace,
        program: &OsStr,
        args: &[&OsStr],
        size: Size,
        environment: &[(String, String)],
    ) -> Result<Pty, PtyError> {
        let leader = open_leader()?;
        let follower_name = follower_name(&leader)?;
        let follower = open_follower(&follower_name)?;
        // **LA MISURA SI DÀ DOPO CHE IL CAPO DEL FIGLIO È APERTO, E LA PRIMA
        // VERSIONE LO FACEVA PRIMA.** Su macOS un capo `leader` a cui nessuno
        // sta ancora dall'altra parte non è un terminale, e il sistema risponde
        // «ioctl inappropriata per questo dispositivo» (errno 25) — misurato il
        // 31/08/2026. Prima di dirlo, il terminale nasceva a zero righe per
        // zero colonne.
        set_size(&leader, size).map_err(PtyError::Broken)?;

        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(&workspace.root);
        // Tre descrittori distinti sullo stesso terminale: se il figlio chiude
        // il proprio standard input, non deve portarsi via anche la sua uscita.
        command.stdin(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        command.stdout(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        command.stderr(Stdio::from(
            follower.try_clone().map_err(PtyError::FollowerNotReady)?,
        ));
        for (name, value) in environment {
            command.env(name, value);
        }

        // Il figlio deve diventare capo di una sessione nuova e prendersi questo
        // terminale come terminale di controllo: senza, un Ctrl-C non arriva a
        // lui, e i programmi che chiedono «chi è in primo piano?» si sentono
        // rispondere che non c'è nessuno.
        let leader_fd = leader.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                // Il nostro capo non deve restare aperto nel figlio: finché
                // esiste una copia, chi legge il terminale non vede mai la fine.
                libc::close(leader_fd);
                Ok(())
            });
        }

        let child = command.spawn().map_err(PtyError::NotStarted)?;
        // Il capo del figlio si chiude qui, nel padre. Se restasse aperto,
        // leggere il terminale non finirebbe mai: il sistema operativo aspetta
        // che l'ultimo scrittore se ne vada.
        drop(follower);

        Ok(Pty {
            leader: Mutex::new(File::from(leader)),
            child: Mutex::new(child),
            device: follower_name.to_string_lossy().into_owned(),
        })
    }

    /// The terminal device the program inside is attached to.
    ///
    /// Whoever wants to reach that program keys on this and not on the caller's
    /// own terminal: the two are different, and only this one is what the
    /// program reports about itself.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The same device under the short name `ps` uses: `/dev/ttys004` is
    /// `ttys004`. It is the key of the letterbox, the count and the mandate.
    pub fn tty(&self) -> &str {
        self.device.strip_prefix("/dev/").unwrap_or(&self.device)
    }

    /// Un secondo capo aperto sullo stesso terminale, per il filo che legge.
    ///
    /// Leggere e scrivere sullo stesso `File` sotto lo stesso lucchetto
    /// bloccherebbe chi scrive per tutto il tempo in cui chi legge aspetta —
    /// cioè quasi sempre, perché un terminale sta fermo quasi sempre.
    pub fn reader(&self) -> Result<impl Read + Send, PtyError> {
        locked(&self.leader).try_clone().map_err(PtyError::Broken)
    }

    /// Scrive sull'ingresso del terminale, come se qualcuno avesse digitato.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        let mut leader = locked(&self.leader);
        leader.write_all(bytes).map_err(PtyError::Broken)?;
        leader.flush().map_err(PtyError::Broken)
    }

    /// Dice al terminale quanto è grande adesso.
    pub fn resize(&self, size: Size) -> Result<(), PtyError> {
        set_size(&*locked(&self.leader), size).map_err(PtyError::Broken)
    }

    /// Se il processo dentro il terminale è ancora vivo.
    pub fn alive(&self) -> bool {
        matches!(locked(&self.child).try_wait(), Ok(None))
    }

    /// Com'è finito il processo dentro, se è finito.
    ///
    /// **NON ASPETTA, E LA DIFFERENZA È UN BLOCCO.** La chiama il filo che
    /// drena, appena l'uscita finisce; un'attesa vera terrebbe il lucchetto del
    /// figlio per tutto il tempo, e chi nel frattempo chiude il terminale
    /// resterebbe fermo sulla porta di un lucchetto che non si apre mai.
    ///
    /// Un `try_wait` fallito torna `None` come un processo ancora vivo: in tutti
    /// e due i casi la risposta onesta è «non lo so», ed è quella che il
    /// chiamante trasforma in [`crate::Ending::StillRunning`].
    pub fn finished(&self) -> Option<crate::Ending> {
        match locked(&self.child).try_wait() {
            Ok(Some(status)) => Some(match status.code() {
                Some(code) => crate::Ending::Exited(code),
                None => crate::Ending::Killed,
            }),
            _ => None,
        }
    }

    /// L'identificativo di processo di ciò che gira dentro.
    pub fn process_id(&self) -> u32 {
        locked(&self.child).id()
    }

    /// Chiude il terminale e aspetta che il processo sia davvero finito.
    ///
    /// **SI ASPETTA, E NON È PEDANTERIA.** Un `kill` senza `wait` lascia un
    /// processo zombie per ogni terminale chiuso; su una sessione lunga
    /// diventano centinaia, e il guasto si vede in un posto che non c'entra.
    pub fn close(&self) -> Result<(), PtyError> {
        let mut child = locked(&self.child);
        // Un figlio già morto dà «nessun processo»: non è un guasto, è la
        // condizione normale di chi chiude un terminale dopo aver scritto
        // `exit`.
        let _ = child.kill();
        child.wait().map_err(PtyError::Broken)?;
        Ok(())
    }
}

fn open_leader() -> Result<OwnedFd, PtyError> {
    let raw = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if raw < 0 {
        // Blamed where the error is born, not where it is printed: a perimeter
        // denies this call, and the four words it answers with read as a defect
        // of whatever crate the failing test happens to sit in.
        return Err(PtyError::NotOpened(crate::scratch::blamed(
            io::Error::last_os_error(),
        )));
    }
    let leader = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::grantpt(leader.as_raw_fd()) } != 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    if unsafe { libc::unlockpt(leader.as_raw_fd()) } != 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(leader)
}

fn follower_name(leader: &OwnedFd) -> Result<CString, PtyError> {
    let _guard = locked(&PTSNAME_LOCK);
    let raw = unsafe { libc::ptsname(leader.as_raw_fd()) };
    if raw.is_null() {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(unsafe { CStr::from_ptr(raw) }.to_owned())
}

fn open_follower(name: &CString) -> Result<OwnedFd, PtyError> {
    let raw = unsafe { libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
    if raw < 0 {
        return Err(PtyError::FollowerNotReady(io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

fn set_size(leader: &impl AsRawFd, size: Size) -> io::Result<()> {
    let measured = libc::winsize {
        ws_row: size.rows,
        ws_col: size.columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let done = unsafe { libc::ioctl(leader.as_raw_fd(), libc::TIOCSWINSZ as _, &measured) };
    if done < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A terminal whose leader is the null device and whose child is a shell:
    /// enough to drive the locks, without a pseudo-terminal a perimeter denies.
    fn stub(exit_code: i32) -> Pty {
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("exit {exit_code}"))
            .spawn()
            .expect("a shell");
        let sink = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .expect("the null device");
        Pty {
            leader: Mutex::new(sink),
            child: Mutex::new(child),
            device: "/dev/null".to_owned(),
        }
    }

    /// The thread draining a terminal must still learn how it ended after some
    /// other thread died holding its locks, or the pane shows it alive forever.
    #[test]
    fn a_terminal_still_answers_after_a_thread_died_holding_its_locks() {
        let pty = Arc::new(stub(7));
        let poisoning = Arc::clone(&pty);
        let died = std::thread::spawn(move || {
            let _leader = poisoning.leader.lock().expect("still clean");
            let _child = poisoning.child.lock().expect("still clean");
            panic!("died holding the terminal");
        })
        .join();
        assert!(died.is_err(), "the thread has to die holding both locks");
        assert!(pty.leader.is_poisoned() && pty.child.is_poisoned());

        pty.write(b"typed").expect("the null device takes anything");
        assert!(pty.resize(Size::default()).is_err(), "the null device has no size");
        assert!(pty.process_id() > 0);
        let until = Instant::now() + Duration::from_secs(5);
        let ending = loop {
            if let Some(ending) = pty.finished() {
                break ending;
            }
            assert!(Instant::now() < until, "the shell never reported its exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(ending, crate::Ending::Exited(7));
        assert!(!pty.alive());
        pty.close().expect("closing a child already gone");
    }
}
