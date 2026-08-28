//! Quando un flusso è dovuto, e con che peso.
//!
//! PERCHÉ NASCE. Le dodici lavorazioni notturne sono diventate flussi il
//! 28/08/2026, e nella conversione hanno perso tre cose: **ogni quanto girano,
//! quanto pesano, e dove possono scrivere**. Non erano andate perdute per
//! distrazione: il formato del flusso non aveva un posto dove metterle, quindi
//! sono finite nella prosa della descrizione — dove nessun programma le legge.
//! Finché restano lì, un cron non si può convertire: si convertirebbe *che cosa*
//! fa, perdendo *quando* lo fa.
//!
//! IL GIUDIZIO QUI È PURO, E NON DEVE CHIEDERE L'ORA A NESSUNO. `is_due` prende
//! l'istante come argomento perché una decisione che legge l'orologio da sé si
//! può provare solo aspettando — e una prova che aspetta non si scrive, quindi
//! non si scrive la prova. Chi chiama l'ora la legge una volta e la passa.

use serde::{Deserialize, Serialize};

/// Ogni quanto un flusso deve girare.
///
/// DUE FORME, PERCHÉ DUE SONO QUELLE CHE ESISTONO su questa macchina: le
/// lavorazioni notturne, che vogliono un'ora del giorno, e i cron di sistema,
/// che vogliono un intervallo (la staffetta ogni 60 secondi, il registro dello
/// swap ogni 300). Una terza forma si aggiunge quando serve davvero: un
/// linguaggio di pianificazione completo scritto prima di avere il caso è la
/// cosa che poi nessuno usa e nessuno osa togliere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    /// Ogni N secondi dall'ultima corsa. Un flusso mai girato è dovuto subito.
    EverySeconds { seconds: u64 },
    /// Una volta al giorno, a partire da quell'ora locale.
    DailyAt { hour: u32, minute: u32 },
}

/// Quanto costa una corsa, dichiarato da chi scrive il flusso.
///
/// NON DECIDE NIENTE, OGGI, e va detto invece di lasciarlo credere: è un dato
/// che il motore riporta, non un freno che applica. Serve perché la
/// distinzione esisteva nelle lavorazioni notturne (leggere sotto il minuto,
/// pesanti fino a dodici) e perderla significa non poter più dire, dopo, perché
/// una notte è andata storta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    Light,
    Heavy,
}

/// Quando gira, quanto pesa, dove può scrivere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub recurrence: Recurrence,
    pub weight: Weight,
    /// Le cartelle dentro cui la lavorazione può scrivere, come le dichiarava
    /// la voce di coda da cui viene. Vuoto significa «non dichiarato», che è
    /// diverso da «nessun limite»: chi legge deve poter distinguere i due.
    #[serde(default)]
    pub perimeter: Vec<String>,
}

/// Il flusso è dovuto adesso?
///
/// `last_run` è l'istante dell'ultima corsa in secondi dall'epoca, `None` se non
/// è mai girato. **Un flusso mai girato è sempre dovuto**: la prima corsa non ha
/// un ritardo da aspettare, e trattarla come le altre vorrebbe dire che una
/// lavorazione nuova resta ferma fino al primo giro utile senza che nessuno
/// capisca perché.
///
/// L'ora del giorno si confronta sul **giorno locale**: `DailyAt` chiede «oggi è
/// già girata dopo quell'ora?», non «sono passate 24 ore». Le due domande
/// divergono ogni volta che una corsa slitta, ed è la prima quella che chi
/// scrive la lavorazione ha in mente.
pub fn is_due(schedule: &Schedule, last_run: Option<i64>, now: i64) -> bool {
    let Some(last) = last_run else {
        return true;
    };
    match schedule.recurrence {
        Recurrence::EverySeconds { seconds } => now.saturating_sub(last) >= seconds as i64,
        Recurrence::DailyAt { hour, minute } => {
            let today = start_of_local_day(now);
            let at = today + (hour as i64) * 3600 + (minute as i64) * 60;
            now >= at && last < at
        }
    }
}

/// La mezzanotte locale del giorno che contiene `now`.
///
/// LO SCARTO DAL FUSO SI RICAVA, non si chiede a una libreria: il workspace
/// tiene le dipendenze al minimo, e qui serve una cosa sola. `localtime_r` dà i
/// campi dell'ora locale; da quelli si torna indietro ai secondi trascorsi dalla
/// mezzanotte, e si sottraggono. Vale anche nei giorni in cui l'ora cambia,
/// perché lo scarto viene misurato **in quel giorno**, non assunto costante.
fn start_of_local_day(now: i64) -> i64 {
    let seconds_today = local_seconds_of_day(now);
    now - seconds_today
}

