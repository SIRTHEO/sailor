//! Il gancio che porta le regole di repo dentro le copie di lavoro, da solo.
//!
//! Il giudizio sta in `guards::link_worktree_rules`; qui c'è ciò che tocca il
//! mondo: il payload su stdin, l'esistenza dello script, il processo figlio col
//! suo tempo massimo, e l'avviso su stderr.
//!
//! PERCHÉ ESISTE. `.claude/` e `CLAUDE.md` sono ignorati da git nei quattro
//! repo, quindi una copia di lavoro nasce senza regole e senza comandi, e
//! dall'interno non si vede: il contesto non elenca ciò che manca. La cura —
//! `scripts/link-worktree-rules.sh` — esisteva già, ma andava rilanciata a mano:
//! il 13/08/2026 le copie scoperte erano 0 su 7, il 14/08 erano **31 su 36**.
//! Questo gancio la lancia al posto nostro.
//!
//! NON DUPLICA LA LOGICA DEL COLLEGAMENTO. Il come sta tutto nello script bash,
//! che resta invocabile a mano; qui c'è solo il quando, e il conto di ciò che è
//! successo.
//!
//! FALLISCE APERTO E MUTO, sempre. Sta anche su `PostToolUse`, quindi un'uscita
//! diversa da zero sporcherebbe ogni comando della sessione; e non difende
//! niente, fa manutenzione. Se lo script manca, non parte o non risponde in
//! tempo, il gancio tace ed esce 0.
//!
//! I VENTI SECONDI VALGONO ANCHE COL NIPOTE ATTACCATO ALLA PIPE, e non era
//! ovvio: `subprocess.run` col tempo massimo uccide il figlio e poi richiama
//! `communicate()` senza limite, quindi sembrava che uno script che lascia
//! dietro un processo in secondo piano potesse tenere fermo il gancio ben oltre
//! la promessa. Misurato il 17/08/2026 con uno script che fa `sleep 45 &` e poi
//! `exec sleep 30`: **20,0 s da entrambe le parti**, uscita 0 e silenzio. Qui la
//! stessa cifra si ottiene non aspettando i fili che leggono le pipe dopo aver
//! ucciso il figlio.
//!
//! NESSUN REGISTRO, ed è una scelta di equivalenza, non una dimenticanza:
//! l'originale non ne scrive nessuno. Aggiungerlo qui vorrebbe dire fare al
//! porto una promessa che l'oracolo non fa, e il confronto vedrebbe una riga in
//! più da un lato solo. Se lo si vuole, si aggiunge ai due insieme.

use guards::link_worktree_rules::{linked_count, notice, wants_run};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Sopra questo tempo si rinuncia: il gancio non vale un comando lento. Lo
/// script fa quattro `git worktree list` e uno stat per voce, quindi sta molto
/// sotto.
const TIMEOUT: Duration = Duration::from_secs(20);

/// La stessa `HOME` che vede l'originale.
///
/// `Path.home()` di Python legge `HOME` dall'ambiente e ricade sul database
/// degli utenti solo se manca; qui una `HOME` assente dà la stringa vuota, cioè
/// uno script che non esiste e un gancio che tace. È la stessa scelta degli
/// altri porti, ed è il verso giusto in cui sbagliare per un gancio che fa
/// manutenzione.
fn script() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".claude/scripts/link-worktree-rules.sh")
}

