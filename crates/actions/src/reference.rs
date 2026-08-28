//! Come un passo riceve il lavoro del passo prima.
//!
//! **PERCHÉ NASCE.** Il motore consegna a un passo l'uscita delle sue
//! dipendenze — quella sola, così com'è — e il campo `with` del grafo ci
//! sovrascrive sopra valori decisi il giorno in cui il flusso è stato scritto.
//! Con questi due soli attrezzi uno smistamento non si può scrivere: il testo
//! che il nodo principale produce per un motore deve finire dentro
//! l'invocazione di quel motore, e nessuna costante può contenerlo. Fino al
//! 28/08/2026 l'unico modo era che il passo dopo fosse uno script, cioè che il
//! lavoro uscisse dal grafo.
//!
//! **IL BUCO SI COPRE CON UN RINVIO, NON CON UN INTERPRETE.** Dentro l'ingresso
//! di un passo un oggetto `{"$from": "<puntatore>"}` vale il valore che quel
//! puntatore JSON trova nell'ingresso stesso, `{"$join": [...]}` vale i suoi
//! pezzi uniti in un testo solo, e `{"$json": "<puntatore>"}` vale quel valore
//! scritto come JSON. Non c'è altro: nessuna espressione, nessuna condizione,
//! nessuna chiamata, nessun ordine di esecuzione. Il puntatore JSON è già il
//! modo in cui il grafo guarda dentro un valore — lo usa
//! `Condition::PointerEquals` per decidere se un passo gira — e qui si riusa
//! quello invece di inventare una seconda sintassi per la stessa domanda.
//!
//! **PERCHÉ ESISTE `$json` E NON BASTAVA `$join`.** `$join` unisce testo e
//! rifiuta tutto il resto, apposta: decidere come si scrive un numero è di chi
//! compone il messaggio. Ma da quando un passo può dichiarare *la forma* della
//! propria risposta, quella forma va detta al motore dentro il prompt — e
//! riscriverla a mano lì accanto vorrebbe dire tenerne due copie che un giorno
//! divergono in silenzio. `$json` è la sola conversione ammessa, e converte
//! l'unica cosa per cui non c'è scelta di stile: un valore strutturato, scritto
//! come JSON.
//!
//! **UN VALORE RISOLTO NON SI RILEGGE.** I rinvii si cercano nell'ingresso come
//! è arrivato, e ciò che ne esce entra al suo posto senza essere riguardato:
//! due rinvii non si concatenano, e un rinvio non può nascere da un dato. Chi
//! scrive il flusso decide cosa si legge; chi risponde no.
//!
//! **IL LIMITE, DETTO INVECE CHE NASCOSTO.** I rinvii si cercano in *tutto*
//! l'ingresso, e l'ingresso contiene anche l'uscita delle dipendenze. Oggi non
//! è un varco perché i motori rispondono con testo, e dentro un testo non c'è
//! nessun oggetto da riconoscere; lo diventerebbe con un'azione che produce
//! oggetti liberi — `store_read` restituisce il valore depositato così com'è —
//! passati poi a un nodo che esegue. Chi lega quei due nodi lo deve sapere.

use flow::ActionError;
use serde_json::{Map, Value};

/// La chiave che nomina un valore preso dall'ingresso del passo.
pub const FROM_KEY: &str = "$from";
/// La chiave che unisce più pezzi in un testo solo.
pub const JOIN_KEY: &str = "$join";
/// La chiave che scrive come JSON il valore che un puntatore trova.
pub const JSON_KEY: &str = "$json";

/// Sostituisce i rinvii dentro l'ingresso di un passo con i valori che
/// nominano, e restituisce l'ingresso che l'azione può leggere come sempre.
///
/// Un ingresso senza rinvii torna identico: chi non li usa non paga niente e
/// non cambia comportamento.
pub fn resolve_references(input: &Value) -> Result<Value, ActionError> {
    resolve_against(input, input)
}

fn resolve_against(value: &Value, root: &Value) -> Result<Value, ActionError> {
    match value {
        Value::Object(fields) => resolve_object(fields, root),
        Value::Array(items) => {
            let resolved: Result<Vec<Value>, ActionError> = items
                .iter()
                .map(|item| resolve_against(item, root))
                .collect();
            Ok(Value::Array(resolved?))
        }
        // Un testo non si guarda dentro: `{"$from": …}` scritto in mezzo a una
        // frase è una frase, non un rinvio.
        other => Ok(other.clone()),
    }
}

