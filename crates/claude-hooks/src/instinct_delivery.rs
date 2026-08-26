//! `PreToolUse` su Bash: consegna il corpo di un istinto **nel momento in cui
//! l'innesco accade**, invece di tenerlo nel prologo di ogni sessione.
//!
//! L'involucro sa dove vivono i file e come si parla al gancio; il giudizio —
//! quale istinto è dovuto per un comando — sta in `guards::instinct_delivery`,
//! dove è puro e si prova senza toccare il disco.
//!
//! PERCHÉ NON È UNA COPIA DI `ai_personal_data`. Quello consegna **un testo
//! fisso** a chi scrive in una certa zona di codice; qui il testo è un file
//! sul disco che cambia quando la lezione viene rimisurata, la scelta di quale
//! mandare dipende dal comando, e la scadenza scritta nel frontmatter decide se
//! vale ancora. Il pezzo davvero comune ai due — il posto preso una volta sola
//! — è finito in `guards::instinct_delivery::claim`.
//!
//! NON BLOCCA MAI ed esce sempre 0. Una lezione che ferma il comando che la
//! innesca è un ostacolo, e un ostacolo si impara ad aggirare. Qui il comando
//! parte, e la lezione arriva insieme al suo esito: chi legge ha davanti sia
//! l'errore sia il perché.
//!
//! FAIL-OPEN OVUNQUE: payload illeggibile, file assente, casa introvabile —
//! tutto lascia passare in silenzio. Questo gira davanti a ogni comando di ogni
//! sessione, e il costo di un falso allarme qui è che si smetta di leggere gli
//! allarmi.

use hook_io::HookInput;

/// La casa dell'utente. Senza `HOME` non si consegna niente: indovinare un
/// percorso porterebbe a leggere il file di qualcun altro.
fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Chi riceve la consegna, ai fini del «una volta sola».
///
/// Un subagent condivide `session_id` con la madre ma ha un contesto tutto suo:
/// se il posto fosse preso per la sola sessione, il figlio resterebbe senza la
/// lezione perché la madre l'aveva già ricevuta in una conversazione che lui
/// non vede.
fn recipient(input: &HookInput) -> String {
    let session = input.session_id.as_deref().unwrap_or("ignota");
    match input.agent_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(agent) => format!("{session}-{agent}"),
        None => session.to_string(),
    }
}

pub fn run(input: &HookInput) -> i32 {
    if !input.is_tool("Bash") {
        return 0;
    }
    let command = input.bash_command();
    let Some(id) = guards::instinct_delivery::due_for_command(command) else {
        return 0;
    };
    let Some(home) = home() else {
        return 0;
    };

    // Prima si legge, poi si prende il posto: un file che non si legge oggi
    // deve poter arrivare al comando successivo, invece di restare muto per
    // tutta la sessione.
    let Ok(raw) = std::fs::read_to_string(guards::instinct_delivery::path_of(&home, id)) else {
        return 0;
    };
    let today: String = hook_io::local_time::now_local_iso8601().chars().take(10).collect();
    if !guards::instinct_delivery::is_live(&raw, &today) {
        return 0;
    }
    let body = guards::instinct_delivery::body_of(&raw);
    if body.trim().is_empty() {
        return 0;
    }

    let who = recipient(input);
    if !guards::instinct_delivery::claim(&std::env::temp_dir(), &who, id) {
        return 0;
    }

    let text = guards::instinct_delivery::delivery_text(body);
    println!(
        "{}",
        hook_io::python_json::dumps_unicode(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": text,
            }
        }))
    );
    // La riga di registro è l'unico modo di accorgersi che questa via è morta:
    // il prologo non elenca ciò che ha caricato, e una consegna che non arriva
    // non si vede da dentro la sessione.
    hook_io::journal::record(
        "consegna-istinto",
        "consegna",
        "istinto-consegnato",
        &[
            ("istinto", id.into()),
            ("destinatario", who.into()),
            ("byte", (text.len() as i64).into()),
        ],
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(tool: &str, command: &str, session: &str, agent: Option<&str>) -> HookInput {
        let mut v = serde_json::json!({
            "tool_name": tool,
            "tool_input": { "command": command },
            "session_id": session,
        });
        if let Some(a) = agent {
            v["agent_id"] = a.into();
        }
        serde_json::from_value(v).expect("payload")
    }

    /// Madre e figlio hanno la stessa sessione: se il posto fosse preso per la
    /// sola sessione, il subagent resterebbe senza una lezione che nel suo
    /// contesto non è mai arrivata.
    #[test]
    fn a_subagent_is_a_recipient_of_its_own() {
        let mother = payload("Bash", "find . -newermt '-2 hours'", "s1", None);
        let child = payload("Bash", "find . -newermt '-2 hours'", "s1", Some("agente-7"));
        assert_ne!(recipient(&mother), recipient(&child));
    }

    /// Un `agent_id` vuoto è la forma che prende un campo perso per strada, non
    /// un subagent: contarlo come tale darebbe alla madre due consegne.
    #[test]
    fn an_empty_agent_id_is_not_a_subagent() {
        let mother = payload("Bash", "ls", "s1", None);
        let blank = payload("Bash", "ls", "s1", Some("   "));
        assert_eq!(recipient(&mother), recipient(&blank));
    }

    /// Uno strumento che non è Bash non ha un comando da innescare: si esce
    /// prima di toccare il disco.
    #[test]
    fn nothing_is_delivered_outside_bash() {
        assert_eq!(run(&payload("Read", "find . -newermt '-2 hours'", "s1", None)), 0);
    }
}
