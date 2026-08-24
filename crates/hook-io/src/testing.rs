//! La radice sotto cui ogni caso di prova costruisce le sue cartelle.
//!
//! PERCHÉ ESISTE, E PERCHÉ NON È DIETRO `cfg(test)`. Le cartelle di prova
//! avevano nomi fissi, e il lucchetto che serializza i casi vive dentro **un**
//! processo: due `cargo test` insieme sono due processi, e ognuno cancellava le
//! cartelle dell'altro mentre le usava. Misurato il 19/08/2026: due batterie
//! simultanee, 7 rossi su casi che in seriale sono verdi — ed è la situazione
//! normale su questa macchina, dove più sessioni compilano e provano lo stesso
//! repo. Sta in `hook-io` perché serve a tutti i crate, e `cfg(test)` non
//! attraversa il confine fra crate: il modulo di prova di `guards` non vedrebbe
//! un `cfg(test)` di `claude-hooks`.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

const PREFIX: &str = "claude-hooks-prove-";

/// Oltre questa età una radice è di un processo finito: nessuna batteria dura
/// due ore, e il caso più lento di tutta la configurazione sta sotto il minuto.
const STALE: Duration = Duration::from_secs(2 * 60 * 60);

static SWEEP: Once = Once::new();

/// La sede sotto cui piantare le radici, decisa una volta per processo.
///
/// PERCHÉ NON È `/tmp` FISSO, che è quello che c'era fino al 24/08/2026. Il
/// perimetro delle scritture di una sessione nega `/tmp` e consente `TMPDIR`:
/// con la sede fissa **la batteria non gira affatto da dentro una sessione** —
/// 270 casi rossi su 490, di cui 267 col messaggio qui sotto, misurati il
/// 24/08/2026. Da lì segue il resto: nessuna sessione può rilasciare il binario,
/// perché `release-hooks.sh` pretende la batteria verde, e il rilascio si ferma
/// sempre — correttamente, e per la ragione sbagliata.
///
/// PERCHÉ NON BASTA `std::env::temp_dir()` NUDO, che è la ragione per cui la
/// sede era fissa: due casi (`handoff_threshold`, `long_session`) spostano
/// `TMPDIR` dentro la propria casa isolata mentre girano. Chi lo leggesse in
/// quel momento pianterebbe la propria cartella dentro quella di un altro caso,
/// che poco dopo la cancella. Due difese, e servono entrambe: il valore si
/// calcola **una volta sola** per processo, e se malgrado ciò arriva già
/// spostato — perché il primo caso a chiamare era proprio uno di quei due — si
/// **risale** oltre la radice di prova che se l'è tirato dentro.
fn base_tmp() -> PathBuf {
    static BASE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let tmp = std::env::temp_dir();
        for ancestor in tmp.ancestors() {
            let is_a_test_root = ancestor
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(PREFIX));
            if is_a_test_root {
                return ancestor.parent().unwrap_or(Path::new("/tmp")).to_path_buf();
            }
        }
        tmp
    })
    .clone()
}

/// La radice delle prove di **questo** processo.
pub fn test_root() -> PathBuf {
    // Il pid rende le radici uniche, quindi nessuno le riusa più: senza questa
    // raccolta si accumulano per sempre — 46 cartelle in mezz'ora di lavoro,
    // misurate il 19/08/2026 mentre si correggeva proprio questo.
    SWEEP.call_once(sweep_stale_roots);
    let root = base_tmp().join(format!("{PREFIX}{}", std::process::id()));
    if let Some(why) = writing_here_is_denied(&root) {
        panic!("{why}");
    }
    root
}

