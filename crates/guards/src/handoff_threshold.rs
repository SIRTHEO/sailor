//! Quanto contesto resta, detto **finché** resta: cosa si decide.
//!
//! Porto del giudizio di `skills/hooks/handoff-threshold.py`. Qui non si legge
//! niente e non si scrive niente: entrano i token misurati e la finestra
//! dichiarata, escono la percentuale, il gradino raggiunto e il testo da dire.
//! La parte che tocca il mondo — stdin, la coda del transcript, il file di stato
//! in `TMPDIR` — sta in `claude-hooks/src/handoff_threshold.rs`.
//!
//! È IL TERZO IMPIANTO DI SOGLIE DELLA CONFIGURAZIONE, e non misura la stessa
//! cosa degli altri due. `handoff_required` e `handoff_on_stop` confrontano il
//! contesto col **budget di qualità** dedotto dal modello (Opus 5 → 500k) e
//! pretendono la consegna al 78% e al 90%. Questo confronta col numero che la
//! sessione dichiara in `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, cioè con la
//! **finestra tecnica** su cui scatta la compattazione automatica, e al 70% e
//! all'85% non pretende niente: constata. I due numeri sono diversi di proposito
//! — 500k contro 1M sulla stessa sessione — quindi le percentuali non sono
//! confrontabili fra un gancio e l'altro, ed è la cosa che chi legge dopo
//! scambierà per un errore.
//!
//! COSTATA, NON ORDINA. Il 12/08/2026 tre giri su tre hanno invocato una
//! competenza sapendo solo che esisteva: l'imperativo non serve a far partire
//! una competenza, e in cambio rende impossibile misurare se serviva.

use crate::handoff::{gruppi, round_half_to_even};

/// Finestra di ripiego quando nessuno la dichiara.
///
/// Non è un valore da tarare: la sessione lo dichiara in
/// `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, ed è il numero su cui la compattazione
/// scatta davvero. Con i 200k fissi, una sessione da 1M si sentiva dire «il 119%
/// della finestra» — un avviso che arriva quattro volte troppo presto viene
/// ignorato, e poi ignorato sempre.
pub const DEFAULT_WINDOW: u128 = 200_000;

/// Le percentuali a cui si parla, **una volta sola per gradino**.
///
/// L'ordine è quello dell'originale e conta: [`step_reached`] lo scorre
/// all'indietro, quindi il gradino alto vince su quello basso. Invertirlo
/// farebbe rispondere 70 a una sessione al 90%, e da lì il gradino 85 non
/// verrebbe mai annunciato — il file di stato direbbe già «70» e nient'altro.
pub const STEPS: [u64; 2] = [70, 85];

/// Il valore dichiarato in una variabile d'ambiente, se è un intero positivo.
///
/// `raw.strip().isdigit()` dell'originale, e non `parse()`: il segno meno non
/// passa il filtro, quindi `-5` non è «meno cinque», è «non dichiarato» e si
/// scende alla variabile dopo. Lo zero passa il filtro e viene fermato dal
/// `> 0`, con lo stesso esito.
///
/// RESIDUO DICHIARATO, e riguarda solo i non-ASCII. `str.isdigit()` di Python è
/// Unicode: `'٤'` lo passa e `int()` lo converte in 4, `'²'` lo passa e `int()`
/// **alza** — cioè fa morire il gancio in silenzio, senza scrivere e senza
/// stampare. Qui passano i soli ASCII, quindi entrambi ricadono sulla variabile
/// successiva. Riprodurre le due tabelle Unicode costa più di quanto valga per
/// un numero che arriva da una variabile di shell, ed è dichiarato qui invece di
/// essere scoperto dopo.
pub fn declared_window(raw: &str) -> Option<u128> {
    let t = raw.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Un numero più lungo di un `u128` è un intero grande per Python e resta
    // finito qui: satura invece di fallire, perché fallire lo farebbe scendere
    // alla variabile dopo mentre l'originale lo userebbe — e una finestra
    // enorme è una sessione che tace, non una che parla col valore storico.
    let value = t.parse::<u128>().unwrap_or(u128::MAX);
    if value > 0 {
        Some(value)
    } else {
        None
    }
}

