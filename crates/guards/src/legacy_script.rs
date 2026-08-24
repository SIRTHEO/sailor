//! Il linguaggio deprecato dentro la configurazione, e le quattro forme che
//! restano legittime.
//!
//! PERCHÉ ESISTE. Il 24/08/2026 Theo ha deciso che il lavoro che **decide** —
//! instrada, giudica vivo o morto, calcola una soglia, cancella — vive in Rust,
//! e che restano in shell soltanto i pochi file che devono girare quando il
//! binario Rust non c'è. La decisione è stata scritta nel registro di bordo la
//! mattina stessa, e **non ha fermato niente**: nel pomeriggio una sessione ha
//! riscritto in Python uno strumento che esisteva già in Rust da quattro giorni,
//! e la sera il registro dei ganci contava 84 scritture su `.py` e 21 su `.sh`
//! dentro `~/.claude` in sette giorni. Fra le prime c'era perfino un gancio.
//!
//! Una decisione che nessun meccanismo applica non è una decisione: è
//! un'opinione archiviata. Questo è il meccanismo.
//!
//! PERCHÉ NEGA INVECE DI AVVERTIRE. Un avviso qui sarebbe teatro, e la casa lo
//! ha già misurato: il gate della ricerca semantica avverte, e un blocco su tre
//! viene superato ritentando lo stesso gesto. Chi sta per riparare uno script
//! deprecato di solito **sa** che è deprecato, e procede lo stesso perché la
//! riparazione è a portata di mano e la riscrittura no.
//!
//! PERCHÉ NON SI SOVRAPPONE A NIENTE. `code_language` giudica la **lingua** in
//! cui sono scritti identificatori e commenti; qui si giudica il **linguaggio di
//! programmazione** del file. Cercato prima di scrivere: nessun altro freno
//! guarda l'estensione del bersaglio.
//!
//! PERCHÉ LE ECCEZIONI SONO QUATTRO E NON UNA IN PIÙ. Ognuna è una forma in cui
//! il linguaggio non è una scelta ma un vincolo, e tutte si riconoscono dal
//! percorso: chi le allarga lo fa modificando questo elenco, dove si vede.

use hook_io::Decision;

/// L'avvio a freddo: gira **prima** che il binario dei ganci esista, o quando
/// non esiste affatto. Riscriverlo in Rust è un paradosso, non un lavoro — e
/// `rust-hooks-present.sh` lo dichiara nella propria intestazione da giorni.
const COLD_START: &[&str] = &[
    "scripts/rust-hooks-present.sh",
    "scripts/release-hooks.sh",
    "scripts/settings-fingerprint-watch.sh",
    "guard-scope.sh",
    "statusline.sh",
];

/// La famiglia legittima a cui appartiene questo percorso, se ce n'è una.
fn exempt(path: &str) -> Option<&'static str> {
    if COLD_START.iter().any(|f| path.ends_with(f)) {
        return Some("avvio a freddo");
    }
    // Una batteria si lancia a mano e prova uno script che esiste ancora:
    // portarla in Rust prima del programma che prova non ha senso.
    if path.contains(".test.") {
        return Some("batteria");
    }
    // Un gesto è una sequenza che Theo esegue una volta e poi butta. Nasce già
    // consumato, e non c'è niente da mantenere.
    if path.contains("/gesti-theo-") || path.contains("/apply-") {
        return Some("gesto una-tantum");
    }
    // Codice di terzi, e i flussi che l'ambiente vuole in JavaScript per
    // contratto: il linguaggio non lo scegliamo noi.
    for third in [
        "/plugins/",
        "/mattpocock-skills/",
        "/node_modules/",
        "/workflows/",
    ] {
        if path.contains(third) {
            return Some("il linguaggio non lo scegliamo noi");
        }
    }
    None
}

/// Il linguaggio deprecato di questo file, se il file ne ha uno.
fn deprecated_language(path: &str) -> Option<&'static str> {
    for suffix in [".sh", ".bash", ".zsh"] {
        if path.ends_with(suffix) {
            return Some("shell");
        }
    }
    if path.ends_with(".py") {
        return Some("Python");
    }
    for suffix in [".js", ".mjs"] {
        if path.ends_with(suffix) {
            return Some("JavaScript");
        }
    }
    None
}

