//! Consegna un istinto **quando l'evento accade**, invece che nel prologo.
//!
//! PERCHÉ ESISTE. Il prologo è un costo fisso: entra in ogni sessione e in ogni
//! subagent, che la lezione serva o no. Un istinto sulle virgolette di zsh si
//! paga anche nei mille turni in cui non si tocca una riga di shell. Un gancio
//! è un costo condizionale: si paga solo quando l'innesco accade davvero.
//!
//! Misurato il 26/08/2026 sugli undici istinti vivi: **dieci hanno un innesco
//! che un payload può riconoscere**, otto dei quali su `Bash`. Il vincolo che
//! li teneva fuori era il tetto del prologo — 3.500 byte contro 23.884 di
//! materiale, con due corpi su undici che entravano interi — e su questa via
//! quel tetto non si applica, perché non si spende niente finché nessuno
//! scrive il comando che innesca.
//!
//! CHE LA CONSEGNA ARRIVI NON È UNA SCOMMESSA. `additionalContext` da
//! `PreToolUse` è stato collaudato lo stesso giorno sulla regola di
//! SocratiCode: 47 righe di consegna nel registro, e 6 occorrenze nel
//! transcript della frase che vive solo dentro quella regola.
//!
//! COSA NON PASSA DI QUI. Una lezione che deve cambiare **come imposti** il
//! lavoro arriva troppo tardi da un gancio: quando il payload esiste, la
//! decisione è già presa. Quelle restano nel prologo, ed è il motivo per cui il
//! prologo non va a zero.
//!
//! Qui c'è solo il giudizio, che è puro: quale istinto è dovuto per questo
//! comando. Il corpo lo legge l'involucro, che sa dove vivono i file.

/// Un istinto consegnabile: il suo identificativo — che è anche il nome del
/// file sotto `homunculus/instincts/personal/` — e il riconoscitore del suo
/// innesco.
///
/// I riconoscitori sono funzioni e non stringhe perché gli inneschi non hanno
/// tutti la stessa forma: uno è un'opzione, un altro è un comando in testa, un
/// altro ancora è una forma sintattica. Una tabella di sottostringhe li
/// avrebbe appiattiti sul caso più povero.
pub type Recognizer = fn(&str) -> bool;

/// La tabella in servizio. **Si aggiunge una riga per volta**, e ogni riga
/// entra solo dopo che il suo corpo è uscito dal prologo: consegnare dalle due
/// vie insieme costa il doppio e non si vede, perché nessuna delle due dichiara
/// cosa ha già consegnato l'altra.
pub const DELIVERABLE: &[(&str, Recognizer)] =
    &[("find-newermt-non-esiste-e-mente", asks_for_newer_than)];

/// `find … -newermt` e i suoi fratelli: non esistono su questa macchina, e il
/// modo in cui falliscono è il motivo per cui vale la pena avvisare prima.
///
/// Si guarda l'opzione con il trattino davanti, non la parola nuda: quel nome
/// dentro un file o dentro una stringa non è un uso dell'opzione, e un avviso
/// che scatta su un nome di file insegna a ignorare gli avvisi.
fn asks_for_newer_than(command: &str) -> bool {
    const OPTIONS: &[&str] = &["-newermt", "-newerat", "-newerct", "-newerBt"];
    command
        .split(|c: char| c.is_whitespace())
        .any(|word| OPTIONS.contains(&word))
}

/// Quale istinto è dovuto per questo comando, se ce n'è uno.
///
/// Uno solo per comando anche quando più inneschi combaciano: due lezioni
/// insieme si leggono come un muro, e la prima è quella che il comando ha
/// toccato per prima nella tabella. Se il caso diventasse frequente lo dirà il
/// registro, e allora si sceglierà con un criterio invece che con l'ordine.
pub fn due_for_command(command: &str) -> Option<&'static str> {
    DELIVERABLE
        .iter()
        .find(|(_, matches)| matches(command))
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le forme reali con cui l'opzione compare: da sola, in mezzo a un
    /// `find`, e nelle sue varianti.
    #[test]
    fn the_nonexistent_find_option_is_recognized_in_its_real_forms() {
        for command in [
            "find . -newermt '-2 hours'",
            "find /tmp -type f -newermt '2026-08-26' -print",
            "find . -newerat '-1 day' | wc -l",
            "find . -newerct '-30 minutes'",
        ] {
            assert_eq!(
                due_for_command(command),
                Some("find-newermt-non-esiste-e-mente"),
                "{command:?} doveva innescare"
            );
        }
    }

    /// La parola senza il trattino non è un uso dell'opzione. Un avviso che
    /// scatta qui insegna a ignorare gli avvisi, che è il modo più caro di
    /// rompere un gancio che funziona.
    #[test]
    fn the_word_without_its_dash_is_not_the_option() {
        for command in [
            "cat appunti-newermt.txt",
            "grep -c newermt registro.log",
            "ls /tmp/newerat/",
            "git commit -m 'newermt'",
        ] {
            assert_eq!(due_for_command(command), None, "{command:?} non è un uso");
        }
    }

    /// Un comando qualunque non consegna niente: è il caso di gran lunga più
    /// frequente, ed è quello che rende questa via gratis.
    #[test]
    fn an_ordinary_command_delivers_nothing() {
        for command in [
            "git status",
            "cargo test -p guards",
            "ls -la",
            "",
            "find . -name '*.rs'",
        ] {
            assert_eq!(due_for_command(command), None, "{command:?}");
        }
    }

    /// Ogni riga della tabella nomina un istinto che esiste davvero sul disco.
    /// Un identificativo scritto male non fallisce: consegna il vuoto, e la
    /// lezione non arriva senza che nessuno se ne accorga.
    #[test]
    fn every_deliverable_names_an_instinct_that_exists() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../homunculus/instincts/personal");
        // Il banco può girare dove quella cartella non c'è: allora questa
        // prova non ha niente da dire, e tacere è meglio che fallire a caso.
        if !dir.is_dir() {
            return;
        }
        for (id, _) in DELIVERABLE {
            assert!(
                dir.join(format!("{id}.md")).is_file(),
                "{id} non ha un file sotto {}",
                dir.display()
            );
        }
    }
}