/// La percentuale della finestra, arrotondata come `round()` di Python.
///
/// L'ORDINE DELLE OPERAZIONI È QUELLO DELL'ORIGINALE: prima il prodotto, poi la
/// divisione. `used / window * 100` darebbe gli stessi numeri quasi sempre e non
/// sempre — due arrotondamenti binari invece di uno — e la differenza compare
/// esattamente sul confine di un gradino, che è il solo punto dove cambia il
/// comportamento.
pub fn percent(used: u64, window: u128) -> u64 {
    round_half_to_even(used as f64 * 100.0 / window as f64)
}

/// Il gradino più alto già raggiunto, se ce n'è uno.
pub fn step_reached(percent: u64) -> Option<u64> {
    STEPS.iter().rev().copied().find(|s| percent >= *s)
}

/// Il gradino è già stato annunciato, secondo il contenuto del file di stato.
///
/// `int(f.read().strip() or 0) >= step` dell'originale: un file vuoto vale zero,
/// un contenuto illeggibile vale «non ancora detto». Il verso in cui sbaglia è
/// quello giusto per un gancio che non blocca niente — al massimo ripete un
/// avviso, invece di tacerlo per sempre.
pub fn already_said(text: &str, step: u64) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return 0 >= step; // `'' or 0` di Python
    }
    match t.parse::<i128>() {
        Ok(n) => n >= step as i128,
        Err(_) => {
            // GLI INTERI DI PYTHON NON HANNO UN TETTO, e un numero più lungo di
            // un `i128` non è «illeggibile»: è enorme, e un enorme positivo ha
            // già superato ogni gradino. Trovato dal confronto, non a mente —
            // con un `parse` secco il porto riparlava dove l'oracolo taceva, su
            // uno stato da ventun cifre.
            let (segno, cifre) = match t.strip_prefix('-') {
                Some(resto) => (-1, resto),
                None => (1, t.strip_prefix('+').unwrap_or(t)),
            };
            if cifre.is_empty() || !cifre.chars().all(|c| c.is_ascii_digit()) {
                // RESIDUO DICHIARATO, stesso confine di `declared_window`:
                // `int()` accetta anche le cifre Unicode e i trattini bassi fra
                // le cifre (`int('7_0')` è 70). Lì l'originale potrebbe dire
                // «già detto» dove qui si riparla — il verso in cui sbagliare
                // per un gancio che non blocca niente.
                return false;
            }
            segno > 0
        }
    }
}

