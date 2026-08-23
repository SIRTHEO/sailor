//! La colla del canarino del formato transcript: trova il file vero, ne prende
//! un campione, e traduce il verdetto in un codice d'uscita.
//!
//! Il transcript si cerca **a runtime** sotto `~/.claude/projects/`: copiarne
//! uno nel repo farebbe un canarino imbalsamato, verde per sempre sullo schema
//! del giorno in cui è stato copiato — cioè l'esatto contrario del suo mestiere.
//!
//! Codici d'uscita: `0` vivo, `1` morto (una o più assunzioni cadute), `2` non
//! misurato (nessun transcript, o campione troppo povero). Il `2` non è un
//! verde: dice che la prova non c'è stata.

use guards::transcript_canary::{check, render, render_assumptions, Report, Verdict};
use std::path::{Path, PathBuf};

/// Quante righe di coda si guardano, se nessuno dice altro.
///
/// Quattrocento righe di una sessione viva portano ~120 turni `assistant`: il
/// campione più piccolo che contiene con certezza tutte le forme che i lettori
/// si aspettano. Di più costa tempo senza aggiungere assunzioni nuove.
const DEFAULT_SAMPLE: usize = 400;

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.iter().any(|a| a == "--assumptions") {
        print!("{}", render_assumptions());
        return 0;
    }
    let lines = flag(&args, "--lines")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SAMPLE);
    let chosen = match flag(&args, "--file").map(PathBuf::from) {
        Some(p) => report_for(&p, lines).map(|r| (p, r)),
        None => measure(lines),
    };
    let Some((path, report)) = chosen else {
        eprintln!(
            "transcript-canary: NON MISURATO — nessun transcript leggibile sotto {}",
            projects_dir().display()
        );
        return 2;
    };
    println!("transcript: {}", path.display());
    print!("{}", render(&report));
    match report.verdict() {
        Verdict::Alive => 0,
        Verdict::Dead => {
            eprintln!(
                "transcript-canary: MORTO su {} — le misure che leggono i transcript (costi, consegna, sessione lunga, staffetta) stanno già degradando in silenzio.",
                path.display()
            );
            1
        }
        Verdict::NotMeasured => 2,
    }
}

/// Il rapporto sul transcript indicato, per chi vuole giudicare da dentro una
/// prova invece che dal codice d'uscita.
pub fn report_for(path: &Path, lines: usize) -> Option<Report> {
    let sample = sample_lines(path, lines);
    if sample.is_empty() {
        return None;
    }
    Some(check(&sample.iter().map(String::as_str).collect::<Vec<_>>()))
}

