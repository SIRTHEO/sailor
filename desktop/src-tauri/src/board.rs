//! Quello che la plancia sapeva dire e la finestra no.
//!
//! **PERCHÉ ESISTE QUESTO MODULO.** `sailor ui` serve da mesi una pagina su
//! `127.0.0.1:47831` con cinque sezioni. La finestra ne copre due — i flussi, e
//! da stasera «cosa sta succedendo adesso». Le altre tre — il riepilogo di
//! oggi, la storia delle esecuzioni, cosa è installato — esistono solo di là.
//! Finché è così, togliere la plancia non è semplificare: è sottrarre.
//!
//! È LA LEZIONE PIÙ CARA DELLA RICOGNIZIONE DEL 31/08/2026. Airflow 3 ha tolto
//! Gantt, Calendar, Duration e Landing Times riscrivendo l'interfaccia, e ha
//! dovuto rimetterle otto mesi dopo perché la gente non aggiornava — con
//! l'ammissione del manutentore: «ci è finito il tempo per reimplementare la
//! vista calendario». Jira ha tolto voci di barra e ha lasciato il ripristino
//! disponibile per tre settimane. In tre casi su tre è il gruppo di prodotto ad
//! ammettere il difetto per iscritto, mesi dopo. La regola che ne segue è
//! meccanica: prima di sostituire una vista si elenca che cosa permetteva di
//! fare, e si verifica voce per voce che la nuova lo permetta ancora.
//!
//! **I CONTI NON SI RIFANNO QUI**, e nemmeno nella tela. `build_executions` è
//! la stessa funzione che serve la plancia: due somme scritte in due posti
//! darebbero due cifre e nessuno saprebbe quale credere. Vale anche per
//! l'inventario — `default_roots` sta nel crate proprio perché la riga di
//! comando e la pagina dicano lo stesso numero sulla stessa macchina.

use inventory::{collect_survey, default_roots, Inventory};
use serde::Serialize;
use std::collections::BTreeMap;
use ui::dashboard::{build_executions, ExecutionView};
use ui::gather::{default_ledger_dir, gather};

/// Il riepilogo di una giornata.
///
/// **PORTA ANCHE CIÒ CHE NON HA POTUTO MISURARE**, ed è la parte che di solito
/// manca. Nella ricognizione del 31/08/2026, Langfuse mostrava 4.509 token dove
/// erano 2.265 (trattava il totale come completamento, «known issue»
/// confermato), LangSmith gonfiava di 75-200 volte con le immagini e non sa
/// contare la cache dei prompt, Phoenix ha il costo giusto nel database e
/// sbagliato a schermo — parole di uno del gruppo. Un numero mostrato con
/// autorità e sbagliato è peggio di un numero assente. `unmeasured` e
/// `unpriced` dicono quante chiamate non hanno portato token o prezzo, così
/// una cifra bassa si legge per quello che è: bassa, o incompleta.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct DaySummary {
    /// Vero se il deposito esiste. Falso non è zero: è «non lo so».
    pub ledger_present: bool,
    pub runs: usize,
    pub went: usize,
    pub broke: usize,
    pub still_open: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_micros: i64,
    /// Chiamate al modello che non hanno riportato token.
    pub unmeasured: usize,
    /// Chiamate al modello che non hanno riportato un prezzo.
    pub unpriced: usize,
    /// Token visti, per modello.
    pub tokens_by_model: BTreeMap<String, u64>,
}

/// La storia delle esecuzioni, dalla più recente.
///
/// **NON È «ADESSO» CON PIÙ RIGHE.** «Adesso» chiede al deposito ciò che è
/// aperto e non conosce il passato; questa porta tutto, chiuso compreso, ed è
/// la vista in cui si cerca un difetto che si ripete — la stessa corsa caduta
/// tre volte di fila si vede solo qui.
///
/// Un deposito che non esiste dà un elenco vuoto, non un errore: è una macchina
/// su cui non è ancora girato niente.
#[tauri::command]
pub(crate) fn execution_history() -> Result<Vec<ExecutionView>, String> {
    let ledger_dir = default_ledger_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let data = gather(&ledger_dir).map_err(|error| format!("non riesco a leggere il deposito: {error}"))?;
    let Some(data) = data else {
        return Ok(Vec::new());
    };
    let mut executions = build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, now);
    // La più recente in cima: qui si guarda indietro, e si parte da ieri.
    executions.reverse();
    Ok(executions)
}

/// Il riepilogo delle corse cominciate da un certo istante in poi.
///
/// **L'ISTANTE LO PORTA CHI CHIAMA, E NON È PIGRIZIA.** «Oggi» è un giorno di
/// calendario locale, e il fuso orario lo sa la finestra — che gira dentro un
/// sistema che glielo dice — mentre qui servirebbe una libreria intera per
/// riscoprirlo. Quello che non si delega è la somma: sta qui, una volta sola,
/// perché è la cifra che una persona guarda per decidere.
#[tauri::command]
pub(crate) fn day_summary(since: i64) -> Result<DaySummary, String> {
    let ledger_dir = default_ledger_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let data = gather(&ledger_dir).map_err(|error| format!("non riesco a leggere il deposito: {error}"))?;
    let Some(data) = data else {
        // Deposito assente: `ledger_present` resta falso, e ogni conteggio è
        // zero. Chi legge deve poter distinguere «non è girato niente» da
        // «non ho potuto guardare», e quel campo è tutta la differenza.
        return Ok(DaySummary::default());
    };
    let executions = build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, now);
    let mut summary = DaySummary {
        ledger_present: true,
        ..DaySummary::default()
    };
    for execution in executions.iter().filter(|run| run.started_at >= since) {
        summary.runs += 1;
        let open = !execution.steps_open.is_empty() || matches!(execution.status.as_str(), "running" | "open");
        // ROTTA È PIÙ FORTE DI APERTA. Una corsa con un passo caduto e un altro
        // ancora in volo è un guasto che sta ancora bruciando, non un lavoro in
        // corso: contarla fra gli aperti la toglierebbe dall'occhio di chi
        // guarda i guasti.
        let broke = execution.error.is_some()
            || execution.steps_broke > 0
            || matches!(execution.status.as_str(), "failed" | "broke" | "error");
        if broke {
            summary.broke += 1;
        } else if open {
            summary.still_open += 1;
        } else if execution.status == "succeeded" {
            summary.went += 1;
        }
        summary.input_tokens += execution.tokens.input_tokens;
        summary.output_tokens += execution.tokens.output_tokens;
        summary.cached_tokens += execution.tokens.cached_tokens;
        summary.cache_write_tokens += execution.tokens.cache_write_tokens;
        summary.cost_micros += execution.tokens.cost_micros;
        summary.unmeasured += execution.tokens.calls_without_tokens;
        summary.unpriced += execution.tokens.calls_without_cost;
        for (model, tokens) in &execution.tokens_by_model {
            *summary.tokens_by_model.entry(model.clone()).or_insert(0) +=
                tokens.input_tokens + tokens.output_tokens + tokens.cached_tokens + tokens.cache_write_tokens;
        }
    }
    Ok(summary)
}

/// Cosa è installato su questa macchina: competenze, agenti, comandi, regole,
/// ganci.
///
/// **PORTA ANCHE DOVE HA GUARDATO.** `Inventory` non espone solo le voci ma le
/// radici davvero attraversate, ed è deliberato: un elenco che non dice dove ha
/// cercato non si può smentire. La finestra le mostra, come la plancia.
#[tauri::command]
pub(crate) fn machine_inventory() -> Inventory {
    collect_survey(&default_roots(ledger::sailor_home().as_deref()))
}
