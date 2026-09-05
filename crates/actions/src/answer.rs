//! What a step makes of what its command said: the failures it tolerates, the
//! tail it reports when red, and the declared shape its answer must fit.

use crate::spec::EngineSpec;
use flow::{ActionError, Refusal, RefusalRule, ValueSchema};
use serde_json::Value;

/// Gli esiti di fallimento che un motore esterno può produrre.
pub(crate) const ENGINE_FAILURES: [&str; 3] = ["exit_error", "timed_out", "spawn_failed"];
/// Quelli di una verifica di shell.
pub(crate) const CHECK_FAILURES: [&str; 2] = ["failed", "timed_out"];

/// Un `accept` che nomina un esito impossibile è un errore di chi ha scritto il
/// passo, non un silenzio: darebbe una tolleranza che non si applica mai, e il
/// passo diventerebbe rosso il giorno in cui serviva che non lo fosse.
pub(crate) fn check_tolerance(accept: &[String], known: &[&str]) -> Result<(), ActionError> {
    for name in accept {
        if !known.contains(&name.as_str()) {
            return Err(ActionError::new(
                "invalid_input",
                format!(
                    "`accept` names «{name}», which this step cannot produce; the possible values are: {}",
                    known.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn tolerates(accept: &[String], status: &str) -> bool {
    accept.iter().any(|name| name == status)
}

/// Gli esiti che non lasciano nessuna risposta da mettere in forma.
const SILENT_FAILURES: [&str; 2] = ["timed_out", "spawn_failed"];

/// **CHIEDERE SENZA VERIFICARE E VERIFICARE SENZA CHIEDERE SONO LO STESSO
/// DIFETTO**, e questo controllo chiude il cerchio dalla parte che di solito
/// resta aperta: un motore non rispetta una forma perché qualcuno l'ha
/// dichiarata in un campo, la rispetta se gliel'hanno detta. Qui si guarda che
/// il testo della forma compaia davvero in ciò che sta per partire — sia esso
/// l'ingresso o un argomento — e se non c'è il passo si ferma **prima** di
/// spendere una chiamata che fallirebbe di sicuro.
pub(crate) fn shape_was_asked_for(written: &str, spec: &EngineSpec) -> Result<(), ActionError> {
    if let Some(silent) = SILENT_FAILURES
        .iter()
        .find(|status| tolerates(&spec.accept, status))
    {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step declares a shape for the answer and at the same time tolerates «{silent}», which leaves no answer at all: the two do not go together"
            ),
        ));
    }
    let mut sent = spec.stdin.clone().unwrap_or_default();
    for arg in &spec.args {
        sent.push('\n');
        sent.push_str(arg);
    }
    if sent.contains(written) {
        return Ok(());
    }
    Err(ActionError::new(
        "shape_not_in_prompt",
        format!(
            "the step demands an answer in a declared shape, and that shape does not appear in what it sends the engine: put it in the prompt with a {} reference to /answer_shape, so it is written once. The shape is: {written}",
            flow::reference::JSON_KEY
        ),
    ))
}

/// Quanto di ciò che ha detto un comando entra nel messaggio di un passo rotto.
const SAID_TAIL: usize = 1200;

/// **LE ULTIME RIGHE, NON LE PRIME.** Un motore che fallisce scrive l'errore in
/// fondo, dopo pagine di avvio. E servono davvero qui dentro: un passo rotto non
/// scrive nessuna uscita tipata, quindi senza questo testo stdout e stderr
/// muoiono col processo e chi guarda il deposito trova un rosso senza motivo.
fn tail(text: &str) -> &str {
    let text = text.trim_end();
    if text.len() <= SAID_TAIL {
        return text;
    }
    let mut start = text.len() - SAID_TAIL;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

pub(crate) fn what_it_said(stdout: &str, stderr: &str) -> String {
    let mut parts = Vec::new();
    if !stderr.trim().is_empty() {
        parts.push(format!("stderr: {}", tail(stderr)));
    }
    if !stdout.trim().is_empty() {
        parts.push(format!("stdout: {}", tail(stdout)));
    }
    if parts.is_empty() {
        return "it said nothing, on stdout or on stderr".to_owned();
    }
    parts.join("\n")
}

/// `None` non è «uscito con zero»: è un processo ucciso da un segnale, e
/// confonderli manda a cercare un guasto nel posto sbagliato.
pub(crate) fn how_it_exited(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("it exited with code {code}"),
        None => "it was killed by a signal".to_owned(),
    }
}

/// Il testo da leggere come JSON dentro ciò che un motore ha detto.
///
/// Un modello incornicia spesso la risposta in un blocco recintato, a volte
/// dopo una riga di cortesia: si accetta **il primo blocco recintato**, e se non
/// ce n'è nessuno tutto il testo. Non si cercano le parentesi più esterne dentro
/// una frase: quella regola accetterebbe anche mezza risposta, o un esempio
/// citato nel discorso, e un dato sbagliato che passa è peggio di un rosso.
fn json_body(said: &str) -> &str {
    let trimmed = said.trim();
    let Some(open) = trimmed.find("```") else {
        return trimmed;
    };
    let after = &trimmed[open + 3..];
    // La riga della recinzione può portare il nome del linguaggio: si scarta.
    let body = match after.find('\n') {
        Some(end) => &after[end + 1..],
        None => return trimmed,
    };
    match body.find("```") {
        Some(close) => body[..close].trim(),
        None => body.trim(),
    }
}

/// Tiene solo i campi che la forma dichiara. `allow_extra` dice cosa si
/// **tollera** nella risposta; questa potatura dice cosa si **inoltra**, e sono
/// due domande diverse: la prima difende dal motore prolisso, la seconda dal
/// costo di portarselo dietro per tutta la catena.
fn pruned(shape: &ValueSchema, value: Value) -> Value {
    match (shape, value) {
        // No field declared and extras allowed: the shape says «an object,
        // whatever it holds», and pruning it would forward `{}` every time.
        (
            ValueSchema::Object {
                properties,
                allow_extra: true,
                ..
            },
            value @ Value::Object(_),
        ) if properties.is_empty() => value,
        (ValueSchema::Object { properties, .. }, Value::Object(fields)) => {
            let mut kept = serde_json::Map::new();
            for (name, item) in fields {
                if let Some(inner) = properties.get(&name) {
                    kept.insert(name, pruned(inner, item));
                }
            }
            Value::Object(kept)
        }
        (ValueSchema::Array { items }, Value::Array(values)) => Value::Array(
            values
                .into_iter()
                .map(|value| pruned(items, value))
                .collect(),
        ),
        (_, value) => value,
    }
}

/// The name under which the declared shape of an answer refuses one.
pub(crate) const ANSWER_SHAPE_CHECK: &str = "answer_shape";

/// Legge la risposta di un motore secondo la forma che il passo ha dichiarato.
pub(crate) fn shaped_answer(shape: &ValueSchema, said: &str) -> Result<Value, ActionError> {
    let body = json_body(said);
    let value: Value = serde_json::from_str(body).map_err(|error| {
        ActionError::new(
            "answer_not_json",
            format!(
                "the step demands an answer in a declared shape, and what arrived is not JSON: {error}; it said: {}",
                tail(said)
            ),
        )
        .refused(Refusal::new(
            ANSWER_SHAPE_CHECK,
            "",
            RefusalRule::NotJson,
            body,
        ))
    })?;
    shape.validate(&value).map_err(|error| {
        ActionError::new(
            "answer_off_shape",
            format!(
                "the answer does not respect the shape the step declared ({error}); it said: {}",
                tail(said)
            ),
        )
        .refused(error.refused_by(ANSWER_SHAPE_CHECK))
    })?;
    Ok(pruned(shape, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **A SHAPE WITH NO FIELD AND EXTRAS ALLOWED IS «ANY OBJECT»**, not «no
    /// field»: pruned to its declaration it came out `{}`, and the flow a model
    /// had drafted whole reached the next step empty.
    #[test]
    fn an_object_shape_declaring_no_field_hands_the_whole_object_on() {
        let shape: ValueSchema = serde_json::from_value(json!({
            "type": "object", "properties": {}, "required": [], "allow_extra": true
        }))
        .expect("a shape");
        let whole = json!({"id": "una-bozza", "graph": {"steps": []}});
        assert_eq!(pruned(&shape, whole.clone()), whole);
    }
}