/// Il testo che la sessione legge, a un gradino dato.
///
/// LA COMPETENZA NOMINATA È `handoff`, NON `mattpocock-skills:handoff`: la prima
/// scrive un file di memoria più il puntatore in `MEMORY.md`, che la sessione
/// dopo ricarica da sola; la seconda scrive nella cartella temporanea del
/// sistema, dove non la rilegge nessuno. Il gancio gemello sullo `Stop` diceva
/// già così, questo mandava altrove — due presidi che indicano strade diverse
/// valgono meno di uno solo.
///
/// Il separatore delle migliaia è il **punto**: `f"{used:,}".replace(',', '.')`
/// dell'originale. È testo che si confronta byte per byte, quindi `144,000` e
/// `144.000` sono due risposte diverse.
pub fn message(used: u64, percent: u64, step: u64) -> String {
    let count = gruppi(used).replace(',', ".");
    if step >= 85 {
        format!(
            "Contesto a {count} token, il {percent}% della finestra: la \
             compattazione automatica e' vicina. Comprime quello che c'e', non \
             sceglie cosa serve. La skill `handoff` scrive una consegna che \
             sceglie, e adesso c'e' ancora contesto per ragionare su cosa \
             metterci."
        )
    } else {
        format!(
            "Contesto a {count} token, il {percent}% della finestra. Da qui in \
             poi ogni risposta rilegge tutto: se il lavoro sta per passare a \
             un'altra sessione, la skill `handoff` costa meno adesso che dopo \
             la compattazione."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_finestra_si_dichiara_solo_con_cifre_e_positiva() {
        assert_eq!(declared_window("200000"), Some(200_000));
        assert_eq!(declared_window("  1000000 \n"), Some(1_000_000));
        // Lo zero e il negativo non dichiarano niente: si scende alla variabile
        // dopo, esattamente come fa `isdigit()` insieme al `> 0`.
        assert_eq!(declared_window("0"), None);
        assert_eq!(declared_window("-5"), None);
        assert_eq!(declared_window("+5"), None);
        assert_eq!(declared_window("boh"), None);
        assert_eq!(declared_window(""), None);
        assert_eq!(declared_window("1e6"), None);
        // Gli zeri iniziali sono cifre come le altre.
        assert_eq!(declared_window("00200000"), Some(200_000));
    }

    #[test]
    fn i_due_gradini_e_il_verso_del_confronto() {
        assert_eq!(step_reached(69), None);
        // Il confine è chiuso a sinistra: settanta è già il gradino.
        assert_eq!(step_reached(70), Some(70));
        assert_eq!(step_reached(84), Some(70));
        assert_eq!(step_reached(85), Some(85));
        assert_eq!(step_reached(119), Some(85));
        assert_eq!(step_reached(0), None);
    }

    #[test]
    fn la_percentuale_arrotonda_al_pari_come_python() {
        // 144k su 200k sono esattamente il 72%.
        assert_eq!(percent(144_000, 200_000), 72);
        // Gli stessi token su un milione sono il 14%: la finestra dichiarata
        // dalla sessione è la differenza fra un avviso e un avviso che mente.
        assert_eq!(percent(144_000, 1_000_000), 14);
        // La metà esatta va al pari, come `round()` di Python: 181k su 200k è
        // 90,5%, e `f64::round()` direbbe 91.
        assert_eq!(percent(181_000, 200_000), 90);
        assert_eq!(percent(183_000, 200_000), 92); // 91,5% → 92, sempre al pari
        assert_eq!(percent(0, 200_000), 0);
    }

    #[test]
    fn il_gradino_gia_detto_si_legge_dal_file() {
        assert!(already_said("70", 70));
        assert!(!already_said("70", 85));
        assert!(already_said("85", 70));
        assert!(already_said(" 85 \n", 85));
        // File vuoto: `'' or 0` di Python.
        assert!(!already_said("", 70));
        assert!(!already_said("   ", 70));
        // Illeggibile: non è «già detto», così al massimo si ripete l'avviso.
        assert!(!already_said("boh", 70));
        assert!(!already_said("-5", 70));
        // Ventun cifre non stanno in nessun intero di Rust, ma in Python sì: è
        // un numero enorme, non un file corrotto, e ha superato ogni gradino.
        assert!(already_said("999999999999999999999", 85));
        assert!(!already_said("-999999999999999999999", 70));
        assert!(!already_said("70.0", 70), "int() rifiuta il decimale");
    }

    #[test]
    fn il_testo_cambia_col_gradino_e_conta_le_migliaia_col_punto() {
        let basso = message(144_000, 72, 70);
        assert!(basso.starts_with("Contesto a 144.000 token, il 72% della finestra."), "{basso}");
        assert!(basso.contains("la skill `handoff` costa meno adesso"), "{basso}");
        // Al gradino basso NON si nomina la compattazione: è ciò che distingue
        // i due messaggi, e un porto che ne scrivesse uno solo passerebbe metà
        // dei casi senza che nessuno se ne accorga.
        assert!(!basso.contains("compattazione automatica"), "{basso}");

        let alto = message(180_000, 90, 85);
        assert!(alto.contains("il 90% della finestra: la compattazione automatica e' vicina"), "{alto}");
        assert!(alto.contains("Contesto a 180.000 token"), "{alto}");
    }

    #[test]
    fn il_testo_constata_e_non_ordina() {
        // La stessa prova dell'originale: nessun imperativo, in nessuno dei due
        // messaggi. Un ordine qui renderebbe impossibile misurare se serviva.
        for testo in [message(144_000, 72, 70), message(180_000, 90, 85)] {
            let t = testo.to_lowercase();
            assert!(!t.contains("invoca"), "{testo}");
            assert!(!t.contains("devi"), "{testo}");
        }
    }
}
