//! Gli identificatori sono in inglese, e adesso qualcuno lo misura.
//!
//! **PERCHÉ QUESTA PROVA ESISTE.** La regola sta in `AGENTS.md` dal 28/08/2026:
//! «Identificatori in inglese — nomi di funzione, tipi, campi, opzioni». Il
//! 31/08 se ne contavano **136 violazioni**, quasi tutte scritte dopo. Nessuna
//! era un errore di distrazione: la direttiva di sessione dice «italiano» senza
//! dire «tranne gli identificatori», e una regola che nessuna misura interroga
//! non diventa rossa mai — è la stessa lezione del puntatore morto che
//! `AGENTS.md` racconta di sé alle righe 17-20.
//!
//! **NON È UN ANALIZZATORE, ED È VOLUTO.** Non prova a capire il codice: cerca
//! parole di un elenco scritto a mano, in posizione di dichiarazione. Il prezzo
//! è dichiarato — una parola italiana che non è nell'elenco passa — e il
//! guadagno è che **non ha falsi positivi**, cioè non costringe nessuno a
//! discutere con lei. Chi ne trova una nuova la aggiunge sotto: è una riga.
//!
//! **COSA NON GUARDA.** Commenti, testo dentro le stringhe, e i documenti in
//! `docs/`: lì l'italiano è la regola, non l'eccezione. E i nomi delle *fixture*
//! dentro le stringhe di prova — `f.name == "assente"` — restano quello che
//! sono: dati, non identificatori.
//!
//! **E NON GUARDA I FLUSSI, PER DECISIONE.** Gli `id` dei flussi e dei passi —
//! `sviluppa-sailor`, `verdetto` — e i nomi dei file `.flow.json` restano in
//! italiano: decisione di Theo del 31/08/2026, scritta in `docs/decisioni.md`.
//! Sono dati che il **deposito conserva**: rinominare un passo farebbe apparire
//! le corse già registrate come passi sconosciuti, e cambiare il nome di un
//! flusso spedito farebbe smettere di vincere — in silenzio — il flusso che un
//! utente ha scritto in casa propria per sostituirlo.
//!
//! **Chi estende questo controllo ai `.flow.json` sta rompendo quella
//! decisione, non completandola.** Non è una dimenticanza.

use std::path::{Path, PathBuf};

/// Le parole italiane che non devono comparire in un identificatore.
///
/// Sono quelle **viste davvero** nel censimento del 31/08/2026, più i termini
/// del dominio che il progetto usa continuamente parlando (corsa, passo, flusso,
/// deposito) e che quindi finiscono in un nome senza che nessuno se ne accorga.
///
/// **QUELLE CHE MANCANO APPOSTA.** `solo`, `per`, `come`, `non`, `si`, `e`: sono
/// parole inglesi valide o troppo corte per essere distinte da un pezzo di nome
/// composto — `cache_write_per_million` non è italiano. Un elenco che le
/// contenesse darebbe errori che nessuno può correggere, e il primo che ne
/// incontra uno lo zittisce insieme a tutti gli altri.
const ITALIAN_WORDS: &[&str] = &[
    // `batteria`, `stile` e `finestra` sono entrate il 01/09/2026, aggiunte da
    // chi le ha incontrate — è l'istruzione che questo elenco dà di sé stesso.
    // Erano le chiavi dei lavori della CI. La quarta, `prove`, **non è entrata
    // e non può entrare**: è una parola inglese valida, cioè esattamente la
    // famiglia che il commento qui sopra esclude apposta.
    "assente",
    "atteso",
    "attesa",
    "batteria",
    "casa",
    "cassette",
    "ciclico",
    "cio",
    "conteggio",
    "coperto",
    "finestra",
    "stile",
    "corsa",
    "costata",
    "deposito",
    "elenco",
    "esempio",
    "esito",
    "fabbrica",
    "facoltativo",
    "famiglie",
    "flusso",
    "flussi",
    "guasto",
    "ignota",
    "lati",
    "letto",
    "listino",
    "lungo",
    "mai",
    "miei",
    "misurata",
    "motore",
    "nome",
    "nomi",
    "nuovo",
    "ondata",
    "parti",
    "passo",
    "piano",
    "prova",
    "ramo",
    "registro",
    "rotto",
    "sano",
    "senza",
    "smista",
    "spedito",
    "spediti",
    "spesa",
    "spia",
    "tetto",
    "tronco",
    "valido",
    "vecchio",
    "voce",
    "voci",
    "verdetto",
    "verifica",
    "quanto",
];

