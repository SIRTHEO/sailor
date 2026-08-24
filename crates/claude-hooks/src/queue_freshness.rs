//! Il braccio della freschezza della coda: dove stanno le voci, chi le legge,
//! chi ci scrive dentro il rilievo.
//!
//! Il giudizio — cosa vuol dire «la stessa cosa», quando una voce è troppo
//! vecchia, com'è fatto il blocco — sta in `guards::queue_overlap`, che non
//! tocca il disco. Qui c'è solo il mondo: la cartella, la lettura, la
//! riscrittura, il conteggio finale.
//!
//! L'ancoraggio al codice **non si rifà**: le voci di coda sono entrate fra le
//! cartelle di `memory-anchors`, quindi `percorso#simbolo@impronta` nel loro
//! frontmatter è già timbrato, verificato e riallineato dallo strumento che
//! esisteva. Qui si legge il verdetto e lo si porta dentro la voce.
//!
//! Uso:
//!     claude-hooks queue-freshness            racconta e non scrive niente
//!     claude-hooks queue-freshness --mark     scrive il rilievo nelle voci
//!     claude-hooks queue-freshness --pairs    l'elenco esteso delle coppie
//!
//! NON AGGIUNGE UN BYTE AL PROLOGO, di proposito. Il difetto di questa casa è
//! che ogni meccanismo nuovo scrive una riga in più che qualcuno paga a ogni
//! sessione: qui il rilievo vive **dentro** la voce, dove lo legge solo chi
//! quella voce la apre davvero. Fuori esce una riga sola, per chi lancia il
//! comando.
//!
//! Uscita: 0 sempre. Una coppia sospetta non è un guasto, è una cosa da
//! guardare — e un comando che esce rosso su una condizione normale si smette
//! di lanciare.

use guards::memory_anchor::{anchors_in, judge, Anchor};
use guards::queue_overlap::{
    block_body, is_closed, partners_of, read_voice, stale_days, suspect_pairs, with_block, Voice,
};
use guards::stale_facts::Date;
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// La cartella della coda.
///
/// `QUEUE_DIR` la sostituisce: è la valvola che rende provabile lo strumento
/// senza toccare le voci vere, e la stessa che usa `memory-anchors` con
/// `MEMORY_ANCHOR_DIR`.
pub(crate) fn queue_dir() -> PathBuf {
    std::env::var_os("QUEUE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude/state/plancia/segnalazioni"))
}

/// I file che sono voci.
///
/// `README.md` è il formato, non un'affermazione: nomina ogni campo e ogni
/// stato della coda, quindi contarlo fra le voci lo renderebbe la compagna di
/// tutte — e i suoi soggetti falserebbero la frequenza che decide cosa è raro.
fn voice_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| p.file_name().is_some_and(|n| n != "README.md"))
        .collect();
    out.sort();
    out
}

fn label_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// La data di oggi dall'orologio locale, come la legge `stale-facts`.
fn today() -> Date {
    let stamp = hook_io::local_time::now_local_iso8601();
    let n = |a: usize, b: usize| stamp.get(a..b).and_then(|s| s.parse::<i64>().ok());
    match (n(0, 4), n(5, 7), n(8, 10)) {
        (Some(y), Some(m), Some(d)) => Date::new(y, m, d),
        _ => None,
    }
    .unwrap_or(Date {
        year: 2026,
        month: 1,
        day: 1,
    })
}

/// Gli ancoraggi di una voce che non descrivono più il codice di adesso.
///
/// Si risolve il percorso con lo stesso risolutore di `memory-anchors`: due
/// idee diverse di dove viva un file darebbero due verdetti diversi sullo
/// stesso ancoraggio.
fn drifted_anchors(text: &str) -> Vec<String> {
    anchors_in(text)
        .into_iter()
        .filter_map(|anchor: Anchor| {
            let source = crate::memory_anchors::resolve_for_queue(&anchor.path)
                .and_then(|p| std::fs::read_to_string(p).ok());
            judge(&anchor, source.as_deref())
                .alarming()
                .then(|| anchor.render())
        })
        .collect()
}

/// Una voce letta dal disco, col suo testo.
struct Entry {
    path: PathBuf,
    text: String,
    voice: Voice,
}

