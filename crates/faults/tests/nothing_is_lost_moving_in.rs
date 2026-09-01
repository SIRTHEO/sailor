//! La migrazione non perde niente, e il numero non lo sceglie più chi scrive.
//!
//! **LA PROVA CHE CONTA È IL GIRO COMPLETO.** Leggere la tabella, metterla nel
//! deposito, riscriverla, e confrontarla riga per riga con l'originale. È
//! l'unico modo di sapere che nessuna delle voci è caduta per strada: una
//! migrazione che ne perde una e non lo dice è esattamente il genere di cosa che
//! quella tabella esiste per registrare — e nessuno la ricontrollerebbe, perché
//! la fonte sarebbe già stata cancellata.

use faults::{Draft, Fault, Faults};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "prova-guasti-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("la cartella di prova");
    dir.join(faults::FAULTS_FILE)
}

fn table() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/faults")
        .join("docs/guasti-incontrati.md");
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", file.display()))
}

/// **NIENTE SI PERDE ENTRANDO.** Le righe vere della tabella, portate dentro e
/// riscritte, devono tornare identiche — non «equivalenti»: identiche, perché
/// la sfumatura di una cella è l'informazione.
#[test]
fn every_row_survives_the_move_word_for_word() {
    let source = table();
    let read = faults::parse(&source);
    assert!(
        read.len() > 40,
        "la tabella si è svuotata sotto i piedi della migrazione: {} righe",
        read.len()
    );

    let store = Faults::open(scratch("giro-completo")).expect("aprire");
    for fault in &read {
        store.restore(fault).expect("rimettere la riga col suo numero");
    }

    let back = store.all().expect("rileggere");
    assert_eq!(
        back.len(),
        read.len(),
        "sono entrate {} righe e ne sono uscite {}",
        read.len(),
        back.len()
    );

    // **SI CONFRONTA PER NUMERO, NON PER POSIZIONE.** Nel markdown le righe non
    // stanno in ordine di numero — l'ordine del file è una scelta di chi lo
    // legge, non un dato — e confrontarle a coppie posizionali direbbe «cambiata»
    // di una riga identica scritta due righe più giù. Ciò che deve sopravvivere
    // è il contenuto di ogni voce, non il posto che occupava.
    let rewritten = faults::render(&back);
    let mut by_number: std::collections::BTreeMap<i64, &str> = std::collections::BTreeMap::new();
    for line in rewritten.lines() {
        let number: i64 = line
            .trim_matches('|')
            .split(" | ")
            .next()
            .and_then(|first| first.trim().parse().ok())
            .expect("ogni riga resa comincia col suo numero");
        by_number.insert(number, line);
    }

    let mut seen = 0usize;
    for line in source.lines().map(str::trim) {
        let Some(number) = line
            .strip_prefix('|')
            .and_then(|rest| rest.split(" | ").next())
            .and_then(|first| first.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let now = by_number
            .get(&number)
            .unwrap_or_else(|| panic!("il guasto {number} non è uscito dal deposito"));
        assert_eq!(
            &line, now,
            "il guasto {number} è cambiato attraversando il deposito"
        );
        seen += 1;
    }
    assert_eq!(seen, read.len(), "non tutte le righe sono state confrontate");
}

/// **IL NUMERO LO ASSEGNA IL DEPOSITO, E DUE CHIAMATE NE PRENDONO DUE.**
///
/// È la chiusura del guasto 42. Finché il numero lo sceglieva chi scriveva
/// guardando l'ultima riga di un file, due rami che non si vedono ne prendevano
/// uno uguale: l'01/09/2026 è successo tre volte in un pomeriggio, col 43, il 47
/// e il 48, e ogni volta si è scoperto alla fusione. Nessuna prova poteva
/// impedirlo, perché una prova guarda un ramo alla volta.
#[test]
fn the_store_hands_out_the_number_and_never_the_same_one_twice() {
    let store = Faults::open(scratch("numeri")).expect("aprire");
    let draft = |what: &str| Draft {
        happened_on: "01/09".to_owned(),
        what_happened: what.to_owned(),
        how_it_showed: "eseguendolo".to_owned(),
        what_would_prevent: "una prova che nasce rossa".to_owned(),
        status: "**aperto**".to_owned(),
    };

    let first = store.record(&draft("il primo")).expect("registrare");
    let second = store.record(&draft("il secondo")).expect("registrare");

    assert_eq!(first.number, 1);
    assert_eq!(second.number, 2, "il secondo non riceve il numero del primo");
    assert_ne!(
        first.number, second.number,
        "due guasti diversi non possono avere lo stesso numero: è ciò per cui \
         il numero è uscito dalle mani di chi scrive"
    );
}

/// Il conto degli aperti si **calcola**, e «chiuso in parte» conta come aperto.
/// Era sbagliato in quattro documenti su quattro il 31/08/2026, perché lo
/// ricopiava una persona; qui non esiste più un secondo posto dove ricopiarlo.
#[test]
fn a_half_closed_fault_still_counts_as_open() {
    let store = Faults::open(scratch("conto")).expect("aprire");
    for status in [
        "**aperto**",
        "**aperto** — le difese di procedura sono in vigore, il codice no",
        "**chiuso in parte** il 01/09, riaperto il 02/09",
        "**chiuso** il 01/09 — con mutante",
    ] {
        store
            .record(&Draft {
                happened_on: "01/09".to_owned(),
                what_happened: "qualcosa".to_owned(),
                how_it_showed: "eseguendolo".to_owned(),
                what_would_prevent: "una prova".to_owned(),
                status: status.to_owned(),
            })
            .expect("registrare");
    }

    assert_eq!(
        store.still_open().expect("contare"),
        3,
        "«aperto» con una sfumatura in più resta aperto, e «chiuso in parte» \
         anche: uno stato di mezzo racconta quale metà è fatta, non toglie la \
         riga dal conto"
    );
}

/// Cambiare stato è l'unica cosa che a un guasto succede dopo, e un numero che
/// non esiste è un errore con un nome invece di un silenzio.
#[test]
fn closing_a_fault_that_does_not_exist_says_so() {
    let store = Faults::open(scratch("ignoto")).expect("aprire");
    let refused = store
        .set_status(99, "**chiuso** oggi")
        .expect_err("un guasto che non c'è non si chiude");
    assert!(refused.to_string().contains("99"), "{refused}");
}

/// Un deposito scritto da un binario più nuovo si riconosce **per nome** invece
/// di sembrare rotto: è il guasto che l'01/09/2026 ha fermato mezza giornata di
/// lavoro con «unsupported projection schema version 8».
#[test]
fn a_newer_store_says_it_is_newer_and_not_broken() {
    let path = scratch("piu-nuovo");
    Faults::open(&path).expect("crearlo");
    let connection = rusqlite::Connection::open(&path).expect("riaprirlo a mano");
    connection
        .pragma_update(None, "user_version", 99_i64)
        .expect("alzare la versione");
    drop(connection);

    let said = match Faults::open(&path) {
        Ok(_) => panic!("un deposito più nuovo non deve aprirsi"),
        Err(refused) => refused.to_string(),
    };
    assert!(said.contains("99"), "{said}");
    assert!(
        said.contains("non è rotto"),
        "chi legge deve capire che è più nuovo, non guasto: {said}"
    );
}

/// La forma delle sei colonne è quella che la tabella aveva, e **una voce senza
/// «cosa lo impedirebbe» non è finita**: è la riga che separa questo da un
/// diario, e sta scritta in testa al file dal 28/08/2026.
#[test]
fn a_row_without_the_check_that_would_have_stopped_it_is_not_finished() {
    let read = faults::parse(&table());
    for fault in &read {
        assert!(
            !fault.what_would_prevent.is_empty(),
            "il guasto {} non dice cosa lo impedirebbe",
            fault.number
        );
        assert!(!fault.what_happened.is_empty(), "il {} è vuoto", fault.number);
        assert!(!fault.how_it_showed.is_empty(), "il {} non dice come si è visto", fault.number);
        assert!(!fault.status.is_empty(), "il {} non ha stato", fault.number);
    }
}

/// Nessun numero doppio e nessun buco fra quelli che arrivano dal markdown: la
/// migrazione li conserva, e da qui in poi non è più possibile sbagliarli.
#[test]
fn the_numbers_that_come_in_have_no_gaps_and_no_twins() {
    let read: Vec<Fault> = faults::parse(&table());
    let mut numbers: Vec<i64> = read.iter().map(|f| f.number).collect();
    numbers.sort_unstable();
    let mut expected: Vec<i64> = (1..=numbers.len() as i64).collect();
    expected.sort_unstable();
    assert_eq!(
        numbers, expected,
        "i numeri che entrano non sono 1..N senza buchi: la migrazione porterebbe \
         dentro un difetto invece di lasciarlo fuori"
    );
}
