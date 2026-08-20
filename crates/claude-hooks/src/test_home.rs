//! Una `HOME` usa-e-getta per i casi che scrivono stato, e **un solo** lucchetto.
//!
//! PERCHÉ ESISTE. Diversi ganci scrivono sotto `$HOME/.claude/state`, e i test
//! girano col `HOME` vero: il 17/08/2026 quattro righe `sess=provarel` — fra cui
//! una «RIGENERATA» — sono finite nel registro di produzione della staffetta,
//! dove chi indaga un guasto le legge come fatti.
//!
//! PERCHÉ IL LUCCHETTO STA QUI E NON IN OGNI MODULO. `HOME` è una variabile del
//! processo, e i test Rust girano in parallelo dentro lo stesso processo: due
//! moduli con un mutex ciascuno non si serializzano fra loro. Misurato lo stesso
//! giorno, poche ore dopo la prima correzione: aggiungendo casi isolati in un
//! secondo modulo, altre due righe di prova sono comparse nel registro vero —
//! ogni modulo rispettava il proprio lucchetto, e i due non si conoscevano.
//! Un lucchetto per file dà l'aspetto dell'isolamento senza la sostanza.
//!
//! PERCHÉ LA RADICE PORTA IL PID. Il lucchetto vive dentro **un** processo, e
//! due `cargo test` insieme sono due processi: il 19/08/2026 due batterie
//! simultanee hanno prodotto 7 rossi su casi diversi, perché ognuna cancellava
//! le cartelle dell'altra (i nomi erano fissi). Ogni sessione che compila e
//! prova nello stesso repo ne apre una, quindi la corsa non è un caso di
//! laboratorio: è il motivo per cui la batteria «mentiva a caso».

pub use hook_io::testing::test_root;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static LUCCHETTO: Mutex<()> = Mutex::new(());

pub struct HomeIsolata {
    _lock: MutexGuard<'static, ()>,
    precedente: Option<String>,
    pub dir: PathBuf,
}

impl HomeIsolata {
    /// Una casa vuota sotto la cartella temporanea, con `state/` già dentro.
    ///
    /// Il nome distingue i casi fra loro: due che condividessero la cartella si
    /// leggerebbero i file a vicenda, e il secondo passerebbe grazie al primo.
    pub fn nuova(nome: &str) -> Self {
        // Se un caso è andato in panico col lucchetto in mano, l'avvelenamento
        // non è una ragione per far fallire tutti quelli dopo: la casa viene
        // comunque ricreata da zero.
        let lock = LUCCHETTO.lock().unwrap_or_else(|e| e.into_inner());
        let precedente = std::env::var("HOME").ok();
        let dir = test_root().join(nome);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(dir.join(".claude").join("state"));
        std::env::set_var("HOME", &dir);
        // L'attesa che il successore raccolga il mandato vale venticinque
        // secondi in produzione: qui a zero, o ogni caso che rigenera li
        // pagherebbe. Chi prova proprio quell'attesa se la rialza da sé.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        Self { _lock: lock, precedente, dir }
    }

    pub fn stato(&self) -> PathBuf {
        self.dir.join(".claude").join("state")
    }
}

/// Il lucchetto senza sostituire `HOME`, per i casi che devono leggere quella vera.
///
/// PERCHÉ SERVE. Un caso che guarda `$HOME` **vera** — perché prova proprio le
/// radici che stanno fuori da `.claude` — non ha niente da isolare, ma corre lo
/// stesso rischio al contrario: mentre lui legge, un altro caso può avere la
/// casa finta impostata, e allora vede un albero che non è quello di nessuno.
/// Misurato il 20/08/2026: `cargo test -p claude-hooks` era rosso 2 volte su 8,
/// sempre sullo stesso caso, dopo che undici case isolate nuove hanno allargato
/// la finestra in cui `HOME` è sostituita. Prendere il lucchetto senza toccare
/// niente costa quanto un'attesa e chiude la corsa.
#[must_use = "tienila viva in un `let _lock = …`: rilasciata subito, non serve a niente"]
pub fn real_home_guard() -> MutexGuard<'static, ()> {
    LUCCHETTO.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for HomeIsolata {
    fn drop(&mut self) {
        std::env::remove_var("RELAY_PICKUP_TIMEOUT_SEC");
        // Chi prova la ripresa dichiara l'albero da qui: lasciarla impostata
        // farebbe leggere a un caso successivo l'albero di quello prima, ed è
        // lo stesso genere di perdita che ha portato il lucchetto in questo
        // file — una variabile di processo è globale, i test no.
        std::env::remove_var("ORCA_WORKTREE_ID");
        std::env::remove_var("ORCA_TAB_ID");
        match &self.precedente {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }
}
