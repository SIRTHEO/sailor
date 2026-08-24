//! Il braccio del filtro: legge un'uscita da stdin, la accorcia, conserva
//! l'intero su disco e stampa quello che deve entrare in contesto.
//!
//! NON È UN GANCIO, e non per una svista. Il momento giusto — dopo l'esecuzione
//! di un comando, prima che l'uscita torni — non esiste fra i ganci di questa
//! versione (2.1.241): lo schema di uscita di `PostToolUse` porta solo
//! `additionalContext` e `classifierContext`, cioè si può **aggiungere**
//! contesto, mai sostituire il risultato; `updatedInput` è dichiarato
//! `PreToolUse only`. Finché quel punto non esiste, il filtro si invoca a mano
//! nel comando che si sta per lanciare:
//!
//!     cargo test > /tmp/prova.txt 2>&1; echo "uscita: $?"; \
//!       claude-hooks filter-output < /tmp/prova.txt
//!
//! Due note su quella riga, tutte e due imparate a spese nostre: l'uscita non
//! si incanala in una pipeline (il codice di uscita sarebbe quello dell'ultimo
//! comando, e `cargo test | tail` esce 0 a batteria rossa), e l'esito si stampa
//! a parte perché il filtro non lo conosce.
//!
//! L'intero non si perde mai: quando taglia, il testo completo finisce in
//! `~/.claude/state/uscite/` e l'intestazione ne dà il percorso.

use guards::output_filter::{self, Limits, DEFAULT};
use std::io::Read;

/// Quanti file tenere in `state/uscite/`. Un archivio che cresce senza limite è
/// un guasto noto di questa casa: 5 GB in sette minuti, il 12/08.
const KEEP: usize = 200;

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let limits = match parse_limits(&args) {
        Ok(l) => l,
        Err(message) => {
            eprintln!("filter-output: {message}");
            return 64;
        }
    };
    let archive_wanted = !args.iter().any(|a| a == "--no-archive");

    let mut input = String::new();
    if std::io::Read::by_ref(&mut std::io::stdin())
        .read_to_string(&mut input)
        .is_err()
    {
        // Fail-open: se non si riesce a leggere, non si inventa niente.
        return 0;
    }

    let filtered = output_filter::filter_with_exit(&input, &limits, exit_code(&args));
    let archive = if !filtered.trimmed() {
        None
    } else if let Some(existing) = given_archive(&args) {
        // Chi invoca ha già l'uscita intera su disco — è il caso del gancio,
        // che gliela passa da lì: una seconda copia sarebbe solo un file in più
        // da raccogliere.
        Some(existing)
    } else if archive_wanted {
        store(&input)
    } else {
        None
    };
    // `print!` e non `println!`: sotto il tetto il filtro deve restituire i
    // byte com'erano, e una riga vuota in più li cambia — misurato sul corpus,
    // 12 uscite su 400 finivano con un a capo e ne uscivano con due.
    print!("{}", filtered.render(archive.as_deref()));
    0
}

/// Il codice di uscita del comando, se chi invoca lo passa. È il fatto che il
/// filtro non può dedurre dal testo, e quando c'è decide da solo se l'uscita
/// va tenuta intera.
fn exit_code(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == "--exit-code")?;
    args.get(i + 1)?.parse().ok()
}

/// Il percorso dove l'uscita intera sta già, se chi invoca lo dichiara.
fn given_archive(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--archive")?;
    args.get(i + 1).cloned()
}

/// I tetti da riga di comando. Sconosciuto è un errore, non un valore
/// ignorato: un tetto scritto male che passa in silenzio taglierebbe con
/// numeri che chi ha scritto la riga non ha voluto.
fn parse_limits(args: &[String]) -> Result<Limits, String> {
    let mut limits = DEFAULT;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = || -> Result<usize, String> {
            let raw = args
                .get(i + 1)
                .ok_or_else(|| format!("{flag} vuole un numero"))?;
            raw.parse::<usize>()
                .map_err(|_| format!("{flag}: {raw:?} non è un numero"))
        };
        match flag {
            "--cap" => {
                limits.cap = value()?;
                i += 2;
            }
            "--error-cap" => {
                limits.error_cap = value()?;
                i += 2;
            }
            "--head" => {
                limits.head = value()?;
                i += 2;
            }
            "--tail" => {
                limits.tail = value()?;
                i += 2;
            }
            "--no-archive" => i += 1,
            // Letto da `given_archive()`: qui basta che ci sia un percorso.
            "--archive" => {
                args.get(i + 1)
                    .ok_or_else(|| "--archive vuole un percorso".to_string())?;
                i += 2;
            }
            // Letto da `exit_code()`: qui si controlla solo che sia un numero,
            // così una riga scritta male non passa in silenzio.
            "--exit-code" => {
                let raw = args
                    .get(i + 1)
                    .ok_or_else(|| "--exit-code vuole un numero".to_string())?;
                raw.parse::<i32>()
                    .map_err(|_| format!("--exit-code: {raw:?} non è un numero"))?;
                i += 2;
            }
            other => return Err(format!("opzione sconosciuta: {other}")),
        }
    }
    if limits.error_cap < limits.cap {
        return Err("--error-cap sotto --cap: un'uscita che fallisce verrebbe tagliata prima di una che riesce".into());
    }
    Ok(limits)
}

