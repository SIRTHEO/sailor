//! Una figura di guardia non apre un modulo a Theo: la domanda va al capitano.
//!
//! PERCHÉ ESISTE, con le parole di chi ha fondato la regola. Theo, il
//! 21/08/2026: *«io non dovrei neanche presidiare il macchinista autorizzando
//! permessi; dovrebbe farlo il capitano, e il capitano riferire a me»*. E la
//! sera dello stesso giorno, dopo averne risolte tre a mano: **«le ho risolte io
//! ma non dovrebbe essere così»**.
//!
//! LA PROSA NON BASTA, ED È IL PUNTO. La regola era scritta nei mandati, nelle
//! schede dei mestieri e perfino in una nota iniettata a ogni sessione
//! (`homunculus/instincts/personal/domanda-al-capitano-non-a-theo.md`), e il
//! 21/08/2026 è stata saltata **cinque volte in una giornata** — da figure che
//! l'avevano letta. La diagnosi migliore la scrisse chi l'aveva appena commessa:
//! *chiedere direttamente a Theo funziona sempre, e per questo è la scorciatoia
//! che si prende da sola ogni volta che qualcuno ha fretta. Un difetto che
//! nessun controllo ferma è esattamente quello che serve scrivere.*
//!
//! CHI PASSA E CHI NO, e la riga di mezzo è quella che conta:
//! - una sessione **senza mestiere** passa: è una sessione che Theo ha aperto
//!   lui, e parlargli è il suo lavoro;
//! - il **capitano** passa: istruire una decisione a Theo è precisamente il suo
//!   mestiere, e toglierglielo spegnerebbe la catena invece di ripararla;
//! - ogni altra figura **non passa**, e si vede scritto a chi scrivere.
//!
//! NON NEGA IL LAVORO, NEGA LA PORTA SBAGLIATA. Il messaggio non dice «non si
//! chiede»: dice dove si chiede, col nome del capitano in carica già risolto,
//! perché il costo che fa prendere la scorciatoia è proprio il dover cercare.

use hook_io::Decision;

/// Il mestiere che può parlare direttamente a Theo.
const CAPTAIN: &str = "CAPITANO";

/// PURA: si può aprire un modulo? Dipende da chi chiede e da chi c'è.
///
/// `my_role` è il mestiere di chi chiede (`None` se non ne ha uno), `captain`
/// l'identificativo del capitano vivo (`None` se non c'è nessuno).
pub fn judge(tool: &str, my_role: Option<&str>, captain: Option<&str>) -> Decision {
    if tool != "AskUserQuestion" {
        return Decision::Pass;
    }
    let Some(role) = my_role else {
        return Decision::Pass; // nessun mestiere: è una sessione di Theo
    };
    // Il confronto è sul prefisso perché un ruolo può portare un commento
    // accanto — «MARINAIO suite-offerte» è un marinaio.
    if role.trim_start().starts_with(CAPTAIN) {
        return Decision::Pass;
    }
    Decision::Deny(match captain {
        Some(id) => format!(
            "questa domanda non va a Theo: sei {role} di guardia, e le decisioni \
             passano dal capitano. Il capitano in carica e' `{id}`: scrivigli con \
             SendMessage, mettendo la domanda, le strade che vedi e quale \
             sceglieresti.\n\
             Sono parole di Theo, non una convenzione: «io non dovrei neanche \
             presidiare il macchinista autorizzando permessi; dovrebbe farlo il \
             capitano, e il capitano riferire a me».\n\
             Se e' una scelta di prodotto, di spesa o una rimozione resta sua — ma \
             gliela porta il capitano, istruita.",
            role = role.trim(),
            id = id
        ),
        None => format!(
            "questa domanda non va a Theo: sei {role} di guardia. **Non c'e' \
             nessun capitano vivo adesso**, e non e' un motivo per scavalcare: \
             lascia la voce in coda (`~/.claude/state/plancia/segnalazioni/`) con \
             `stato: aperta` e `per: il capitano`. Il giro che sorveglia la coda \
             ne apre uno, ed e' il modo previsto perche' quel posto si riempia.",
            role = role.trim()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_question_tool_is_judged() {
        // Il gancio che ospita questo controllo gira su OGNI strumento: se
        // giudicasse anche gli altri, un errore qui fermerebbe tutta la nave.
        for tool in ["Bash", "Write", "Edit", "Read", "SendMessage", "Task"] {
            assert_eq!(judge(tool, Some("MACCHINISTA"), None), Decision::Pass, "{tool}");
        }
    }

    #[test]
    fn a_session_without_a_trade_is_theos_own() {
        assert_eq!(judge("AskUserQuestion", None, Some("aaaa1111")), Decision::Pass);
    }

    #[test]
    fn the_captain_may_ask_because_that_is_his_trade() {
        assert_eq!(judge("AskUserQuestion", Some("CAPITANO"), None), Decision::Pass);
        // Col commento accanto resta il capitano.
        assert_eq!(
            judge("AskUserQuestion", Some("CAPITANO di guardia"), None),
            Decision::Pass
        );
    }

    #[test]
    fn a_figure_on_watch_is_sent_to_the_captain_by_name() {
        let d = judge("AskUserQuestion", Some("MACCHINISTA"), Some("3131ff13"));
        let Decision::Deny(m) = d else { panic!("doveva negare: {d:?}") };
        // Il nome del capitano DEVE esserci: e' il costo di cercarlo che fa
        // prendere la scorciatoia.
        assert!(m.contains("3131ff13"), "manca il nome del capitano: {m}");
        assert!(m.contains("SendMessage"), "manca come scrivergli: {m}");
    }

    #[test]
    fn with_no_captain_the_answer_is_the_queue_not_theo() {
        let d = judge("AskUserQuestion", Some("MARINAIO"), None);
        let Decision::Deny(m) = d else { panic!("doveva negare: {d:?}") };
        assert!(m.contains("per: il capitano"), "manca dove lasciare la voce: {m}");
        assert!(
            !m.contains("scrivigli"),
            "non puo' mandare a scrivere a un capitano che non c'e': {m}"
        );
    }
}
