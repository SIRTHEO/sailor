//! LA PROVA CHE CONTA DI TUTTO IL CRATE (mandato del 27/08/2026, punto 4):
//! una regola vera di `guards` — `cd_guard`, il divieto di `cd X && …` fuori
//! dal workspace che ha già il suo freno — giudica un gesto arrivato da
//! Claude Code e lo stesso gesto arrivato da Codex, con lo stesso verdetto.
//!
//! LA MISURA CHE POTEVA VENIRE DIVERSA: lo stesso comando proibito, con la
//! sola provenienza cambiata (Claude → Codex), deve restare bloccato. Se
//! `judge` desse `Pass` sulla versione Codex, il traduttore non servirebbe a
//! niente — sarebbe un tipo in più senza effetto sul giudizio.

use gesture::from_claude::from_claude;
use gesture::from_codex::from_codex_hook_json;
use guards::cd_guard::judge;
use hook_io::Decision;

const FORBIDDEN_COMMAND: &str = "cd /repo && git status";
const ALLOWED_COMMAND: &str = "git -C /repo status";

fn claude_payload(command: &str) -> String {
    format!(
        r#"{{"hook_event_name":"PreToolUse","tool_name":"Bash",
        "tool_input":{{"command":"{command}"}},
        "cwd":"/home/someone/orca/general","session_id":"claude-session"}}"#
    )
}

/// Stessa forma catturata dal vivo il 26/08/2026 (vedi
/// `src/from_codex.rs`), coi campi propri di Codex che `Gesture` non porta —
/// per provare che restano innocui, non che si evitano scrivendoli.
fn codex_payload(command: &str) -> String {
    format!(
        r#"{{"hook_event_name":"PreToolUse","tool_name":"Bash",
        "tool_input":{{"command":"{command}"}},
        "cwd":"/home/someone/orca/general","session_id":"codex-session",
        "permission_mode":"bypassPermissions","model":"gpt-5.6-sol",
        "tool_use_id":"exec-1"}}"#
    )
}

#[test]
fn the_forbidden_gesture_is_blocked_from_both_sources_with_the_same_message() {
    let claude_input: hook_io::HookInput =
        serde_json::from_str(&claude_payload(FORBIDDEN_COMMAND)).unwrap();
    let from_claude_gesture = from_claude(&claude_input);
    let claude_decision = judge(from_claude_gesture.bash_command());

    let from_codex_gesture = from_codex_hook_json(&codex_payload(FORBIDDEN_COMMAND)).unwrap();
    let codex_decision = judge(from_codex_gesture.bash_command());

    match (&claude_decision, &codex_decision) {
        (Decision::Block(claude_msg), Decision::Block(codex_msg)) => {
            assert_eq!(claude_msg, codex_msg, "stesso comando, messaggio diverso");
        }
        other => panic!("atteso Block su entrambe le fonti, ottenuto {other:?}"),
    }
}

/// LA MISURA CHE POTEVA VENIRE DIVERSA: senza questo caso, un traduttore che
/// lasciasse passare tutto avrebbe superato anche il test sopra per il
/// motivo sbagliato — bloccando sempre, a prescindere dal comando.
#[test]
fn a_legitimate_gesture_from_codex_passes() {
    let gesture = from_codex_hook_json(&codex_payload(ALLOWED_COMMAND)).unwrap();
    assert_eq!(judge(gesture.bash_command()), Decision::Pass);
}

/// E lo stesso comando lecito, dalla parte di Claude Code, per chiudere il
/// confronto simmetrico: fonte diversa, stesso verdetto in entrambe le
/// direzioni.
#[test]
fn the_same_legitimate_gesture_from_claude_also_passes() {
    let claude_input: hook_io::HookInput =
        serde_json::from_str(&claude_payload(ALLOWED_COMMAND)).unwrap();
    let gesture = from_claude(&claude_input);
    assert_eq!(judge(gesture.bash_command()), Decision::Pass);
}
