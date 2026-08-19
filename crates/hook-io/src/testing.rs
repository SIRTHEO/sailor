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

/// La radice delle prove di **questo** processo.
///
/// Non passa da `std::env::temp_dir()` di proposito: quella legge `TMPDIR`, che
/// alcuni casi spostano dentro la propria casa isolata. Chi la leggesse mentre è
/// spostata pianterebbe la sua cartella dentro quella di un altro caso, che poco
/// dopo la cancella. `/tmp` esiste sempre e non si muove sotto i piedi.
pub fn test_root() -> PathBuf {
    // Il pid rende le radici uniche, quindi nessuno le riusa più: senza questa
    // raccolta si accumulano per sempre — 46 cartelle in mezz'ora di lavoro,
    // misurate il 19/08/2026 mentre si correggeva proprio questo.
    SWEEP.call_once(sweep_stale_roots);
    Path::new("/tmp").join(format!("{PREFIX}{}", std::process::id()))
}

/// Butta le radici lasciate dai processi finiti. Un guasto qui non è una ragione
/// per far cadere una prova: è pulizia, non isolamento.
fn sweep_stale_roots() {
    let now = SystemTime::now();
    // Anche `TMPDIR`: fino al 19/08/2026 le radici nascevano lì, con un nome
    // fisso per caso, e ne sono rimaste un centinaio che nessuno riusa più.
    let mut roots = vec![PathBuf::from("/tmp")];
    let tmp = std::env::temp_dir();
    if tmp != roots[0] {
        roots.push(tmp);
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
        assert!(root.starts_with("/tmp"), "{root:?}");
    }

    /// La raccolta guarda l'età, non il nome: una radice fresca resta anche se
    /// non è la nostra — potrebbe essere di una batteria che sta girando ora.
    #[test]
    fn the_sweep_only_takes_the_old_ones() {
        let pid = std::process::id();
        let old = Path::new("/tmp").join(format!("{PREFIX}{pid}-stantia"));
        let fresh = Path::new("/tmp").join(format!("{PREFIX}{pid}-fresca"));
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