fn resolve_object(fields: &Map<String, Value>, root: &Value) -> Result<Value, ActionError> {
    if let Some(pointer) = fields.get(FROM_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(FROM_KEY));
        }
        return look_up(pointer, root);
    }
    if let Some(parts) = fields.get(JOIN_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(JOIN_KEY));
        }
        return join(parts, root);
    }
    if let Some(pointer) = fields.get(JSON_KEY) {
        if fields.len() != 1 {
            return Err(ambiguous(JSON_KEY));
        }
        let value = look_up(pointer, root)?;
        // `to_string` e non una forma indentata: è testo che finisce dentro un
        // prompt, e le righe in più sono token in più a ogni chiamata.
        return Ok(Value::String(serde_json::to_string(&value).expect(
            "un valore già in memoria si riserializza sempre",
        )));
    }
    let mut resolved = Map::new();
    for (name, value) in fields {
        resolved.insert(name.clone(), resolve_against(value, root)?);
    }
    Ok(Value::Object(resolved))
}

fn look_up(pointer: &Value, root: &Value) -> Result<Value, ActionError> {
    let Some(pointer) = pointer.as_str() else {
        return Err(ActionError::new(
            "invalid_reference",
            format!("{FROM_KEY} vuole un puntatore JSON come testo, non {pointer}"),
        ));
    };
    // Un puntatore senza la barra iniziale non trova mai niente, e senza questa
    // riga il flusso riceverebbe «non contiene» al posto di «è scritto male».
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(ActionError::new(
            "invalid_reference",
            format!("un puntatore JSON comincia con /: {pointer} no"),
        ));
    }
    root.pointer(pointer).cloned().ok_or_else(|| {
        ActionError::new(
            "unresolved_reference",
            format!("il flusso chiede {pointer}, che l'ingresso del passo non contiene"),
        )
    })
}

fn join(parts: &Value, root: &Value) -> Result<Value, ActionError> {
    let Some(parts) = parts.as_array() else {
        return Err(ActionError::new(
            "invalid_reference",
            format!("{JOIN_KEY} vuole un elenco di pezzi, non {parts}"),
        ));
    };
    let mut text = String::new();
    for part in parts {
        let resolved = resolve_against(part, root)?;
        match resolved.as_str() {
            Some(piece) => text.push_str(piece),
            // Unire un numero a un testo vorrebbe dire scegliere come si scrive
            // quel numero: è una decisione di chi compone il messaggio, non di
            // chi lo consegna.
            None => {
                return Err(ActionError::new(
                    "unjoinable_reference",
                    format!("{JOIN_KEY} unisce testo, e {resolved} non lo è"),
                ))
            }
        }
    }
    Ok(Value::String(text))
}

