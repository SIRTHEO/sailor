//! Una figura di guardia non apre un modulo a Theo: la domanda va in coda.
//!
//! PERCHÉ ESISTE, con le parole di chi ha fondato la regola. Theo, il
//! 21/08/2026: *«io non dovrei neanche presidiare il macchinista autorizzando
//! permessi»*. E la sera dello stesso giorno, dopo averne risolte tre a mano:
//! **«le ho risolte io ma non dovrebbe essere così»**.
//!
//! DOVE VA LA DOMANDA, ADESSO. Fino al 24/08/2026 questo freno mandava al
//! *capitano*, e gli istruiva pure il nome. Quella figura è ritirata dal
//! 23/08/2026, quindi il freno spediva a un indirizzo che non esiste: peggio di
//! non negare, perché una strada sbagliata la si percorre. La forma nuova è
//! quella decisa lo stesso giorno — **coordina il file, non un coordinatore**:
//! la domanda si lascia scritta nella coda di bordo con `per: Theo`, e chi
//! chiede va avanti col resto del proprio lavoro.
//!
//! LA PROSA NON BASTA, ED È IL PUNTO. La regola era scritta nei mandati, nelle
//! schede dei mestieri e perfino in una nota iniettata a ogni sessione, e il
//! 21/08/2026 è stata saltata **cinque volte in una giornata** — da figure che
//! l'avevano letta. La diagnosi migliore la scrisse chi l'aveva appena commessa:
//! *chiedere direttamente a Theo funziona sempre, e per questo è la scorciatoia
//! che si prende da sola ogni volta che qualcuno ha fretta. Un difetto che
//! nessun controllo ferma è esattamente quello che serve scrivere.*
//!
//! CHI PASSA E CHI NO:
//! - una sessione **senza mestiere** passa: è una sessione che Theo ha aperto
//!   lui, e parlargli è il suo lavoro;
//! - ogni sessione **con un mestiere** non passa, e si vede scritto dove
//!   lasciare la domanda.
//!
//! NON NEGA IL LAVORO, NEGA LA PORTA SBAGLIATA. Il messaggio non dice «non si
//! chiede»: dice dove si chiede, e con quale forma, perché il costo che fa
//! prendere la scorciatoia è proprio il dover cercare.

use hook_io::Decision;

/// PURA: si può aprire un modulo? Dipende solo da chi chiede.
///
/// `my_role` è il mestiere di chi chiede (`None` se non ne ha uno).
pub fn judge(tool: &str, my_role: Option<&str>) -> Decision {
    if tool != "AskUserQuestion" {
        return Decision::Pass;
    }
    let Some(role) = my_role else {
        return Decision::Pass; // nessun mestiere: è una sessione di Theo
    };
    Decision::Deny(format!(
        "questa domanda non va a Theo con un modulo: sei {role} di guardia. \
         Lasciala nella coda di bordo (`~/.claude/state/plancia/segnalazioni/`), \
         un file nuovo con `stato: aperta` e `per: Theo`, e dentro la domanda, le \
         strade che vedi e quale sceglieresti. Poi VAI AVANTI col resto del tuo \
         lavoro: la voce resta visibile a chi apre una sessione, non serve che tu \
         aspetti.\n\
         Se e' una scelta di prodotto, di spesa o una rimozione la decisione \
         resta di Theo — ma gliela porta la coda, istruita.",
        role = role.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_question_tool_is_judged() {
        // Il gancio che ospita questo controllo gira su OGNI strumento: se
        // giudicasse anche gli altri, un errore qui fermerebbe tutta la nave.
        for tool in ["Bash", "Write", "Edit", "Read", "SendMessage", "Task"] {
            assert_eq!(judge(tool, Some("BUILDER")), Decision::Pass, "{tool}");
        }
    }

    #[test]
    fn a_session_without_a_trade_is_theos_own() {
        assert_eq!(judge("AskUserQuestion", None), Decision::Pass);
    }

    #[test]
    fn a_trade_on_watch_is_sent_to_the_queue() {
        let d = judge("AskUserQuestion", Some("BUILDER"));
        let Decision::Deny(m) = d else { panic!("doveva negare: {d:?}") };
        // I due pezzi senza i quali il messaggio non e' azionabile: dove si
        // scrive, e con quale destinatario.
        assert!(m.contains("segnalazioni"), "manca dove lasciare la voce: {m}");
        assert!(m.contains("per: Theo"), "manca il destinatario: {m}");
    }

    #[test]
    fn no_retired_figure_is_ever_named() {
        // IL BRACCIO CHE SA FALLIRE. Prima del 24/08/2026 questo messaggio
        // mandava a scrivere al capitano, col suo nome risolto: una figura
        // ritirata il giorno prima. Un freno che indirizza a chi non c'e'
        // costa piu' del difetto che ripara, perche' l'indirizzo sembra buono.
        // I mestieri veri e basta: il messaggio ripete il ruolo di CHI CHIEDE,
        // quindi un file di ruolo rimasto `CAPITANO` farebbe comparire quella
        // parola senza che nessuno ci venga mandato.
        for role in ["BUILDER", "MEASURER", "INVESTIGATOR", "REVIEWER"] {
            let d = judge("AskUserQuestion", Some(role));
            let Decision::Deny(m) = d else { continue };
            let lower = m.to_lowercase();
            assert!(!lower.contains("capitano"), "nomina il capitano: {m}");
            assert!(!lower.contains("macchinista"), "nomina il macchinista: {m}");
        }
    }

    #[test]
    fn even_the_retired_captain_no_longer_gets_a_free_pass() {
        // La riga di mezzo di prima: il capitano passava, perche' istruire una
        // decisione era il suo mestiere. Non essendoci piu' quel mestiere, un
        // file di ruolo rimasto sul disco non deve aprire la porta.
        assert!(matches!(
            judge("AskUserQuestion", Some("CAPITANO")),
            Decision::Deny(_)
        ));
    }
}