/// Il verdetto su una scrittura.
///
/// `exists` distingue il file nuovo da quello che c'era già, perché sono due
/// errori di gravità diversa: modificare uno script vivo può essere una
/// riparazione urgente, crearne uno nuovo è sempre una scelta — e una scelta
/// contro una decisione già presa.
///
/// `already_said` è la taratura che decide se questo freno sopravvive. A
/// messaggio pieno per ogni gesto costerebbe **103 dinieghi il 24/08/2026 e 424
/// il 17/08** — lo stesso ordine di grandezza dei 432 che, misurati su una bozza
/// di un altro freno, bastarono a farla respingere. Contando una volta per file
/// si scende a **28 e 71**. Quindi dal secondo gesto sullo stesso file la
/// spiegazione si accorcia a una riga.
///
/// **Ma il secondo tentativo non passa**, ed è la differenza da come si comporta
/// il gate della ricerca: là il rilancio identico è un'uscita libera, e un blocco
/// su tre se ne va di lì. Qui l'unica porta è la valvola, che lascia traccia nel
/// registro — così chi ha valutato e chi ha solo ripetuto il gesto smettono di
/// somigliarsi.
pub fn judge(path: &str, exists: bool, already_said: bool) -> Decision {
    // Fuori dalla configurazione non si dice niente: negli altri depositi il
    // linguaggio lo decide chi ci lavora, non questa casa.
    if !path.contains("/.claude/") {
        return Decision::Pass;
    }
    let Some(language) = deprecated_language(path) else {
        return Decision::Pass;
    };
    if exempt(path).is_some() {
        return Decision::Pass;
    }

    if already_said {
        return Decision::Deny(format!(
            "BLOCCATO (legacy-script): {path} è ancora uno script che la decisione del 24/08/2026 \
             dà per deprecato, e il motivo per esteso è già stato detto in questa sessione.\n  \
             · consapevole e necessario: `LEGACY_SCRIPT=off <comando>`, che resta nel registro"
        ));
    }

    let what = if exists {
        format!("modifica a uno script {language} che la decisione del 24/08/2026 dà per deprecato")
    } else {
        format!("creazione di uno script {language} nuovo dentro la configurazione")
    };
    let advice = if exists {
        "· se la correzione è **urgente e quel file gira sotto `launchd` adesso**, è il solo caso \
         in cui si tocca lo script invece di riscriverlo: passa dalla valvola, che resta scritta \
         nel registro\n  \
         · altrimenti il lavoro va in `rust/crates/`, dove sta già l'84% di questa casa\n  \
         · e prima ancora: quel difetto si vede in servizio, o solo dentro il perimetro di una \
         sessione? Il secondo caso non è un difetto di produzione, è lavoro sprecato su codice \
         che va comunque riscritto"
    } else {
        "· uno script nuovo qui non ha nessuno dei quattro motivi che tengono in vita quelli \
         esistenti: va scritto in `rust/crates/`\n  \
         · e prima di scriverlo, cerca se esiste già: il 24/08 uno strumento è stato riscritto in \
         Python mentre viveva in Rust da quattro giorni"
    };

    Decision::Deny(format!(
        "BLOCCATO (legacy-script): {what}.\n  \
         Il lavoro che decide — instrada, giudica, calcola una soglia, cancella — sta in Rust: è \
         la decisione presa da Theo il 24/08/2026, e questo freno esiste perché quel giorno la \
         decisione, scritta, non fermò niente.\n  \
         {advice}\n  \
         · restano legittimi in silenzio: avvio a freddo, batterie `*.test.*`, gesti una-tantum, \
         codice di terzi\n  \
         · consapevole e necessario: `LEGACY_SCRIPT=off <comando>`, che resta nel registro"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied(path: &str, exists: bool) -> bool {
        matches!(judge(path, exists, false), Decision::Deny(_))
    }

    /// Il caso per cui il freno nasce: riparare uno script che decide, invece di
    /// portarlo in Rust. I tre percorsi sono veri, e i primi due sono stati
    /// toccati il 24/08 proprio così.
    #[test]
    fn a_deciding_script_inside_the_config_is_refused() {
        assert!(denied("/Users/theo/.claude/scripts/deposito-guasti.sh", true));
        assert!(denied("/Users/theo/.claude/scripts/queue-patrol.sh", true));
        assert!(denied(
            "/Users/theo/.claude/skills/hooks/lingua-codice.py",
            true
        ));
    }

    /// Creare è sempre una scelta, e il messaggio lo dice diversamente:
    /// all'esistente si offre la porta di `launchd`, al nuovo no.
    ///
    /// MUTANTE: fatto tornare lo stesso messaggio nei due rami, questo va in
    /// rosso.
    #[test]
    fn a_brand_new_script_says_something_different_from_an_edit() {
        let Decision::Deny(nuovo) = judge("/Users/theo/.claude/scripts/x.py", false, false) else {
            panic!("doveva negare il file nuovo");
        };
        let Decision::Deny(esistente) = judge("/Users/theo/.claude/scripts/x.py", true, false) else {
            panic!("doveva negare il file esistente");
        };
        assert!(nuovo.contains("cerca se esiste già"), "{nuovo}");
        assert!(esistente.contains("launchd"), "{esistente}");
        assert_ne!(nuovo, esistente);
    }

    /// LA TARATURA CHE DECIDE SE IL FRENO SOPRAVVIVE: dal secondo gesto sullo
    /// stesso file la spiegazione si accorcia, ma **la porta non si apre**. È la
    /// differenza dal gate della ricerca, dove il rilancio identico passa.
    ///
    /// MUTANTE: fatto tornare `Pass` quando `already_said`, questo va in rosso —
    /// e il freno diventerebbe un avviso, cioè teatro.
    #[test]
    fn the_second_gesture_is_shorter_but_still_refused() {
        let path = "/Users/theo/.claude/scripts/deposito-guasti.sh";
        let Decision::Deny(prima) = judge(path, true, false) else {
            panic!("il primo gesto doveva negare");
        };
        let Decision::Deny(poi) = judge(path, true, true) else {
            panic!("anche il secondo gesto deve negare, non passare");
        };
        assert!(poi.len() < prima.len() / 2, "la seconda volta deve costare molto meno: {poi}");
        assert!(poi.contains("LEGACY_SCRIPT=off"), "la sola porta resta la valvola: {poi}");
    }

    /// LE QUATTRO ECCEZIONI, una per una. Senza queste il freno negherebbe
    /// lavoro che non ha alternativa, e verrebbe spento entro un giorno — che è
    /// il modo in cui i freni muoiono in questa casa.
    #[test]
    fn the_four_legitimate_shapes_pass_in_silence() {
        for path in [
            "/Users/theo/.claude/scripts/rust-hooks-present.sh",
            "/Users/theo/.claude/guard-scope.sh",
            "/Users/theo/.claude/scripts/deposito-guasti.test.sh",
            "/Users/theo/.claude/scripts/queue-select.test.sh",
            "/Users/theo/.claude/scripts/gesti-theo-2026-08-24.sh",
            "/Users/theo/.claude/scripts/apply-theo-decisions.sh",
            "/Users/theo/.claude/plugins/qualcosa/setup.sh",
            "/Users/theo/.claude/workflows/divide-and-verify.js",
        ] {
            assert!(
                matches!(judge(path, true, false), Decision::Pass),
                "doveva passare: {path}"
            );
        }
    }

    /// Fuori dalla configurazione non si dice niente.
    ///
    /// MUTANTE: tolto il controllo su `/.claude/`, questo va in rosso — e senza
    /// di esso il freno parlerebbe dentro i depositi di lavoro, dove la
    /// decisione di Theo non si applica.
    #[test]
    fn scripts_outside_the_config_are_none_of_our_business() {
        for path in [
            "/Users/theo/gyver/work/scripts/deploy.sh",
            "/Users/theo/orca/general/tools/x.py",
            "/tmp/prova.sh",
        ] {
            assert!(
                matches!(judge(path, true, false), Decision::Pass),
                "non doveva dire niente: {path}"
            );
        }
    }

    /// Il Rust, la prosa e le impostazioni non passano nemmeno da qui: il freno
    /// guarda il linguaggio, non il fatto di stare dentro la configurazione.
    #[test]
    fn rust_and_prose_are_never_touched() {
        for path in [
            "/Users/theo/.claude/rust/crates/guards/src/legacy_script.rs",
            "/Users/theo/.claude/docs/libro-di-bordo.md",
            "/Users/theo/.claude/settings.json",
        ] {
            assert!(matches!(judge(path, true, false), Decision::Pass), "{path}");
        }
    }
}