/// Il messaggio da dare a chi lancia la batteria dentro il perimetro, o `None`
/// se qui si può scrivere davvero.
///
/// PERCHÉ NON SI LASCIA SEMPLICEMENTE FALLIRE. Il perimetro delle scritture nega
/// `/tmp`, e una prova che non riesce a costruire la propria casa produce
/// asserzioni rosse **indistinguibili da un difetto vero**. Il 24/08/2026 è
/// costato quattro verifiche in un giorno, a quattro agenti diversi: «221 su 464
/// falliscono», «65 rosse in code_language» — tutte verdi da fuori. Il verso
/// pericoloso non è la perdita di tempo: è che un giorno saranno rosse davvero, e
/// chi le ha viste rosse per finta tre volte le archivierà come artefatto. Un
/// falso allarme ripetuto è un anestetico. Qui si dice **una cosa diversa da
/// «fallito»** — non misurato, e perché — invece di lasciar cadere venti
/// asserzioni a valle.
///
/// Il verdetto si calcola una volta e si ripete uguale: il controllo è una
/// scrittura vera, e rifarla a ogni caso costerebbe quanto la prova.
fn writing_here_is_denied(root: &Path) -> Option<String> {
    static VERDICT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VERDICT
        .get_or_init(|| {
            let probe = root.join(".probe");
            let done = std::fs::create_dir_all(root).and_then(|_| std::fs::write(&probe, b"x"));
            let _ = std::fs::remove_file(&probe);
            match done {
                Ok(()) => None,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Some(format!(
                    "PROVA NON ESEGUITA, non fallita: la batteria non può scrivere in {}.\n  \
                     È il perimetro delle scritture, non un difetto del codice — dentro il \
                     perimetro `/tmp` è negato, e ogni caso che costruisce la propria casa \
                     cadrebbe con asserzioni che sembrano vere.\n  \
                     Rilancia la batteria da fuori il perimetro: quel numero rosso non dice \
                     niente su questo codice.",
                    root.display()
                )),
                // Un guasto diverso resta un guasto: qui si nomina solo ciò che
                // si è misurato, o questa scorciatoia diventerebbe il posto in
                // cui i difetti veri si nascondono.
                Err(e) => Some(format!(
                    "la batteria non riesce a costruire la propria radice in {}: {e}",
                    root.display()
                )),
            }
        })
        .clone()
}

/// Il messaggio da dare a un caso che deve scrivere in una cartella **fuori**
/// dalla propria casa isolata, o `None` se lì si scrive davvero. Chi lo riceve
/// **stampa e torna indietro**, come per `ps_is_denied`.
///
/// Serve dove la cosa da provare è proprio la sede vera: le coppie congelate del
/// rilevatore di duplicati nascono in `~/.claude/state/`, che il perimetro nega,
/// e il caso che le legge cadeva con un `PermissionDenied` che non dice niente
/// sul suo codice.
pub fn writes_denied_in(dir: &Path) -> Option<String> {
    let probe = dir.join(".probe");
    let done = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&probe, b"x"));
    let _ = std::fs::remove_file(&probe);
    match done {
        Ok(()) => None,
        Err(e) => Some(format!(
            "PROVA NON ESEGUITA, non fallita: qui non si scrive ({e}) — {}.\n  \
             È il perimetro delle scritture, non un difetto del codice: questo caso \
             prova la sede vera, e la sede vera è fuori.\n  \
             Rilancia la batteria da fuori il perimetro: lì si misura.",
            dir.display()
        )),
    }
}

/// Il messaggio da dare a un caso che ha bisogno di `ps`, o `None` se `ps` parte
/// davvero. Chi lo riceve **stampa e torna indietro**, non asserisce.
///
/// PERCHÉ ESISTE, ED È LA GEMELLA DI `writing_here_is_denied`. Il perimetro di
/// una sessione nega anche `ps`, e chi sa se una sessione è viva passa di lì.
/// Cinque casi cadevano per questo il 24/08/2026, e nessuno dei cinque diceva
/// niente sul proprio codice: due chiedono a `ps` un pid, tre ci arrivano
/// attraverso il conteggio delle sessioni vive. Il terzo di quei tre è il più
/// istruttivo — non trovando nessuna sessione **apriva davvero una tab in
/// Orca**, cioè il banco di prova usciva dal banco. Fermarlo in testa lo
/// impedisce, e non è un effetto collaterale: è la ragione migliore per
/// fermarlo.
///
/// PERCHÉ NON UN `panic!` COME PER LE SCRITTURE. Là il messaggio serve a chi
/// legge un rosso; qui il rosso è ciò che va tolto, perché finché resta
/// `release-hooks.sh` non rilascia e ogni freno nuovo aspetta una persona. Il
/// prezzo è che una prova saltata assomiglia a una passata: lo paga
/// `release-hooks.sh`, che **conta le righe di questo messaggio e le stampa**
/// prima di sostituire il binario.
pub fn ps_is_denied() -> Option<String> {
    static VERDICT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VERDICT.get_or_init(ask_ps_once).clone()
}