/// Conserva l'uscita intera e restituisce il percorso. Se non ci riesce, torna
/// `None` e l'intestazione lo dichiara: meglio dire «non conservata» che dare
/// un percorso che non apre niente.
fn store(input: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let dir = std::path::PathBuf::from(home)
        .join(".claude")
        .join("state")
        .join("uscite");
    std::fs::create_dir_all(&dir).ok()?;
    prune(&dir);
    let stamp = hook_io::local_time::now_local_iso8601().replace(':', "");
    let path = dir.join(format!("{stamp}-{}.txt", std::process::id()));
    std::fs::write(&path, input).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Tiene i `KEEP` più recenti e butta il resto.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((m.modified().ok()?, e.path()))
        })
        .collect();
    if files.len() <= KEEP {
        return;
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Senza opzioni valgono i tetti dichiarati nel giudizio, non copie locali.
    #[test]
    fn no_options_means_the_declared_limits() {
        let l = parse_limits(&[]).unwrap();
        assert_eq!(l.cap, DEFAULT.cap);
        assert_eq!(l.error_cap, DEFAULT.error_cap);
    }

    #[test]
    fn the_caps_can_be_moved_from_the_command_line() {
        let l = parse_limits(&args(&["--cap", "1000", "--error-cap", "2000"])).unwrap();
        assert_eq!(l.cap, 1000);
        assert_eq!(l.error_cap, 2000);
    }

    /// Il caso che rovescerebbe il senso del filtro: un'uscita che fallisce
    /// tagliata prima di una che riesce.
    #[test]
    fn an_error_cap_below_the_cap_is_refused() {
        let err = parse_limits(&args(&["--cap", "9000", "--error-cap", "10"])).unwrap_err();
        assert!(err.contains("--error-cap"), "{err}");
    }

    #[test]
    fn an_unknown_option_is_an_error_not_a_shrug() {
        assert!(parse_limits(&args(&["--taglia-tutto"])).is_err());
        assert!(parse_limits(&args(&["--cap"])).is_err());
        assert!(parse_limits(&args(&["--cap", "molto"])).is_err());
    }

    /// `--no-archive` è un'opzione vera, non un residuo: senza questo caso
    /// finirebbe nel ramo «sconosciuta».
    #[test]
    fn no_archive_is_accepted() {
        assert!(parse_limits(&args(&["--no-archive"])).is_ok());
    }

    /// L'archivio già esistente si legge dove sta, e senza percorso è un
    /// errore: l'intestazione citerebbe il nulla.
    #[test]
    fn a_given_archive_is_read_and_validated() {
        assert_eq!(
            given_archive(&args(&["--archive", "/tmp/uscita.txt"])),
            Some("/tmp/uscita.txt".to_string())
        );
        assert_eq!(given_archive(&args(&["--no-archive"])), None);
        assert!(parse_limits(&args(&["--archive", "/tmp/uscita.txt"])).is_ok());
        assert!(parse_limits(&args(&["--archive"])).is_err());
    }

    /// Il codice di uscita si legge dove sta, e uno scritto male si rifiuta
    /// invece di diventare «non lo so» — la differenza fra tagliare e non
    /// tagliare un fallimento.
    #[test]
    fn the_exit_code_is_read_and_validated() {
        assert_eq!(exit_code(&args(&["--exit-code", "101"])), Some(101));
        assert_eq!(exit_code(&args(&["--no-archive"])), None);
        assert!(parse_limits(&args(&["--exit-code", "101"])).is_ok());
        assert!(parse_limits(&args(&["--exit-code", "rosso"])).is_err());
        assert!(parse_limits(&args(&["--exit-code"])).is_err());
    }
}
