//! Il freno della catena: quando una staffetta ha smesso di convergere.
//!
//! La staffetta guarda **il momento** — questa sessione è piena, ha consegnato,
//! il suo pannello è vivo — e nessuno dei suoi controlli guarda **la storia**.
//! Una catena che nasce, non conclude niente, consegna e viene sostituita è
//! quindi indistinguibile da una che lavora: 12 rigenerazioni fra il 17/08/2026
//! 11:12 e il 18/08 09:17, e nessun tetto sopra.
//!
//! Qui sta la decisione pura, con la storia come dato: `chain_verdict` non legge
//! disco né ambiente, così i casi limite si scrivono senza preparare una `HOME`.
//!
//! LA MISURA ESISTEVA GIÀ. `relay::progress` conta turni e scritture a ogni
//! rigenerazione, e finiva in una riga di registro che non legge nessuno. Questo
//! modulo la collega a una decisione: è il lavoro, non inventare una metrica.

use crate::handoff::round_half_to_even;

/// Un anello: una rigenerazione già avvenuta su questo albero di lavoro.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChainLink {
    pub session: String,
    /// Quando la rigenerazione è avvenuta, in secondi dall'epoca.
    pub at: f64,
    pub turns: u64,
    pub writes: u64,
    /// La consegna che il successore ha ricevuto in mano.
    pub handoff: String,
}

/// Tetto di rigenerazioni in una catena. **Dieci, e non tre.**
///
/// La catena più lunga misurata sul registro del 17-18/08/2026 ne ha **quattro**
/// (`-Users-theo-orca-general`, 20:46 → 02:08), quindi un tetto a dieci non
/// tocca il lavoro normale e ferma comunque la fuga: le 22 sessioni in mezz'ora
/// del 17/08 sarebbero morte al decimo minuto.
pub const MAX_LINKS: usize = 10;

/// Età massima di una catena viva: un giorno.
pub const MAX_AGE_SEC: f64 = 86_400.0;

/// Silenzio dopo il quale la catena è finita e la prossima riparte da capo.
///
/// SEI ORE, ed è il numero che rende onesto l'identificatore. La catena si
/// riconosce dall'albero di lavoro, che è l'unico ponte che sopravvive alla
/// sostituzione; senza scadenza, due lavorazioni diverse dello stesso albero a
/// giorni di distanza si sommerebbero in una catena finta. Sul registro citato
/// sopra, sei ore separano esattamente le due catene di `suite` (23:53 → 09:17)
/// e lasciano intere quelle di `general`.
pub const IDLE_RESET_SEC: f64 = 21_600.0;

/// Quanti anelli sterili di fila bastano a dire che la catena gira a vuoto.
pub const STALL_LINKS: usize = 3;

/// I quattro numeri che decidono, tutti sovrascrivibili da chi chiama.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainLimits {
    pub max_links: usize,
    pub max_age_sec: f64,
    pub idle_reset_sec: f64,
    pub stall_links: usize,
}

impl Default for ChainLimits {
    fn default() -> Self {
        ChainLimits {
            max_links: MAX_LINKS,
            max_age_sec: MAX_AGE_SEC,
            idle_reset_sec: IDLE_RESET_SEC,
            stall_links: STALL_LINKS,
        }
    }
}

/// Cosa fare della catena prima di rigenerare ancora.
#[derive(Debug, PartialEq, Eq)]
pub enum ChainVerdict {
    /// Si può rigenerare: la catena prosegue.
    Go,
    /// La catena precedente è finita da un pezzo: si riparte da zero anelli.
    Reset,
    /// Un freno morde: non si rigenera, e qualcuno deve saperlo.
    Stop,
}