/// Dove un identificatore può essere dichiarato. Il testo che segue una di
/// queste parole, fino al primo carattere che non può stare in un nome.
const RUST_DECLARATIONS: &[&str] = &[
    "let ", "let mut ", "fn ", "struct ", "enum ", "mod ", "const ", "static ", "type ", "trait ",
];

const WEB_DECLARATIONS: &[&str] = &[
    "let ",
    "const ",
    "var ",
    "function ",
    "interface ",
    "class ",
    "type ",
];

/// La radice del repo, da cui questa prova gira.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta due livelli sotto la radice")
        .to_path_buf()
}

/// Ogni sorgente sotto `dir`, saltando quel che non è nostro.
fn sources_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // `target` e `node_modules` non sono codice nostro; `.git` nemmeno.
            if matches!(name.as_str(), "target" | "node_modules" | ".git" | "dist") {
                continue;
            }
            sources_under(&path, found);
            continue;
        }
        // **QUESTO CONTROLLO NON GUARDA SE STESSO.** Contiene apposta i nomi che
        // rifiuta — l'elenco delle parole, e gli esempi che dà in pasto alle
        // proprie funzioni — e senza questa riga si accuserebbe da solo, per
        // sempre, di essere scritto male.
        if name == "identifiers_are_in_english.rs" {
            continue;
        }
        // **ANCHE GLI `.html`, E NON PER COMPLETEZZA.** `crates/ui/assets/index.html`
        // porta dentro di sé il JavaScript della pagina del cruscotto: due
        // `const` italiane vivevano lì, invisibili al primo giro di questo
        // controllo perché il file non finiva in `.ts`. Nessun type-checker
        // guarda quel codice, quindi è l'unico posto dove una rinomina
        // sbagliata non dà errore — cioè quello dove serve di più.
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("ts") | Some("tsx") | Some("html")
        ) {
            found.push(path);
        }
    }
}

