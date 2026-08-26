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

/// Dove vivono i corpi, relativo alla casa dell'utente.
pub const INSTINCTS_DIR: &str = ".claude/homunculus/instincts/personal";

/// Il file che contiene il corpo di un istinto.
pub fn path_of(home: &std::path::Path, id: &str) -> std::path::PathBuf {
    home.join(INSTINCTS_DIR).join(format!("{id}.md"))
}

/// Il corpo, cioè tutto ciò che sta **dopo** il frontmatter.
///
/// Il frontmatter serve a chi sceglie (data di scadenza, confidenza, innesco);
/// a chi riceve la lezione non dice niente e costa byte a ogni consegna. Un
/// file senza frontmatter è tutto corpo: è il caso di un istinto scritto a mano
/// male, e consegnarlo intero è meglio che consegnare il vuoto.
pub fn body_of(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix("---\n") else {
        return raw.trim_start();
    };
    match rest.split_once("\n---\n") {
        Some((_, body)) => body.trim_start(),
        // Frontmatter aperto e mai chiuso: non si indovina dove finisca, e
        // mandare il file intero è l'unico esito che non perde la lezione.
        None => raw.trim_start(),
    }
}

/// Un istinto si consegna solo finché la sua misura è ancora in piedi.
///
/// **Scaduto vale come assente**, e non è severità: il corpo di un istinto
/// racconta una misura fatta un giorno preciso, e riproporla dopo la scadenza
/// insegna qualcosa che nessuno ha più verificato. Il prologo fa la stessa
/// scelta — declassa lo scaduto alla sola riga — con la differenza che lì
/// resta una traccia visibile, qui no: perciò qui si tace del tutto.
///
/// Senza `expires`, o con una data che non è `YYYY-MM-DD`, non si consegna:
/// non sapere quando scade non è la stessa cosa che essere valido.
pub fn is_live(raw: &str, today: &str) -> bool {
    let fm = crate::instincts::parse_frontmatter(raw);
    let Some(expires) = fm.expires.as_deref() else {
        return false;
    };
    crate::instincts::is_iso_date(expires)
        && crate::instincts::is_iso_date(today)
        && expires >= today
}