/// Il testo che `subprocess.run(..., text=True)` consegnerebbe al Python.
///
/// DUE PASSAGGI, ed entrambi si vedono. Prima la decodifica UTF-8 **stretta**:
/// un byte storto in uscita è una `UnicodeDecodeError`, che il gancio cattura e
/// che quindi lo fa tacere — quindi qui vale `None`, non una sostituzione con il
/// carattere di rimpiazzo. Poi la traduzione dei fine riga: `\r\n` e `\r`
/// diventano `\n` prima che qualcuno conti le righe.
fn python_text(raw: Vec<u8>) -> Option<String> {
    let testo = String::from_utf8(raw).ok()?;
    Some(testo.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Lancia la cura e restituisce la sua uscita, o `None` se non è stato possibile.
///
/// `None` copre i tre modi di fallire dell'originale, che là sono tre eccezioni
/// e qui una risposta sola: bash che non parte, il tempo massimo scaduto, e
/// l'uscita non decodificabile. In tutti e tre il gancio tace.
///
/// LO STDERR DEL FIGLIO SI CATTURA E SI BUTTA, come fa `capture_output=True`.
/// Lasciarlo ereditare lo manderebbe sullo stderr del gancio, cioè sotto gli
/// occhi del modello: sarebbe un porto che parla dove l'originale sta zitto.
///
/// Le due pipe si svuotano da due fili mentre il figlio scrive: con trentasei
/// copie di lavoro l'uscita supera il buffer del sistema, e chi aspetta senza
/// leggere aspetta per sempre.
fn link(script: &Path) -> Option<String> {
    link_within(script, TIMEOUT)
}

/// Il corpo di [`link`], col tempo massimo esplicito.
///
/// È separato solo perché una prova possa chiedere un'attesa di un decimo di
/// secondo invece di venti: senza, provare che il figlio viene davvero ucciso
/// costerebbe venti secondi a ogni `cargo test`, e quindi non lo proverebbe
/// nessuno.
fn link_within(script: &Path, timeout: Duration) -> Option<String> {
    let mut child = Command::new("bash")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let filo_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let filo_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });

    let scadenza = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= scadenza {
                    // `subprocess.run` col tempo massimo uccide il figlio e poi
                    // solleva: qui si uccide e si risponde «niente».
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
    // Il codice di uscita dello script non si guarda, come nell'originale: ciò
    // che conta è cosa ha stampato.
    let raw = filo_out.join().ok()?;
    let _ = filo_err.join();
    python_text(raw)
}

pub fn run() -> i32 {
    let mut grezzo = String::new();
    if std::io::stdin().read_to_string(&mut grezzo).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&grezzo) else {
        return 0;
    };
    // `payload.get(...)` su un JSON che non è un oggetto è un `AttributeError`
    // nell'originale, cioè un'uscita 0 muta: qui è lo stesso ramo.
    if !payload.is_object() {
        return 0;
    }
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let script = script();
    if !wants_run(event, &payload) || !script.exists() {
        return 0;
    }

    let Some(uscita) = link(&script) else {
        return 0;
    };
    let quante = linked_count(&uscita);
    if quante == 0 {
        return 0;
    }

    // Si parla solo quando si è fatto qualcosa, e si dice quanto. Su **stderr**,
    // come l'originale: è una nota di manutenzione, non una decisione.
    eprintln!("{}", notice(quante, &script.display().to_string()));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;
    use std::fs;

    /// Uno script finto dentro la casa isolata: stampa ciò che gli si dice e non
    /// tocca nient'altro. Nessuna prova qui dentro deve poter vedere i worktree
    /// veri, e il modo per garantirlo è che lo script che gira non li nomini.
    fn falso_script(casa: &HomeIsolata, corpo: &str) -> PathBuf {
        let dir = casa.dir.join(".claude").join("scripts");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("link-worktree-rules.sh");
        fs::write(&p, corpo).unwrap();
        p
    }

    #[test]
    fn senza_lo_script_non_si_lancia_niente() {
        let _casa = HomeIsolata::nuova("link-worktree-senza-script");
        assert!(!script().exists());
        // `link` non viene nemmeno chiamata: qui si prova che il percorso segue
        // la HOME, che è la sola cosa che tiene le prove lontane dal vero.
        assert!(script().starts_with(std::env::var("HOME").unwrap()));
    }

    #[test]
    fn si_conta_solo_cio_che_lo_script_dichiara_collegato() {
        let casa = HomeIsolata::nuova("link-worktree-conta");
        let p = falso_script(
            &casa,
            "echo '  ok        a'\necho '  synced    b : .claude/rules (20 rules)'\n\
             echo '  linked    c : CLAUDE.md'\necho '  FOUND     d : x'\n",
        );
        let uscita = link(&p).unwrap();
        assert_eq!(linked_count(&uscita), 2);
        assert_eq!(
            notice(2, &p.display().to_string()),
            format!(
                "Linked repo config into 2 worktree(s) that had none. \
                 Inspect with: bash {} --conta",
                p.display()
            )
        );
    }

    /// Lo stderr del figlio non deve arrivare a chi legge il gancio, e un codice
    /// di uscita diverso da zero non annulla ciò che è stato stampato.
    #[test]
    fn lo_stderr_del_figlio_si_butta_e_l_uscita_non_conta() {
        let casa = HomeIsolata::nuova("link-worktree-stderr");
        let p = falso_script(
            &casa,
            "echo 'rumore' >&2\necho '  synced    b : .claude/rules (20 rules)'\nexit 3\n",
        );
        assert_eq!(linked_count(&link(&p).unwrap()), 1);
    }

    /// Un'uscita che non è UTF-8 fa sollevare il Python, che tace: qui `None`.
    #[test]
    fn un_uscita_non_decodificabile_fa_tacere_il_gancio() {
        let casa = HomeIsolata::nuova("link-worktree-bytes");
        let p = falso_script(&casa, "printf '  collegato \\xff\\n'\n");
        assert!(link(&p).is_none());
    }

    /// I fine riga si traducono prima di contare, come in modo testo.
    #[test]
    fn i_fine_riga_si_traducono_come_in_modo_testo() {
        assert_eq!(
            python_text(b"  collegato a\r\n  collegato b\r".to_vec()).unwrap(),
            "  collegato a\n  collegato b\n"
        );
    }

    /// Uno script che non finisce viene ucciso, e il gancio tace.
    ///
    /// Il Python solleva `TimeoutExpired` **dopo** aver ucciso il figlio, e ciò
    /// che aveva già stampato non lo guarda nessuno: la riga «collegato» qui
    /// c'è, e il risultato deve essere comunque niente.
    #[test]
    fn uno_script_che_non_risponde_viene_ucciso_e_non_si_conta() {
        let casa = HomeIsolata::nuova("link-worktree-timeout");
        // `exec`, non `sleep` e basta: senza, il nipote sopravvive al figlio
        // ucciso e resta appeso alla pipe. Il Python, che dopo aver ucciso
        // richiama `communicate()`, resterebbe fermo ad aspettarlo.
        let p = falso_script(&casa, "echo '  collegato a :'\nexec sleep 120\n");
        let inizio = Instant::now();
        assert!(link_within(&p, Duration::from_millis(150)).is_none());
        // Ha davvero smesso di aspettare, invece di restare appeso al figlio.
        assert!(inizio.elapsed() < Duration::from_secs(5));
    }
}