fn read_all(dir: &Path) -> Vec<Entry> {
    voice_files(dir)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let voice = read_voice(&label_of(&path), &text);
            Some(Entry { path, text, voice })
        })
        .collect()
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mark = args.iter().any(|a| a == "--mark");
    let show_pairs = args.iter().any(|a| a == "--pairs");

    let dir = queue_dir();
    let entries = read_all(&dir);
    if entries.is_empty() {
        println!("Nessuna voce in {}.", dir.display());
        return 0;
    }

    let voices: Vec<Voice> = entries.iter().map(|e| e.voice.clone()).collect();
    let pairs = suspect_pairs(&voices);
    let now = today();

    let mut stale_count = 0;
    let mut drift_count = 0;
    let mut written = 0;
    let mut refused: Vec<String> = Vec::new();
    for entry in &entries {
        let stale = stale_days(&entry.voice, now).map(|d| (d, entry.voice.last_touched));
        if stale.is_some() {
            stale_count += 1;
        }
        // Una voce chiusa non si marca: è storia, e un avviso sopra la storia
        // sposta l'occhio da quelle che invece guidano ancora il lavoro.
        let partners: Vec<(&str, &[String])> = if is_closed(&entry.voice.state) {
            Vec::new()
        } else {
            partners_of(&entry.voice.name, &pairs)
        };
        let drifted = drifted_anchors(&entry.text);
        drift_count += drifted.len();
        if !mark {
            continue;
        }
        let body = block_body(stale, &partners, &drifted);
        let updated = with_block(&entry.text, body.as_deref());
        if updated == entry.text {
            continue;
        }
        match std::fs::write(&entry.path, &updated) {
            Ok(()) => written += 1,
            // UNA SCRITTURA NEGATA SI DICE. Dentro il perimetro
            // `~/.claude/state` è in sola lettura, e la passata rispondeva
            // «rilievo scritto dentro 0 voci» — cioè la stessa riga del caso in
            // cui non c'era niente da scrivere. È il falso verde peggiore:
            // chi lo legge crede che la coda sia marcata e non lo è.
            Err(e) => refused.push(format!("{}: {e}", label_of(&entry.path))),
        }
    }

    if show_pairs {
        for p in &pairs {
            println!("{} ↔ {}", p.a, p.b);
            println!("   in comune: {}", p.shared.join(", "));
        }
    }

    // La riga sola promessa dalla premessa del modulo: quante coppie, quante
    // voci vecchie, quanti ancoraggi in deriva. Non una segnalazione nuova.
    println!(
        "{} voci · {} coppie che parlano della stessa cosa · {} da riverificare (oltre {} giorni) \
         · {} ancoraggi in deriva",
        entries.len(),
        pairs.len(),
        stale_count,
        guards::queue_overlap::STALE_DAYS,
        drift_count
    );
    if !mark {
        println!("Nessun file toccato: `--mark` scrive il rilievo dentro le voci.");
        return 0;
    }
    println!("Rilievo scritto dentro {written} voci.");
    if let Some(first) = refused.first() {
        println!(
            "SCRITTURA NEGATA su {} voci — la coda NON è marcata. La prima: {first}",
            refused.len()
        );
        // Esce 1 solo qui: «non ho potuto» è l'unica condizione in cui chi
        // lancia deve accorgersene senza leggere l'uscita a occhio.
        return 1;
    }
    0
}

/// Solo per le prove: le coppie calcolate su una cartella data.
#[cfg(test)]
fn pairs_in(dir: &Path) -> Vec<guards::queue_overlap::Pair> {
    let voices: Vec<Voice> = read_all(dir).into_iter().map(|e| e.voice).collect();
    suspect_pairs(&voices)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una cartella usa-e-getta con dentro le voci che le si danno.
    fn scratch(name: &str, voices: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("queue-freshness-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in voices {
            std::fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn voice_text(state: &str, body: &str) -> String {
        format!("---\nsessione: x\nquando: 2026-08-21 09:00\nstato: {state}\n---\n\n# titolo\n\n{body}\n")
    }

    #[test]
    fn the_readme_is_the_format_and_not_a_voice() {
        // Il formato nomina ogni campo e ogni stato: contarlo fra le voci lo
        // renderebbe la compagna di tutte.
        let dir = scratch(
            "readme",
            &[
                ("README.md", "il formato: `stato: aperta`, `per:`"),
                ("a.md", &voice_text("aperta", "parla di `alfa.rs`")),
            ],
        );
        let files = voice_files(&dir);
        assert_eq!(files.len(), 1);
        assert_eq!(label_of(&files[0]), "a.md");
    }

    #[test]
    fn two_voices_on_the_same_subjects_come_out_and_a_lonely_one_does_not() {
        let dir = scratch(
            "pairs",
            &[
                (
                    "a.md",
                    &voice_text("aperta", "tetto con `ulimit -v`, `setrlimit` risponde male"),
                ),
                (
                    "b.md",
                    &voice_text("aperta", "`ulimit -v` non si imposta, `setrlimit` dà EINVAL"),
                ),
                ("sola.md", &voice_text("aperta", "`mdfind` e `spotlight.rs`")),
            ],
        );
        let pairs = pairs_in(&dir);
        assert_eq!(pairs.len(), 1);
        assert!(partners_of("sola.md", &pairs).is_empty());
    }

    #[test]
    fn marking_is_idempotent_on_disk() {
        // Due passate di seguito devono lasciare gli stessi byte: un blocco che
        // si accoda a sé stesso farebbe crescere la voce a ogni giro.
        let dir = scratch(
            "idempotente",
            &[
                (
                    "a.md",
                    &voice_text("aperta", "tetto con `ulimit -v`, `setrlimit`"),
                ),
                (
                    "b.md",
                    &voice_text("aperta", "`ulimit -v` no, `setrlimit` EINVAL"),
                ),
            ],
        );
        let path = dir.join("a.md");
        let now = Date::new(2026, 8, 30).unwrap();

        let mark = |now: Date| {
            let entries = read_all(&dir);
            let voices: Vec<Voice> = entries.iter().map(|e| e.voice.clone()).collect();
            let pairs = suspect_pairs(&voices);
            for e in &entries {
                let stale = stale_days(&e.voice, now).map(|d| (d, e.voice.last_touched));
                let partners = partners_of(&e.voice.name, &pairs);
                let body = block_body(stale, &partners, &[]);
                std::fs::write(&e.path, with_block(&e.text, body.as_deref())).unwrap();
            }
        };

        mark(now);
        let once = std::fs::read_to_string(&path).unwrap();
        assert!(once.contains("DA RIVERIFICARE"));
        assert!(once.contains("b.md"));
        mark(now);
        let twice = std::fs::read_to_string(&path).unwrap();
        assert_eq!(once, twice);

        // E il frontmatter resta il primo blocco del file: lo legge il
        // selettore della coda, che senza smetterebbe di vedere `stato:`.
        assert!(twice.starts_with("---\nsessione: x\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
