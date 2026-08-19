//! `PreToolUse` su Write/Edit: ricorda cosa vale quando il codice che si sta
//! scrivendo manda dati di persone a un modello.
//!
//! PERCHÉ NON BASTA LA REGOLA. Il promemoria esiste anche come
//! `~/.claude/rules/dati-personali-ai.md`, che però si accende solo se il file
//! toccato sta nell'albero da cui è partita la sessione: misurato l'84,5% dei
//! tocchi fuori da quell'albero. Il caso che ha prodotto questo gancio è
//! esattamente quello — sessione aperta nella suite, difetto corretto nel
//! motore, regola muta. Un gancio guarda il percorso che gli arriva, non da
//! dove è partita la sessione.
//!
//! DUE RILEVATORI, PERCHÉ IL PERCORSO NON BASTA. Il difetto del 19/08/2026
//! stava in `domain/validation/workflow.ts`, cioè dove il payload si
//! costruisce, non in `agents/`, dove il modello si dichiara. Al percorso si
//! affianca quindi il testo in scrittura: chi nomina `trackedGenerate` o
//! `getAgentById` sta parlando a un modello ovunque si trovi il file.
//!
//! NON BLOCCA MAI, ed esce sempre 0: è un promemoria, e un promemoria che
//! ferma il lavoro diventa un ostacolo da aggirare. Parla una volta per
//! sessione, sul canale `additionalContext` — `systemMessage` andrebbe
//! all'utente e non all'assistente, che è chi deve tenerne conto.

use hook_io::HookInput;
use std::path::PathBuf;

/// Le cartelle dove vive, per convenzione, il codice che parla a un modello.
const PATH_MARKERS: &[&str] = &[
    "/agents/",
    "/mastra/",
    "/prompts/",
    "/llm/",
    "/ai/",
    "/workflows/",
    "/validation/",
];

/// Nomi di file che tradiscono lo stesso mestiere ovunque stia il file.
const FILE_MARKERS: &[&str] = &["agent.", "prompt.", "judge."];

/// Ciò che, comparendo nel testo che si sta scrivendo, dice che quel codice
/// chiama un modello anche quando il percorso non lo lascia intuire. Sono le
/// forme viste nei tre repo più le due SDK dirette.
const CALL_MARKERS: &[&str] = &[
    "trackedGenerate",
    "getAgentById",
    "generateText",
    "streamText",
    "new Agent(",
    "chat.completions",
    "messages.create",
    "responses.create",
];

/// Il giudizio, puro: questo file merita il promemoria?
///
/// `written` è il testo che sta per finire su disco — per `Edit` la sola parte
/// nuova, che basta: chi aggiunge la chiamata al modello la scrive lì.
pub fn concerns_model(path: &str, written: &str) -> bool {
    if !path.ends_with(".ts") && !path.ends_with(".tsx") && !path.ends_with(".js") {
        return false;
    }
    // Un test che nomina un agente non manda niente a nessuno.
    if path.contains("/tests/") || path.contains(".test.") || path.contains(".spec.") {
        return false;
    }
    let lower = path.to_lowercase();
    let by_path = PATH_MARKERS.iter().any(|m| lower.contains(m))
        || FILE_MARKERS.iter().any(|m| lower.contains(m));
    let by_content = CALL_MARKERS.iter().any(|m| written.contains(m));
    by_path || by_content
}

/// Il promemoria. Corto di proposito: il resto sta nella regola, che da qui in
/// avanti chi legge sa di dover cercare.
const REMINDER: &str = "\
Questo file tocca il codice che manda dati a un modello. Tre cose misurate il \
19/08/2026 sul motore, dove il difetto è stato trovato:

- Il criterio è il payload, non l'intenzione: aprire ciò che si spedisce e \
cercarci un campo che il compito non guarda. Lì ogni regola dichiarava di quali \
segnali aveva bisogno, ma la dichiarazione viaggiava come etichetta descrittiva \
mentre l'array spedito era quello completo.
- Filtrare per tipo non basta: un segnale può essere un oggetto che ne contiene \
altri (lì `{first, last, email, phone}`, passato intero perché il tipo era fra \
quelli richiesti).
- Ciò che il modello risponde rientra: se la spiegazione cita i valori e viene \
conservata, il dato è tornato dalla porta di servizio.

Il vincolo che morde è il GDPR, dal 2018 — non l'AI Act, i cui obblighi sul \
reclutamento il Regolamento (UE) 2026/1744 ha spostato al 2 dicembre 2027. \
Il resto in ~/.claude/rules/dati-personali-ai.md";

/// Il marcatore che tiene il promemoria a una volta per sessione.
fn marker(session: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("claude-ai-personal-data-{session}"));
    p
}

pub fn run(input: &HookInput) -> i32 {
    let tool = input.tool_name.as_deref().unwrap_or("");
    if tool != "Write" && tool != "Edit" {
        return 0;
    }

    let Some(ti) = input.tool_input.as_ref() else {
        return 0;
    };
    let path = ti.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
    // `content` per Write, `new_string` per Edit: il testo che entra nel file.
    let written = ti
        .get("content")
        .or_else(|| ti.get("new_string"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !concerns_model(path, written) {
        return 0;
    }

    // Una volta per sessione. Un promemoria che si ripete a ogni scrittura
    // dentro lo stesso sottosistema smette di essere letto.
    let session = input.session_id.as_deref().unwrap_or("ignota");
    let stamp = marker(session);
    if stamp.exists() {
        return 0;
    }
    let _ = std::fs::write(&stamp, "1");

    println!(
        "{}",
        hook_io::python_json::dumps(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": REMINDER,
            }
        }))
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_the_file_where_the_model_is_declared() {
        assert!(concerns_model("/r/src/mastra/agents/validation/judge.agent.ts", ""));
    }

    #[test]
    fn catches_the_file_where_the_payload_is_built() {
        // Il caso reale: nessun marcatore «agents» nel percorso, ma
        // `/validation/` sì — e comunque la chiamata è nel testo.
        assert!(concerns_model(
            "/r/src/domain/validation/workflow.ts",
            "const result = await trackedGenerate(judge, prompt)"
        ));
    }

    #[test]
    fn catches_a_model_call_outside_any_expected_folder() {
        assert!(concerns_model(
            "/r/src/services/onboarding.ts",
            "await openai.chat.completions.create({ messages })"
        ));
    }

    #[test]
    fn stays_quiet_on_ordinary_code() {
        assert!(!concerns_model(
            "/r/src/domain/candidates/read.ts",
            "return prisma.candidates.findMany()"
        ));
    }

    #[test]
    fn stays_quiet_on_tests_and_on_non_code() {
        assert!(!concerns_model("/r/tests/unit/validation/judge.test.ts", "trackedGenerate"));
        assert!(!concerns_model("/r/src/mastra/agents/README.md", "new Agent("));
    }
}
