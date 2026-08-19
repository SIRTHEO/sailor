//! Quando una sessione riparte da un riassunto, quante volte è già successo.
//!
//! Porto del giudizio di `skills/hooks/restart-notice.py`. Qui non si legge
//! niente dal disco: si contano righe che arrivano già lette, e si compone il
//! testo. L'involucro — stdin, il file, stdout — sta in `claude-hooks::restart`.
//!
//! PERCHÉ ESISTE, con la misura dell'originale: su 986 sessioni, 895
//! compattazioni, e in 834 ripartenze osservate la sessione fa 9 chiamate
//! (mediana) prima di riuscire a modificare qualcosa. Il costo peggiore però non
//! è quello: chi riparte da un riassunto non dichiara di star ripartendo, quindi
//! chi legge non sa se parla con qualcuno che ricorda o con qualcuno che sta
//! ricostruendo.

/// La frase con cui l'harness apre un riassunto di compattazione.
pub const PHRASE: &str = "This session is being continued";

/// Quante volte la sessione è ripartita, e quante chiamate a strumenti ha fatto.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Restarts {
    pub restarts: u32,
    pub tool_calls: u32,
}

/// La riga è un vero riassunto di compattazione?
///
/// CERCARE LA FRASE E BASTA NON FUNZIONA, ed è il difetto che questo file esiste
/// per non rifare: la frase compare anche dentro il lavoro — un rapporto che la
/// cita, uno script che la filtra, il sorgente stesso della guardia. Misurato
/// dall'originale il 10/08/2026 sulla trascrizione che ha prodotto tutti questi
/// numeri: **17 presunte ripartenze contro una vera**.
///
/// Quindi si pretende un messaggio dell'utente il cui testo *comincia* con la
/// frase, o il marcatore esplicito. «Comincia», non «contiene»: è l'unica
/// differenza fra 1 e 17.
pub fn is_restart(line: &str) -> bool {
    if line.contains("\"isCompactSummary\":true") {
        return true;
    }
    let Ok(o) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    if o.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if o.get("type").and_then(|v| v.as_str()) != Some("user") {
        return false;
    }
    let content = o.get("message").and_then(|m| m.get("content"));
    let text = match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return false,
    };
    // `c.lstrip()[:120].startswith(FRASE)` dell'originale: la finestra di 120
    // caratteri non cambia l'esito — la frase ne conta 31 — ma si conserva
    // perché è il confronto che l'oracolo fa.
    let head: String = text.trim_start().chars().take(120).collect();
    head.starts_with(PHRASE)
}

/// Conta ripartenze e chiamate su righe già lette.
///
/// Il filtro grezzo sulla stringa prima di decodificare il JSON non è
/// ottimizzazione prematura: una trascrizione arriva a centinaia di megabyte e
/// questo giro sta davanti a ogni ripartenza.
pub fn count_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Restarts {
    let mut out = Restarts::default();
    for line in lines {
        if (line.contains(PHRASE) || line.contains("isCompactSummary")) && is_restart(line) {
            out.restarts += 1;
        }
        // Anche qui si pretende il turno dell'assistente: `"type":"tool_use"`
        // compare pure dentro gli script che analizzano le trascrizioni, ed è lo
        // stesso difetto di sopra in piccolo.
        if line.contains("\"type\":\"tool_use\"") && line.contains("\"type\":\"assistant\"") {
            out.tool_calls += line.matches("\"type\":\"tool_use\"").count() as u32;
        }
    }
    out
}