/// La riga senza il commento che la chiude, se ne ha uno.
///
/// Approssimazione dichiarata: una riga che ha `//` dentro una stringa viene
/// tagliata troppo presto. Il risultato è che si guarda **meno** codice, mai
/// più — questa prova può lasciar passare, non può accusare a torto.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// I pezzi di un identificatore: `spesa_totale` e `spesaTotale` danno entrambi
/// `["spesa", "totale"]`.
/// **LA GOBBA SI TAGLIA SOLO DOPO UNA MINUSCOLA.** La prima versione tagliava a
/// ogni maiuscola, e `CASSETTE_KINDS` diventava tredici lettere singole: nessuna
/// lettera sta nell'elenco, quindi ogni costante urlata sarebbe passata pulita
/// per sempre. L'ha presa la prova che misura questa prova, non un occhio.
fn parts_of(identifier: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_was_lower = false;
    for character in identifier.chars() {
        if character == '_' {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            previous_was_lower = false;
            continue;
        }
        if character.is_uppercase() && previous_was_lower {
            parts.push(std::mem::take(&mut current));
        }
        previous_was_lower = character.is_lowercase();
        current.push(character.to_ascii_lowercase());
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Il nome dichiarato dopo `keyword`, se quella riga ne dichiara uno.
fn declared_after<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
    let at = code.find(keyword)?;
    // Deve essere una parola intera: `applet ` non contiene una dichiarazione.
    let before = code[..at].chars().next_back();
    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let rest = &code[at + keyword.len()..];
    let end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Le parole italiane in un nome, se ce ne sono.
fn italian_in(name: &str) -> Vec<String> {
    parts_of(name)
        .into_iter()
        .filter(|part| ITALIAN_WORDS.contains(&part.as_str()))
        .collect()
}

/// **OGNI IDENTIFICATORE DICHIARATO È IN INGLESE.**
///
/// Il messaggio elenca tutto quello che ha trovato, con percorso e riga: una
/// prova che dice «ce n'è uno» costringe a ricominciare la ricerca a mano.
#[test]
fn every_declared_identifier_is_in_english() {
    let root = repository_root();
    let mut sources = Vec::new();
    for place in ["crates", "desktop/src", "desktop/src-tauri/src"] {
        sources_under(&root.join(place), &mut sources);
    }
    assert!(
        sources.len() > 20,
        "cercato in {} sorgenti: troppo pochi, la scansione non sta guardando dove crede",
        sources.len()
    );

    let mut found: Vec<String> = Vec::new();
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let web = path.extension().is_some_and(|e| e != "rs");
        let keywords = if web {
            WEB_DECLARATIONS
        } else {
            RUST_DECLARATIONS
        };
        for (number, line) in text.lines().enumerate() {
            let code = code_part(line);
            for keyword in keywords {
                let Some(name) = declared_after(code, keyword) else {
                    continue;
                };
                let italian = italian_in(name);
                if italian.is_empty() {
                    continue;
                }
                found.push(format!(
                    "{}:{}  {name}  (in italiano: {})",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    number + 1,
                    italian.join(", ")
                ));
            }
        }
    }

    assert!(
        found.is_empty(),
        "{} identificatori in italiano, e la regola sta in AGENTS.md dal 28/08/2026:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// **ANCHE I NOMI DEI FILE.**
///
/// In Rust un file *è* un modulo, quindi il suo nome è un identificatore — ma
/// la regola in `AGENTS.md` elenca «funzione, tipi, campi, opzioni» e i file non
/// ci sono. È una delle ragioni per cui `smista_il_lavoro.rs` è potuto nascere
/// senza che niente protestasse.
#[test]
fn every_source_file_is_named_in_english() {
    let root = repository_root();
    let mut sources = Vec::new();
    for place in ["crates", "desktop/src", "desktop/src-tauri/src"] {
        sources_under(&root.join(place), &mut sources);
    }

    let mut found: Vec<String> = Vec::new();
    for path in &sources {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().replace('-', "_"))
            .unwrap_or_default();
        let italian = italian_in(&stem);
        if italian.is_empty() {
            continue;
        }
        found.push(format!(
            "{}  (in italiano: {})",
            path.strip_prefix(&root).unwrap_or(path).display(),
            italian.join(", ")
        ));
    }

    assert!(
        found.is_empty(),
        "{} file con nome italiano:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// **ANCHE LE CHIAVI DEI LAVORI DELLA CI, E SI È VISTO PERCHÉ IL 01/09/2026.**
///
/// Le due prove qui sopra guardano `.rs`, `.ts`, `.tsx`, `.html`. Un file
/// `.yml` non è codice per nessuna di loro, quindi
/// il file della CI — che allora si chiamava `la-batteria.yml`, oggi
/// `the-battery.yml` — è nato con tre lavori chiamati `prove`,
/// `stile` e `finestra` e **nessuno ha protestato**. Non è che la regola non
/// c'era: è che nessuno la interrogava su quel tipo di file, e una regola che
/// nessuno interroga lì, lì non diventa rossa mai. È la stessa forma per cui
/// esiste la prova sui nomi dei file, un tipo di file più in là.
///
/// **IL LIMITE, DICHIARATO: `prove` NON È CATTURABILE.** La prima stesura di
/// questa prova affermava che `prove` fosse già nell'elenco delle parole
/// italiane. Non c'era, e non ci può stare: *prove* è una parola inglese
/// valida, cioè la famiglia che quell'elenco esclude apposta perché
/// darebbe accuse che nessuno può correggere. A dirlo è stata
/// `the_check_can_still_see_a_name_it_should_reject`, cinque minuti dopo che
/// l'affermazione era stata scritta — che è il motivo per cui quella prova
/// esiste. Quindi qui si catturano `stile`, `finestra` e `batteria`, non
/// tutto: un controllo che dichiara dove non arriva vale più di uno che
/// lascia credere di arrivare dappertutto.
///
/// **PERCHÉ LE CHIAVI SÌ E I `name:` NO.** Il confine è quello di `AGENTS.md`:
/// ciò che una macchina legge sta in inglese, ciò che una persona legge no. Le
/// chiavi le leggono `needs:`, `jobs.<id>` nelle API e i filtri di `gh run` —
/// sono identificatori quanto un campo di `struct`. I `name:` sono la frase
/// che compare a chi guarda una corsa: sono messaggi, e i messaggi stanno in
/// italiano finché quella riga di `AGENTS.md` dice così.
#[test]
fn every_workflow_job_key_is_in_english() {
    let root = repository_root();
    let workflows = root.join(".github/workflows");
    let Ok(entries) = std::fs::read_dir(&workflows) else {
        panic!("nessun workflow da controllare in {}", workflows.display());
    };

    let mut found: Vec<String> = Vec::new();
    let mut looked_at = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        ) {
            continue;
        }
        looked_at += 1;
        let text = std::fs::read_to_string(&path).expect("leggere il workflow");
        for key in job_keys_of(&text) {
            let italian = italian_in(&key.replace('-', "_"));
            if italian.is_empty() {
                continue;
            }
            found.push(format!(
                "{}  lavoro «{key}» (in italiano: {})",
                path.strip_prefix(&root).unwrap_or(&path).display(),
                italian.join(", ")
            ));
        }
    }

    // Senza questa riga la prova resterebbe verde il giorno in cui la cartella
    // cambia nome: guarderebbe zero file e non lo direbbe a nessuno.
    assert!(
        looked_at > 0,
        "nessun file .yml letto: la prova non sta guardando niente"
    );
    assert!(
        found.is_empty(),
        "{} lavori con la chiave in italiano:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// Le chiavi di primo livello sotto `jobs:`, cioè le righe rientrate di due
/// spazi che finiscono in `:` prima che cominci un'altra sezione a colonna
/// zero. Non è un lettore di YAML e non vuole esserlo: legge la sola forma che
/// i nostri workflow hanno, e se un giorno non basterà più il conto dei lavori
/// scenderà a zero — che è il verso giusto in cui sbagliare, perché la riga
/// `looked_at > 0` qui sopra si accorge del caso limite.
fn job_keys_of(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("jobs:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        // Una riga a colonna zero che non è vuota chiude la sezione.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('#') {
            continue;
        }
        if let Some(name) = rest.strip_suffix(':') {
            keys.push(name.trim().to_owned());
        }
    }
    keys
}

/// La prova che misura questa prova.
///
/// **CHI MISURA VA MISURATO.** Un controllo che cerca parole in un elenco può
/// essere rotto in un modo che nessuno vede: se `parts_of` smettesse di spezzare
/// i nomi composti, o se `declared_after` non trovasse più niente, le due prove
/// qui sopra resterebbero **verdi per sempre** e nessuno saprebbe che hanno
/// smesso di guardare. Qui si dà loro in pasto un caso noto e si pretende che lo
/// vedano.
#[test]
fn the_check_can_still_see_a_name_it_should_reject() {
    assert_eq!(
        parts_of("write_listino"),
        vec!["write", "listino"],
        "i nomi composti si spezzano, o l'elenco non trova mai niente"
    );
    assert_eq!(
        parts_of("CASSETTE_KINDS"),
        vec!["cassette", "kinds"],
        "anche quelli in maiuscolo"
    );
    assert_eq!(
        parts_of("spesaTotale"),
        vec!["spesa", "totale"],
        "e quelli scritti a gobbe, che è come li scrive la finestra"
    );
    assert_eq!(
        declared_after("    let listino = read();", "let "),
        Some("listino")
    );
    assert_eq!(
        declared_after("    applet_size = 3;", "let "),
        None,
        "«applet» non dichiara niente: la parola dev'essere intera"
    );
    assert_eq!(
        code_part("    let x = 1; // qui il listino resta italiano"),
        "    let x = 1; ",
        "il commento si taglia via, o ogni riga di prosa diventerebbe un'accusa"
    );
    // E il lettore delle chiavi: deve prendere i lavori e **non** le righe che
    // stanno dentro un lavoro, o `name`, `steps` e `run` diventerebbero lavori.
    assert_eq!(
        job_keys_of("name: x\n\njobs:\n  # un commento\n  stile:\n    name: il debito\n    steps:\n      - run: echo\n  desktop:\n    name: la finestra\n"),
        vec!["stile", "desktop"],
        "le chiavi dei lavori, e solo quelle"
    );
    assert!(
        !italian_in("stile").is_empty(),
        "«stile» è nell'elenco: se non lo fosse, il lavoro chiamato così passerebbe"
    );
    assert!(
        italian_in("prove").is_empty(),
        "**limite dichiarato, non difetto**: «prove» è una parola inglese valida \
         e non può stare nell'elenco. Se un giorno ci finisse, questa riga \
         diventerebbe rossa e chi la legge saprebbe di aver appena reso \
         impossibile chiamare qualcosa `prove` in inglese"
    );
    assert!(!italian_in("write_listino").is_empty());
    assert!(
        italian_in("write_price_list").is_empty(),
        "e un nome inglese passa, o non si potrebbe mai riparare niente"
    );
}
