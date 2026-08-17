//! Il gancio che avvisa una sessione ripartita da un riassunto.
//!
//! Il giudizio sta in `guards::restart`; qui c'è ciò che tocca il mondo: il JSON
//! di `SessionStart` su stdin, il transcript sul disco, il testo su stdout.
//!
//! FAIL-OPEN DICHIARATO. Qualunque cosa vada storta esce zero in silenzio: un
//! promemoria che si rompe non deve impedire a una sessione di partire.

use guards::restart::{count_lines, message, Restarts};
use std::io::{BufRead, BufReader, Read};

/// I due tetti, dall'ambiente come nell'originale.
///
/// I nomi restano quelli italiani che l'originale già espone: sono scritti nei
/// documenti e in `settings.json`, e rinominarli qui spezzerebbe i rimandi senza
/// migliorare niente. Il porto replica, non riforma.
fn caps() -> (u32, u32) {
    let leggi = |nome: &str, difetto: u32| {
        std::env::var(nome)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(difetto)
    };
    (
        leggi("RIPARTENZA_TETTO_COMPATT", 5),
        leggi("RIPARTENZA_TETTO_CHIAMATE", 3000),
    )
}

/// Conta scorrendo il file riga per riga.
///
/// Non si legge tutto in memoria e non si legge la coda: le ripartenze vanno
/// contate **dall'inizio**, e questi file arrivano a 245 MB. È l'unico gancio
/// della famiglia che deve attraversare l'intero transcript, e il filtro grezzo
/// sulla stringa prima del JSON è ciò che lo rende sostenibile.
pub fn count_whole_file(path: &str) -> Option<Restarts> {
    let file = std::fs::File::open(path).ok()?;
    let mut out = Restarts::default();
    for line in BufReader::new(file).lines() {
        // Una riga non-UTF8 non ferma il conteggio: l'originale apre con
        // `errors='ignore'` e prosegue.
        let Ok(line) = line else { continue };
        let parziale = count_lines([line.as_str()].into_iter());
        out.restarts += parziale.restarts;
        out.tool_calls += parziale.tool_calls;
    }
    Some(out)
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    // Solo dopo una compattazione. Una partenza normale deve restare muta: il
    // rumore a ogni avvio si smette di leggere dopo due giorni.
    if data.get("source").and_then(|v| v.as_str()) != Some("compact") {
        return 0;
    }
    let path = data
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if path.is_empty() || !std::path::Path::new(path).exists() {
        return 0;
    }
    let Some(r) = count_whole_file(path) else { return 0 };
    let (max_restarts, max_tool_calls) = caps();
    println!("{}", message(&r, max_restarts, max_tool_calls));
    0
}

/// L'aggancio dello strumento di equivalenza: conta un transcript e stampa i due
/// numeri, così il confronto può interrogare le due implementazioni sullo stesso
/// file senza passare per stdin e per il ramo `source == compact`.
pub fn count_probe(transcript: &str) -> i32 {
    let r = count_whole_file(transcript).unwrap_or_default();
    println!(
        "{}",
        serde_json::json!({"restarts": r.restarts, "tool_calls": r.tool_calls})
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_tetti_si_leggono_dall_ambiente_e_hanno_un_difetto() {
        let _home = crate::test_home::HomeIsolata::nuova("tetti-ripartenza");
        std::env::remove_var("RIPARTENZA_TETTO_COMPATT");
        std::env::remove_var("RIPARTENZA_TETTO_CHIAMATE");
        assert_eq!(caps(), (5, 3000));
        std::env::set_var("RIPARTENZA_TETTO_COMPATT", "2");
        assert_eq!(caps().0, 2);
        // Un valore illeggibile non è zero: sarebbe un tetto sempre superato.
        std::env::set_var("RIPARTENZA_TETTO_COMPATT", "molte");
        assert_eq!(caps().0, 5);
        std::env::remove_var("RIPARTENZA_TETTO_COMPATT");
    }

    #[test]
    fn una_riga_illeggibile_non_ferma_il_conteggio() {
        let home = crate::test_home::HomeIsolata::nuova("riga-illeggibile");
        let path = home.dir.join("t.jsonl");
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']); // non-UTF8: si salta
        bytes.extend_from_slice(
            br#"{"type":"user","message":{"content":"This session is being continued x"}}"#,
        );
        bytes.push(b'\n');
        std::fs::write(&path, bytes).unwrap();
        let r = count_whole_file(path.to_str().unwrap()).unwrap();
        assert_eq!(r.restarts, 1, "la riga rotta ha fermato il conteggio");
    }
}