/// Il testo che la sessione riceve. Vuoto non è previsto: se si arriva qui, si parla.
pub fn message(r: &Restarts, max_restarts: u32, max_tool_calls: u32) -> String {
    let volte = if r.restarts == 1 { "volta" } else { "volte" };
    let mut text = format!(
        "RIPARTENZA DA UN RIASSUNTO — riprendi senza annunciarlo.\n\n\
         Questa sessione ha già ripreso {} {volte} e ha fatto {} chiamate a \
         strumenti.\n\n\
         Il primo messaggio del turno è già lavoro: nessuna riga su cosa hai \
         perso, nessun preambolo sulla ripresa. Chi legge vuole il passo \
         successivo, non il resoconto di come ci sei arrivato. Se davvero ti \
         manca un fatto per proseguire, chiedi quel fatto — non raccontare la \
         lacuna.",
        r.restarts, r.tool_calls
    );
    if r.restarts >= max_restarts || r.tool_calls >= max_tool_calls {
        let mut reasons: Vec<String> = Vec::new();
        if r.restarts >= max_restarts {
            reasons.push(format!("{} ripartenze (tetto {max_restarts})", r.restarts));
        }
        if r.tool_calls >= max_tool_calls {
            reasons.push(format!("{} chiamate (tetto {max_tool_calls})", r.tool_calls));
        }
        text.push_str(&format!(
            "\n\nOLTRE IL TETTO: {}.\n\n\
             Misura del 10/08/2026: nelle sessioni di questa taglia chi lavora \
             con te deve correggere o ripetere una volta su quattro, contro una \
             su sette nelle sessioni piccole. Misura dell'11/08/2026 su 888 \
             sessioni: l'8% più lungo consuma l'82% di tutta la cache riletta, e \
             la cache per risposta triplica mentre i token prodotti per risposta \
             restano gli stessi — una sessione lunga non rende di più, costa tre \
             volte tanto per unità di lavoro.\n\n\
             COME SI CHIUDE, concretamente: invoca la skill `handoff`, che scrive \
             un memory file di consegna + il puntatore in MEMORY.md — la prossima \
             sessione lo ricarica da sola, senza che nessuno debba dire «riprendi \
             da lì». Non `mattpocock-skills:handoff`, che scrive in cartella \
             temporanea e non viene ricaricata. Non riscrivere a mano un riassunto: \
             esiste lo strumento, ed è il motivo per cui questo avviso non è bastato \
             le volte scorse. Se decidi di continuare lo stesso, dillo e spiega perché.",
            reasons.join(", e ")
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_ripartenza_vera_si_riconosce() {
        assert!(is_restart(
            r#"{"type":"user","message":{"content":"This session is being continued from a previous conversation"}}"#
        ));
        assert!(is_restart(r#"{"isCompactSummary":true,"type":"user"}"#));
    }

    #[test]
    fn chi_ne_parla_soltanto_non_conta() {
        // Le tre trappole che hanno gonfiato il conteggio da 1 a 17: qualcuno
        // che *parla* delle compattazioni invece di ripartire da una.
        assert!(!is_restart(
            r#"{"type":"user","message":{"content":[{"type":"text","text":"il filtro cerca «This session is being continued» nel testo"}]}}"#
        ));
        assert!(!is_restart(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"This session is being continued compare anche qui"}]}}"#
        ));
        assert!(!is_restart(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"grep -c 'This session is being continued' → 17"}]}}"#
        ));
        assert!(!is_restart("non e' json"));
    }

    #[test]
    fn le_chiamate_si_contano_anche_piu_di_una_per_riga() {
        let riga = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"},{"type":"tool_use","name":"Read"}]}}"#;
        assert_eq!(count_lines([riga].into_iter()).tool_calls, 2);
        // SENZA PRETENDERE ANCHE `"type":"assistant"` si conterebbe questa: una
        // riga dell'utente che porta con sé il risultato strutturato di uno
        // strumento, dove la sequenza compare **non** protetta dai backslash.
        //
        // La prima stesura di questo caso metteva la sequenza dentro una
        // stringa JSON (`"cerco \"type\":\"tool_use\""`), dove gli apici sono
        // protetti e il filtro non la vede comunque: il caso era verde con e
        // senza la guardia, e infatti il mutante che toglieva la pretesa è
        // sopravvissuto. Un caso che non discrimina non prova niente.
        let annidato = r#"{"type":"user","toolUseResult":{"blocks":[{"type":"tool_use","name":"X"}]},"message":{"content":"x"}}"#;
        assert!(annidato.contains(r#""type":"tool_use""#), "il caso non contiene la sequenza");
        assert_eq!(count_lines([annidato].into_iter()).tool_calls, 0);
    }

    #[test]
    fn sotto_il_tetto_non_si_ordina_di_chiudere() {
        let r = Restarts { restarts: 1, tool_calls: 10 };
        assert!(!message(&r, 5, 3000).contains("OLTRE IL TETTO"));
        assert!(message(&r, 5, 3000).contains("ha già ripreso 1 volta"));
    }

    #[test]
    fn sopra_ciascuno_dei_due_tetti_lo_si_dice() {
        assert!(message(&Restarts { restarts: 5, tool_calls: 10 }, 5, 3000)
            .contains("OLTRE IL TETTO"));
        assert!(message(&Restarts { restarts: 0, tool_calls: 3000 }, 5, 3000)
            .contains("OLTRE IL TETTO"));
        // Entrambi: le due ragioni si uniscono con «, e », non con una virgola.
        let due = message(&Restarts { restarts: 9, tool_calls: 4000 }, 5, 3000);
        assert!(due.contains("9 ripartenze (tetto 5), e 4000 chiamate (tetto 3000)"), "{due}");
    }

    #[test]
    fn il_plurale_segue_il_numero() {
        assert!(message(&Restarts { restarts: 2, tool_calls: 0 }, 5, 3000)
            .contains("ripreso 2 volte"));
    }
}