fn ask_ps_once() -> Option<String> {
    match std::process::Command::new("ps").arg("-p0").output() {
        Ok(_) => None,
        Err(e) => Some(format!(
            "PROVA NON ESEGUITA, non fallita: `ps` non parte qui ({e}).\n  \
             È il perimetro delle scritture, non un difetto del codice — chi sa se una \
             sessione è viva chiede a `ps`, e senza risposta ogni ramo a valle dice \
             «non lo so» invece di «no».\n  \
             Rilancia la batteria da fuori il perimetro: questo caso lì si misura."
        )),
    }
}

/// Butta le radici lasciate dai processi finiti. Un guasto qui non è una ragione
/// per far cadere una prova: è pulizia, non isolamento.
fn sweep_stale_roots() {
    let now = SystemTime::now();
    // Anche `/tmp`: fino al 24/08/2026 le radici nascevano lì e non altrove, e
    // ne restano quelle dei giri lanciati da fuori il perimetro. Da dentro la
    // cancellazione fallisce, ed è previsto: qui un guasto non ferma nessuno.
    let mut roots = vec![base_tmp()];
    let legacy = PathBuf::from("/tmp");
    if legacy != roots[0] {
        roots.push(legacy);
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| now.duration_since(t).unwrap_or_default() > STALE)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// Una cartella vuota per un caso, sotto la radice del processo.
pub fn test_dir(name: &str) -> PathBuf {
    let dir = test_root().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La radice di due processi diversi non può essere la stessa cartella: è
    /// tutto ciò che separa due batterie simultanee.
    #[test]
    fn the_root_carries_the_pid() {
        let root = test_root();
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("{PREFIX}{}", std::process::id()));
        assert_eq!(root.parent().unwrap(), base_tmp(), "{root:?}");
    }

    /// La difesa che regge tutto il resto: se `TMPDIR` arriva già spostato
    /// dentro la casa isolata di un caso, la sede non è quella — si risale
    /// oltre la radice di prova, o la batteria si pianterebbe le cartelle
    /// dentro un caso che poco dopo le cancella.
    ///
    /// Non si prova su `base_tmp()`, che è memorizzata una volta per processo e
    /// quindi non sa più tornare indietro: si prova la risalita, che è la parte
    /// che decide.
    #[test]
    fn a_moved_tmpdir_does_not_drag_the_root_inside_a_case() {
        let moved = Path::new("/tmp/claude-501")
            .join(format!("{PREFIX}999"))
            .join("un-caso/tmp");
        let climbed = moved
            .ancestors()
            .find(|a| {
                a.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(PREFIX))
            })
            .and_then(Path::parent)
            .unwrap();
        assert_eq!(climbed, Path::new("/tmp/claude-501"));
    }

    /// La raccolta guarda l'età, non il nome: una radice fresca resta anche se
    /// non è la nostra — potrebbe essere di una batteria che sta girando ora.
    #[test]
    fn the_sweep_only_takes_the_old_ones() {
        let pid = std::process::id();
        let old = base_tmp().join(format!("{PREFIX}{pid}-stantia"));
        let fresh = base_tmp().join(format!("{PREFIX}{pid}-fresca"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        // Tre ore fa: `touch -t` evita di aggiungere una dipendenza solo per
        // spostare indietro una data.
        std::process::Command::new("touch")
            .arg("-t")
            .arg("202001010000")
            .arg(&old)
            .output()
            .unwrap();

        sweep_stale_roots();

        assert!(!old.exists(), "una radice di tre ore fa doveva sparire");
        assert!(fresh.exists(), "una radice appena creata non si tocca");
        let _ = std::fs::remove_dir_all(&fresh);
    }
}