/// Il valore di un'opzione `--nome valore`.
fn flag(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

fn projects_dir() -> PathBuf {
    home().join(".claude").join("projects")
}

/// Quanti transcript recenti si provano prima di arrendersi.
///
/// Il più fresco spesso è il file di un subagent nato da dieci secondi: dieci
/// record, nessuna delle forme che servono. Fermarsi lì darebbe «non misurato»
/// quasi ogni notte, cioè un canarino che non canta mai.
const MAX_CANDIDATES: usize = 20;

/// I transcript dal più fresco al più vecchio, subagent compresi: scrivono lo
/// stesso schema, e per un canarino conta la riga più fresca, non chi l'ha
/// scritta.
pub fn recent_transcripts() -> Vec<PathBuf> {
    let mut files = Vec::new();
    crate::costs::collect_jsonl(&projects_dir(), &mut files);
    files.sort_by_key(|p| std::cmp::Reverse(crate::costs::mtime_epoch(p)));
    files.truncate(MAX_CANDIDATES);
    files
}

/// Il transcript recente più fresco che sia **abbastanza ricco da giudicare**,
/// col suo rapporto.
///
/// Un file troppo povero non è una prova: si scende al successivo. Un file
/// ricco si restituisce anche quando il canarino è morto — anzi, soprattutto
/// allora. Se nessuno dei candidati è giudicabile si torna col più fresco
/// leggibile, così chi chiama sa dire su cosa non ha potuto misurare.
pub fn measure(wanted: usize) -> Option<(PathBuf, Report)> {
    let mut fallback = None;
    for path in recent_transcripts() {
        let Some(report) = report_for(&path, wanted) else {
            continue;
        };
        if report.verdict() != Verdict::NotMeasured {
            return Some((path, report));
        }
        if fallback.is_none() {
            fallback = Some((path, report));
        }
    }
    fallback
}

/// Le ultime `wanted` righe del file, senza quella che il taglio può aver
/// spezzato a metà.
fn sample_lines(path: &Path, wanted: usize) -> Vec<String> {
    let tail = crate::handoff::transcript_tail(&path.to_string_lossy());
    if tail.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = tail.lines().collect();
    // La coda parte da un `seek` cieco: su un file più grande della finestra la
    // prima riga comincia a metà record. Scartarla qui evita di contarla come
    // un JSON illeggibile, che sarebbe una morte falsa.
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > guards::handoff::TAIL_BYTES && !lines.is_empty() {
        lines.remove(0);
    }
    if lines.len() > wanted {
        lines = lines.split_off(lines.len() - wanted);
    }
    lines.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IL CANARINO VERO: le assunzioni provate sul transcript più recente di
    /// questa macchina, letto adesso.
    ///
    /// Non c'è nessun file di prova nel repo apposta: un campione congelato
    /// resterebbe verde anche il giorno in cui Anthropic cambia schema. Se non
    /// c'è nessun transcript, la prova non passa in silenzio — si ferma dicendo
    /// che non ha misurato niente, perché un verde senza campione è la bugia
    /// che questo canarino esiste per impedire.
    #[test]
    fn the_assumptions_hold_on_the_newest_real_transcript() {
        // `HOME` è una variabile di processo e i casi corrono in parallelo:
        // senza il lucchetto, un caso con la casa finta impostata farebbe
        // cercare i transcript in una cartella usa-e-getta vuota.
        let _lock = crate::test_home::real_home_guard();
        let Some((path, report)) = measure(DEFAULT_SAMPLE) else {
            panic!(
                "SALTATA (non verde): nessun transcript leggibile sotto {} — il canarino non ha misurato niente",
                projects_dir().display()
            );
        };
        let text = render(&report);
        match report.verdict() {
            Verdict::Alive => {}
            Verdict::NotMeasured => panic!(
                "SALTATA (non verde): campione troppo povero in {}\n{text}",
                path.display()
            ),
            Verdict::Dead => panic!(
                "IL CANARINO È MORTO su {}: lo schema dei transcript è cambiato e le misure di casa stanno già degradando in silenzio.\n{text}",
                path.display()
            ),
        }
    }

    #[test]
    fn a_mutated_transcript_kills_the_canary_and_names_the_field() {
        // Lo stesso file vero, con `message.content` da array a stringa: è la
        // prova che il canarino di sopra POTEVA venire rosso.
        let _lock = crate::test_home::real_home_guard();
        let Some((path, _)) = measure(DEFAULT_SAMPLE) else {
            panic!("SALTATA (non verde): nessun transcript da mutare");
        };
        let sample = sample_lines(&path, DEFAULT_SAMPLE);
        // La mutazione passa dal JSON e non da una sostituzione di testo: una
        // riga spezzata morirebbe come «non è JSON» e non proverebbe niente
        // sul campo che ci interessa.
        let mutated: Vec<String> = sample
            .iter()
            .map(|line| {
                let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) else {
                    return line.clone();
                };
                let is_assistant =
                    value.get("type").and_then(|v| v.as_str()) == Some("assistant");
                if let Some(content) =
                    value.get_mut("message").and_then(|m| m.get_mut("content"))
                {
                    if is_assistant && content.is_array() {
                        *content = serde_json::Value::String("un testo, non più blocchi".into());
                    }
                }
                value.to_string()
            })
            .collect();
        let report = check(&mutated.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        assert!(render(&report).contains("message.content"), "{}", render(&report));
    }

    #[test]
    fn the_sample_drops_the_line_the_seek_cut_in_half() {
        let dir = crate::test_home::test_root().join("transcript-canary-sample");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("short.jsonl");
        std::fs::write(&path, "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n").unwrap();
        // File sotto la finestra: nessun taglio, quindi nessuna riga da buttare.
        assert_eq!(sample_lines(&path, 10).len(), 2);
        assert_eq!(sample_lines(&path, 1), vec!["{\"type\":\"assistant\"}".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_not_measured_rather_than_green() {
        assert!(report_for(Path::new("/nowhere/does/this/exist.jsonl"), 10).is_none());
    }
}