#[cfg(unix)]
fn local_seconds_of_day(now: i64) -> i64 {
    // `libc` non è fra le dipendenze, e per una chiamata sola non vale
    // aggiungerla: la dichiarazione sta qui, accanto all'uso.
    extern "C" {
        fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    }
    let mut out = Tm {
        zone: std::ptr::null(),
        ..Default::default()
    };
    // SAFETY: `now` è un intero valido e `out` è una struttura che vive per
    // tutta la chiamata; `localtime_r` scrive solo lì dentro.
    let filled = unsafe { localtime_r(&now, &mut out) };
    if filled.is_null() {
        // Nessuna ora locale: si ricade sull'UTC, che è sempre calcolabile.
        return now.rem_euclid(86_400);
    }
    (out.hour as i64) * 3600 + (out.min as i64) * 60 + out.sec as i64
}

#[cfg(not(unix))]
fn local_seconds_of_day(now: i64) -> i64 {
    now.rem_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every(seconds: u64) -> Schedule {
        Schedule {
            recurrence: Recurrence::EverySeconds { seconds },
            weight: Weight::Light,
            perimeter: Vec::new(),
        }
    }

    fn daily(hour: u32, minute: u32) -> Schedule {
        Schedule {
            recurrence: Recurrence::DailyAt { hour, minute },
            weight: Weight::Heavy,
            perimeter: vec!["~/.claude".to_string()],
        }
    }

    #[test]
    fn a_flow_that_never_ran_is_due_at_once() {
        assert!(is_due(&every(60), None, 1_000_000));
        assert!(is_due(&daily(3, 0), None, 1_000_000));
    }

    /// I due bracci dell'intervallo: un secondo prima no, al secondo esatto sì.
    /// La staffetta gira ogni 60 secondi, ed è questa la soglia che deve tenere.
    #[test]
    fn an_interval_is_due_only_once_the_seconds_have_passed() {
        let now = 1_000_000;
        assert!(!is_due(&every(60), Some(now - 59), now));
        assert!(is_due(&every(60), Some(now - 60), now));
        assert!(is_due(&every(60), Some(now - 6000), now));
    }

    /// L'ora del giorno chiede «oggi è già girata?», non «sono passate 24 ore».
    /// Il braccio che conta è il terzo: una corsa di ieri sera **dopo** l'ora di
    /// oggi non deve valere come la corsa di oggi.
    #[test]
    fn a_daily_flow_asks_whether_it_already_ran_today() {
        let midnight = start_of_local_day(1_800_000_000);
        let three_in_the_morning = midnight + 3 * 3600;
        let schedule = daily(3, 0);

        // Le due e mezza: l'ora non è ancora arrivata.
        assert!(!is_due(&schedule, Some(midnight - 100), midnight + 9000));
        // Le tre in punto, e l'ultima corsa è di ieri: dovuta.
        assert!(is_due(&schedule, Some(midnight - 100), three_in_the_morning));
        // Le quattro, ma è già girata alle tre e un minuto: non si ripete.
        assert!(!is_due(
            &schedule,
            Some(three_in_the_morning + 60),
            midnight + 4 * 3600
        ));
    }

    /// La forma su disco, che è un contratto con la finestra e con chi scrive i
    /// flussi a mano: se cambia, quei file smettono di caricarsi in silenzio.
    #[test]
    fn the_schedule_reads_back_exactly_as_it_is_written() {
        let text = r#"{
            "recurrence": {"kind": "daily_at", "hour": 3, "minute": 30},
            "weight": "heavy",
            "perimeter": ["~/.claude", "~/personal/sailor"]
        }"#;
        let parsed: Schedule = serde_json::from_str(text).unwrap();
        assert_eq!(
            parsed.recurrence,
            Recurrence::DailyAt {
                hour: 3,
                minute: 30
            }
        );
        assert_eq!(parsed.weight, Weight::Heavy);
        assert_eq!(parsed.perimeter.len(), 2);

        let again: Schedule = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(again, parsed);
    }

    /// Il perimetro assente non è il perimetro vuoto dichiarato, ma qui i due
    /// coincidono nella forma: quello che conta è che l'assenza non faccia
    /// fallire il caricamento di un flusso scritto prima che questo campo
    /// esistesse.
    #[test]
    fn an_older_flow_without_a_perimeter_still_loads() {
        let text = r#"{"recurrence": {"kind": "every_seconds", "seconds": 60}, "weight": "light"}"#;
        let parsed: Schedule = serde_json::from_str(text).unwrap();
        assert!(parsed.perimeter.is_empty());
    }
}