/// Un anello che non ha prodotto niente, per i due segnali che si hanno.
///
/// SERVONO ENTRAMBI, e il verso della congiunzione è il comportamento. Le
/// scritture da sole mentirebbero: con la modalità automatica i file si toccano
/// da Bash, e `progress` conta solo `Write`/`Edit`/`MultiEdit` — una sessione
/// produttiva può risultare a zero. La consegna invariata da sola mentirebbe
/// nell'altro verso: chi scrive codice per due ore senza consegnare ha lavorato
/// eccome. Sterile è chi non ha prodotto in **nessuno** dei due sensi.
///
/// Una consegna vuota è «non lo so», non «uguale»: nel dubbio l'anello vale come
/// produttivo, che è l'errore che costa un giro invece di una catena viva.
fn is_sterile(link: &ChainLink, previous: &ChainLink) -> bool {
    link.writes == 0 && !link.handoff.is_empty() && link.handoff == previous.handoff
}

/// Quanti anelli sterili chiudono la catena, contati dalla coda.
pub fn sterile_tail(links: &[ChainLink]) -> usize {
    let mut count = 0;
    for i in (1..links.len()).rev() {
        if !is_sterile(&links[i], &links[i - 1]) {
            break;
        }
        count += 1;
    }
    count
}

/// Il verdetto sulla catena, e la riga che lo spiega a chi la legge.
///
/// L'ORDINE DEI CONTROLLI È IL COMPORTAMENTO. La scadenza per inattività sta
/// **prima** dei due tetti: una catena lunga ma ferma da otto ore non è una
/// fuga, è finita, e giudicarla col suo passato bloccherebbe il lavoro di
/// domani con i numeri di ieri.
pub fn chain_verdict(links: &[ChainLink], now: f64, limits: &ChainLimits) -> (ChainVerdict, String) {
    let Some(last) = links.last() else {
        return (ChainVerdict::Go, String::new());
    };
    let idle = now - last.at;
    if idle >= limits.idle_reset_sec {
        return (
            ChainVerdict::Reset,
            format!(
                "catena precedente ferma da {} h: riparte da capo",
                round_half_to_even(idle / 3600.0)
            ),
        );
    }
    if links.len() >= limits.max_links {
        return (
            ChainVerdict::Stop,
            format!(
                "tetto di {} rigenerazioni raggiunto su questo albero",
                limits.max_links
            ),
        );
    }
    let age = now - links[0].at;
    if age >= limits.max_age_sec {
        return (
            ChainVerdict::Stop,
            format!(
                "catena aperta da {} h ({} rigenerazioni), oltre il tetto di {} h",
                round_half_to_even(age / 3600.0),
                links.len(),
                round_half_to_even(limits.max_age_sec / 3600.0)
            ),
        );
    }
    let sterile = sterile_tail(links);
    if sterile >= limits.stall_links {
        return (
            ChainVerdict::Stop,
            format!(
                "{sterile} rigenerazioni di fila senza scritture e senza una \
                 consegna nuova: la catena gira a vuoto"
            ),
        );
    }
    (ChainVerdict::Go, format!("{} anelli", links.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(at: f64, writes: u64, handoff: &str) -> ChainLink {
        ChainLink {
            session: "abcdefgh".into(),
            at,
            turns: 100,
            writes,
            handoff: handoff.into(),
        }
    }

    #[test]
    fn an_empty_chain_never_brakes() {
        let (v, _) = chain_verdict(&[], 1000.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Go);
    }

    #[test]
    fn a_working_chain_keeps_going() {
        // I quattro anelli veri della catena più lunga misurata: ognuno ha
        // scritto, e ognuno ha consegnato un documento suo.
        let links = [
            link(0.0, 52, "/a.md"),
            link(3600.0, 31, "/b.md"),
            link(7200.0, 12, "/c.md"),
            link(10_800.0, 8, "/d.md"),
        ];
        let (v, _) = chain_verdict(&links, 12_000.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Go);
    }

    #[test]
    fn silence_ends_the_chain_instead_of_condemning_it() {
        // Dodici anelli sterili — cioè oltre il tetto e in pieno stallo — ma
        // l'ultimo è di ieri: la catena di oggi nasce pulita lo stesso.
        //
        // Il numero è sopra `MAX_LINKS` di proposito: è il caso che distingue
        // «la scadenza si valuta per prima» da «si valuta dopo i tetti», e con
        // nove anelli il mutante che inverte i due controlli sopravviveva.
        let links: Vec<ChainLink> = (0..12).map(|i| link(i as f64 * 60.0, 0, "/same.md")).collect();
        let (v, why) = chain_verdict(&links, 100_000.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Reset, "{why}");
    }

    #[test]
    fn the_link_ceiling_stops_a_runaway() {
        // Una rigenerazione al minuto: è la forma della fuga del 17/08/2026.
        let links: Vec<ChainLink> = (0..MAX_LINKS)
            .map(|i| link(i as f64 * 60.0, 5, &format!("/{i}.md")))
            .collect();
        let (v, why) = chain_verdict(&links, MAX_LINKS as f64 * 60.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Stop);
        assert!(why.contains("tetto di 10"), "{why}");
    }

    #[test]
    fn a_chain_older_than_a_day_stops() {
        let links = [
            link(0.0, 5, "/a.md"),
            link(43_200.0, 5, "/b.md"),
            link(86_000.0, 5, "/c.md"),
        ];
        let (v, why) = chain_verdict(&links, 87_000.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Stop, "{why}");
        assert!(why.contains("aperta da"), "{why}");
    }

    #[test]
    fn three_barren_links_are_a_stall() {
        // Stessa consegna, zero scritture: la catena si rigenera senza produrre.
        let links = [
            link(0.0, 40, "/vecchia.md"),
            link(600.0, 0, "/ferma.md"),
            link(1200.0, 0, "/ferma.md"),
            link(1800.0, 0, "/ferma.md"),
            link(2400.0, 0, "/ferma.md"),
        ];
        assert_eq!(sterile_tail(&links), 3);
        let (v, why) = chain_verdict(&links, 2500.0, &ChainLimits::default());
        assert_eq!(v, ChainVerdict::Stop);
        assert!(why.contains("gira a vuoto"), "{why}");
    }

    #[test]
    fn one_write_is_enough_to_not_be_barren() {
        // MUTANTE: con la congiunzione girata in `or`, questa catena si ferma —
        // ed è una catena che lavora senza consegnare, cioè il caso normale di
        // chi sta ancora scrivendo codice.
        let links = [
            link(0.0, 40, "/x.md"),
            link(600.0, 3, "/x.md"),
            link(1200.0, 3, "/x.md"),
            link(1800.0, 3, "/x.md"),
        ];
        assert_eq!(sterile_tail(&links), 0);
        assert_eq!(
            chain_verdict(&links, 1900.0, &ChainLimits::default()).0,
            ChainVerdict::Go
        );
    }

    #[test]
    fn a_new_handoff_is_enough_to_not_be_barren() {
        // L'altro verso dello stesso mutante: zero scritture perché i file si
        // toccano da Bash, ma ogni anello lascia una consegna sua.
        let links = [
            link(0.0, 0, "/a.md"),
            link(600.0, 0, "/b.md"),
            link(1200.0, 0, "/c.md"),
            link(1800.0, 0, "/d.md"),
        ];
        assert_eq!(sterile_tail(&links), 0);
        assert_eq!(
            chain_verdict(&links, 1900.0, &ChainLimits::default()).0,
            ChainVerdict::Go
        );
    }

    #[test]
    fn an_unknown_handoff_counts_as_work() {
        // La consegna vuota è «non ho trovato niente», non «la stessa di prima».
        let links = [
            link(0.0, 0, ""),
            link(600.0, 0, ""),
            link(1200.0, 0, ""),
            link(1800.0, 0, ""),
        ];
        assert_eq!(sterile_tail(&links), 0);
    }

    #[test]
    fn a_productive_link_breaks_the_barren_run() {
        // Il conteggio è sulla coda, non sul totale: due sterili, poi uno che
        // lavora, poi due sterili non fanno quattro.
        let links = [
            link(0.0, 0, "/x.md"),
            link(600.0, 0, "/x.md"),
            link(1200.0, 9, "/x.md"),
            link(1800.0, 0, "/x.md"),
            link(2400.0, 0, "/x.md"),
        ];
        assert_eq!(sterile_tail(&links), 2);
        assert_eq!(
            chain_verdict(&links, 2500.0, &ChainLimits::default()).0,
            ChainVerdict::Go
        );
    }
}