/// Prende il posto per **questa sessione e questo istinto**, una volta sola.
///
/// Senza il marcatore la stessa lezione arriverebbe a ogni comando che la
/// innesca: chi sbaglia un `find` lo sbaglia in serie, e la seconda copia costa
/// quanto la prima senza insegnare niente di nuovo. `create_new` fallisce se il
/// file c'è già, quindi due strumenti in parallelo non consegnano due volte.
///
/// Il posto si prende **dopo** aver letto il corpo: se il file non si legge, la
/// sessione deve poterci riprovare invece di restare a mani vuote per sempre.
pub fn claim(tmp: &std::path::Path, session: &str, id: &str) -> bool {
    let marker = tmp.join(format!("claude-istinto-{session}-{id}"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .is_ok()
}

/// L'intestazione che accompagna il corpo, perché una lezione che arriva da
/// sola in mezzo a un turno sembra un pezzo di contesto qualunque: dire che è
/// **il comando appena scritto** ad averla chiamata è ciò che la rende
/// azionabile invece che decorativa.
pub fn delivery_text(body: &str) -> String {
    format!(
        "# Istinto misurato in casa, consegnato adesso perché il comando che stai per eseguire lo innesca\n\n\
         Non è una regola generale del mestiere: è una misura fatta su **questa** macchina, e vale qui.\n\n{body}"
    )
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

    /// Un istinto vero, nella forma esatta dei file sul disco.
    fn instinct(expires: &str, body: &str) -> String {
        format!(
            "---\nid: prova\ntrigger: \"quando succede\"\nconfidence: 0.9\nexpires: {expires}\n---\n{body}"
        )
    }

    /// Una cartella tutta sua per ogni prova che scrive marcatori: due prove che
    /// condividono la cartella si toglierebbero il posto a vicenda, e il
    /// fallimento cadrebbe su quella che gira seconda.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("istinti-prova-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di prova");
        dir
    }

    // ── il corpo, senza il frontmatter ─────────────────────────────────

    /// Il frontmatter serve a chi sceglie, non a chi riceve: ciò che parte è
    /// solo il corpo, e i campi di servizio non costano byte a ogni consegna.
    #[test]
    fn the_frontmatter_does_not_travel_with_the_body() {
        let raw = instinct("2026-12-31", "# Il titolo\n\nLa lezione.\n");
        assert_eq!(body_of(&raw), "# Il titolo\n\nLa lezione.\n");
        assert!(!body_of(&raw).contains("confidence"));
    }

    /// Due casi storti in cui la lezione **non si perde**: senza frontmatter è
    /// tutto corpo, e con un frontmatter mai chiuso non si indovina dove
    /// finisca. Consegnare qualcosa di troppo è meglio che tacere.
    #[test]
    fn a_file_without_a_closed_frontmatter_still_delivers_its_lesson() {
        assert_eq!(body_of("# Nudo\n\nLa lezione.\n"), "# Nudo\n\nLa lezione.\n");
        let unclosed = "---\nid: prova\nexpires: 2026-12-31\n# Il titolo\n\nLa lezione.\n";
        assert!(body_of(unclosed).contains("La lezione."));
    }

    // ── la scadenza ────────────────────────────────────────────────────

    /// Il giorno stesso della scadenza vale ancora; il giorno dopo no.
    #[test]
    fn an_instinct_is_live_up_to_its_expiry_day_included() {
        let raw = instinct("2026-09-24", "corpo");
        assert!(is_live(&raw, "2026-09-24"), "il giorno della scadenza vale");
        assert!(is_live(&raw, "2026-08-26"));
        assert!(!is_live(&raw, "2026-09-25"), "il giorno dopo non vale più");
    }

    /// Senza scadenza, o con una data che non è `YYYY-MM-DD`, non si consegna:
    /// non sapere quando scade non è la stessa cosa che essere valido.
    #[test]
    fn an_undated_instinct_is_not_delivered() {
        assert!(!is_live("---\nid: prova\n---\ncorpo", "2026-08-26"));
        assert!(!is_live(&instinct("2026-9-24", "corpo"), "2026-08-26"));
        assert!(!is_live(&instinct("prossimo mese", "corpo"), "2026-08-26"));
        assert!(!is_live("nessun frontmatter", "2026-08-26"));
    }

    // ── una volta sola ─────────────────────────────────────────────────

    /// La stessa lezione non arriva due volte nella stessa sessione: chi sbaglia
    /// un comando lo sbaglia in serie, e la seconda copia costa quanto la prima
    /// senza insegnare niente.
    #[test]
    fn the_same_lesson_is_delivered_once_per_session() {
        let tmp = scratch("una-volta");
        assert!(claim(&tmp, "sessione-a", "istinto-uno"));
        assert!(!claim(&tmp, "sessione-a", "istinto-uno"), "la seconda non passa");
    }

    /// Il posto è preso per una coppia, non per la sessione intera: un secondo
    /// istinto ha ancora il suo, e un'altra sessione non eredita il silenzio
    /// della prima.
    #[test]
    fn the_claim_is_per_lesson_and_per_session() {
        let tmp = scratch("per-coppia");
        assert!(claim(&tmp, "sessione-a", "istinto-uno"));
        assert!(claim(&tmp, "sessione-a", "istinto-due"), "un altro istinto ha il suo posto");
        assert!(claim(&tmp, "sessione-b", "istinto-uno"), "un'altra sessione riparte da zero");
    }

    /// Il corpo arriva intero dentro il testo consegnato, e l'intestazione dice
    /// perché è arrivato: senza quella riga sembra contesto qualunque.
    #[test]
    fn the_delivered_text_carries_the_whole_body_and_says_why_it_came() {
        let text = delivery_text("# Il titolo\n\nLa lezione.\n");
        assert!(text.ends_with("# Il titolo\n\nLa lezione.\n"));
        assert!(text.contains("innesca"));
    }
}