fn ambiguous(key: &str) -> ActionError {
    ActionError::new(
        "ambiguous_reference",
        format!("un rinvio {key} sta da solo; qui ha accanto altre chiavi e non si sa quale valga"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_input_without_references_comes_back_identical() {
        let input = json!({"bin": "codex", "args": ["exec"], "timeout_secs": 30});
        assert_eq!(resolve_references(&input).expect("nessun rinvio"), input);
    }

    /// Il caso per cui questo modulo esiste: il testo prodotto dal passo prima
    /// entra nell'invocazione del passo dopo.
    #[test]
    fn a_reference_carries_the_previous_answer_into_the_next_call() {
        let input = json!({
            "stdout": "cerca i residui in ~/.claude",
            "stdin": {"$from": "/stdout"}
        });

        let resolved = resolve_references(&input).expect("il puntatore esiste");

        assert_eq!(resolved["stdin"], json!("cerca i residui in ~/.claude"));
    }

    #[test]
    fn join_puts_a_fixed_role_in_front_of_what_the_engine_said() {
        let input = json!({
            "stdout": "elenca i ganci morti",
            "stdin": {"$join": ["Esegui solo la tua sezione.\n\n", {"$from": "/stdout"}]}
        });

        let resolved = resolve_references(&input).expect("il puntatore esiste");

        assert_eq!(
            resolved["stdin"],
            json!("Esegui solo la tua sezione.\n\nelenca i ganci morti")
        );
    }

    #[test]
    fn a_pointer_can_reach_a_named_dependency() {
        let input = json!({
            "codex": {"status": "ok", "stdout": "tre residui"},
            "agy": {"status": "timed_out", "stdout": ""},
            "env": {"CODEX": {"$from": "/codex/stdout"}, "AGY_STATUS": {"$from": "/agy/status"}}
        });

        let resolved = resolve_references(&input).expect("i puntatori esistono");

        assert_eq!(resolved["env"]["CODEX"], json!("tre residui"));
        assert_eq!(resolved["env"]["AGY_STATUS"], json!("timed_out"));
    }

    /// **LA MISURA CHE POTEVA VENIRE DIVERSA.** Un puntatore che non trova
    /// niente deve fermare il passo, non consegnargli il vuoto: un motore
    /// invocato con un incarico vuoto costa come uno invocato bene, e risponde
    /// qualcosa che sembra una risposta.
    #[test]
    fn a_pointer_that_finds_nothing_stops_the_step() {
        let input = json!({"stdin": {"$from": "/codex/stdout"}});

        let error = resolve_references(&input).expect_err("non c'è nessun codex");

        assert_eq!(error.class, "unresolved_reference");
        assert!(error.said.contains("/codex/stdout"), "{}", error.said);
    }

    #[test]
    fn a_pointer_without_its_leading_slash_says_so() {
        let input = json!({"stdout": "x", "stdin": {"$from": "stdout"}});

        let error = resolve_references(&input).expect_err("puntatore scritto male");

        assert_eq!(error.class, "invalid_reference");
        assert!(error.said.contains("comincia con /"), "{}", error.said);
    }

    #[test]
    fn a_reference_with_other_keys_beside_it_is_refused() {
        let input = json!({"stdout": "x", "stdin": {"$from": "/stdout", "oppure": "y"}});

        let error = resolve_references(&input).expect_err("rinvio ambiguo");

        assert_eq!(error.class, "ambiguous_reference");
    }

    /// La forma che un passo pretende dalla propria risposta finisce nel prompt
    /// passando di qui: scritta una volta sola, nel campo che poi la fa
    /// rispettare.
    #[test]
    fn a_declared_shape_becomes_the_text_that_goes_into_the_prompt() {
        let input = json!({
            "answer_shape": {"type": "object", "properties": {"total": {"type": "number"}}},
            "stdin": {"$join": ["Rispondi in questa forma: ", {"$json": "/answer_shape"}]}
        });

        let resolved = resolve_references(&input).expect("il puntatore esiste");

        assert_eq!(
            resolved["stdin"],
            json!("Rispondi in questa forma: {\"type\":\"object\",\"properties\":{\"total\":{\"type\":\"number\"}}}")
        );
    }

    /// `$json` scrive **lo stesso testo** che scriverebbe chi va a controllare
    /// che la forma sia davvero nel prompt: se le due scritture divergessero,
    /// quel controllo direbbe di no su un prompt corretto.
    #[test]
    fn the_written_shape_matches_what_a_reader_would_serialise() {
        let shape = json!({"type": "object", "required": ["a", "b"], "allow_extra": false});
        let input = json!({"answer_shape": shape.clone(), "text": {"$json": "/answer_shape"}});

        let resolved = resolve_references(&input).expect("il puntatore esiste");

        assert_eq!(
            resolved["text"].as_str().expect("testo"),
            serde_json::to_string(&shape).expect("serializzabile")
        );
    }

    #[test]
    fn a_json_reference_that_finds_nothing_stops_the_step() {
        let input = json!({"text": {"$json": "/non/ce"}});
        let error = resolve_references(&input).expect_err("il puntatore non trova niente");
        assert_eq!(error.class, "unresolved_reference");
    }

    #[test]
    fn join_refuses_to_decide_how_a_number_is_written() {
        let input = json!({"quanti": 3, "testo": {"$join": ["sono ", {"$from": "/quanti"}]}});

        let error = resolve_references(&input).expect_err("un numero non è testo");

        assert_eq!(error.class, "unjoinable_reference");
    }

    /// Il varco che resta chiuso: un motore risponde con testo, e dentro un
    /// testo nessun rinvio viene riconosciuto.
    #[test]
    fn a_reference_written_inside_an_engine_answer_stays_a_sentence() {
        let input = json!({
            "stdout": "ho scritto {\"$from\": \"/segreto\"} nella risposta",
            "segreto": "non deve uscire",
            "stdin": {"$from": "/stdout"}
        });

        let resolved = resolve_references(&input).expect("il puntatore esiste");

        assert!(
            resolved["stdin"]
                .as_str()
                .expect("testo")
                .contains("$from"),
            "il testo resta testo"
        );
        assert!(!resolved["stdin"].as_str().expect("testo").contains("non deve uscire"));
    }

    /// I rinvii leggono l'ingresso com'è arrivato, non come sta diventando:
    /// due rinvii non si concatenano, e l'ordine delle chiavi non conta.
    #[test]
    fn a_resolved_value_is_not_read_again() {
        let input = json!({
            "segreto": "x",
            "ponte": {"$from": "/segreto"},
            "arrivo": {"$from": "/ponte"}
        });

        let resolved = resolve_references(&input).expect("i puntatori esistono");

        assert_eq!(resolved["ponte"], json!("x"));
        assert_eq!(resolved["arrivo"], json!({"$from": "/segreto"}));
    }
}
