//! I tre casi di una spesa, interrogati da soli.
//!
//! **PERCHÉ UNA PROVA A PARTE, E NON SOLO QUELLA DEL COMANDO.** `Spend`
//! documenta tre casi dal giorno in cui è nato, e per tutto quel tempo l'unico
//! modo di interrogarli era `is_complete()`. Una regola che nessuno interroga da
//! sola vive solo dentro chi la usa: quando `sailor flow cost` stampava
//! «1,6674» per una corsa da 7,2080 dollari, nessuna prova era rossa — la
//! distinzione era giusta nel motore e non arrivava a chi legge. Qui la regola
//! ha una prova sua, e chi scriverà il prossimo lettore la trova già scritta.

use flow::{CostReading, Spend};

/// Come il deposito riassume una corsa: quante chiamate, quanto costo noto, e
/// quante di quelle chiamate non hanno detto niente.
fn spent(micros: i64, calls: i64, calls_without_cost: i64) -> Spend {
    Spend {
        micros,
        calls,
        calls_without_cost,
        dearest_micros: None,
    }
}

/// Il caso facile, e serve che ci sia: senza, una lettura che si dichiarasse
/// sempre incompleta passerebbe le altre due prove.
#[test]
fn every_call_measured_reads_as_the_total() {
    assert_eq!(
        spent(1_667_400, 4, 0).reading(),
        CostReading::Exact(1_667_400)
    );
}

/// **UNA SOLA CHIAMATA MUTA CAMBIA IL VERSO DELLA LETTURA.** Sono i numeri veri
/// della corsa consegnata dell'A/B del 31/08/2026: quattro chiamate, tre senza
/// costo, e la quarta da 1,6674 dollari che il comando presentava come il
/// totale di una corsa costata 7,2080.
#[test]
fn one_unmeasured_call_turns_the_total_into_a_floor() {
    assert_eq!(
        spent(1_667_400, 4, 3).reading(),
        CostReading::AtLeast {
            known_micros: 1_667_400,
            calls: 4,
            calls_without_cost: 3,
        },
        "con una chiamata muta il numero è un pavimento, non una somma"
    );
}

/// **LA SPESA A ZERO NON SI CONFONDE CON LA SPESA IGNOTA**, ed è il terzo caso.
/// Le due corse hanno lo stesso `micros`; solo una ha davvero speso zero.
#[test]
fn nothing_spent_and_nothing_known_are_two_different_readings() {
    assert_eq!(spent(0, 2, 0).reading(), CostReading::Nothing);
    assert_eq!(
        spent(0, 2, 2).reading(),
        CostReading::AtLeast {
            known_micros: 0,
            calls: 2,
            calls_without_cost: 2,
        },
        "nessuna misura non è «ha speso zero»: sono due corse diverse"
    );
}

/// **IL PONTE FRA IL BOOLEANO E I TRE CASI.** La lettura deve passare da
/// `is_complete()`, non da un secondo confronto scritto accanto: due regole per
/// lo stesso fatto divergono, e a divergere sarebbe quella che una persona
/// legge.
///
/// **QUESTA PROVA NON PRENDE IL MUTANTE CHE CONTA, E VA DETTO.** Con
/// `is_complete()` sempre vero resta **verde**: confronta due cose che si
/// muovono insieme, quindi si conferma da sola. A prendere quel mutante sono le
/// due prove qui sopra, che scrivono il caso atteso a mano. Questa serve a
/// un'altra domanda — «la lettura e il booleano sono ancora la stessa regola?» —
/// e diventa rossa il giorno che qualcuno riscrive `reading()` con un confronto
/// suo.
#[test]
fn the_reading_agrees_with_what_the_engine_calls_complete() {
    for spend in [
        spent(0, 0, 0),
        spent(500, 1, 0),
        spent(500, 3, 2),
        spent(0, 1, 1),
    ] {
        let reads_as_a_floor = matches!(spend.reading(), CostReading::AtLeast { .. });
        assert_eq!(
            reads_as_a_floor,
            !spend.is_complete(),
            "un pavimento e un totale incompleto sono la stessa cosa detta due volte: {spend:?}"
        );
    }
}
