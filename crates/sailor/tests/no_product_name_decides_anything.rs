//! **LA REGOLA DI FERRO DEL TRACCIAMENTO: il nome di un prodotto può comparire
//! in un'etichetta, mai in una condizione.**
//!
//! `println!("gira in Orca")` va bene: è un'etichetta, la legge una persona, e
//! se è sbagliata si vede. `if host == "orca"` è vietato: è una decisione, la
//! legge il programma, e quando è sbagliata il programma fa un'altra cosa senza
//! dirlo — su una macchina dove quel prodotto non c'è, o si chiama in un altro
//! modo, o è stato sostituito.
//!
//! **PERCHÉ SERVE UNA PROVA E NON UNA RIGA IN UN DOCUMENTO.** Una regola che
//! nessuno interroga non diventa rossa mai: è la lezione che `AGENTS.md`
//! racconta di sé, ed è costata 136 rinomine sugli identificatori. Il primo
//! `if` su un nome di prodotto entra da solo, sembra ragionevole nel punto in
//! cui lo si scrive, e da quel momento il tracciamento è specifico di un
//! prodotto senza che nessun controllo lo dica.
//!
//! **COSA GUARDA.** I sorgenti del tracciamento e nient'altro: `crates/sessions`
//! e `sailor session`. Non è un controllo su tutto l'albero — altrove i nomi
//! dei prodotti sono legittimi, perché altrove si parla di quei prodotti (i
//! descrittori delle righe di comando, i profili, il catalogo dei modelli).
//! Qui no: qui l'ancora è `(tty, albero, capostipite)`, e il capostipite è
//! **solo un'etichetta**.
//!
//! **QUESTA PROVA SI MISURA DA SOLA.** L'ultima prova del file dà al proprio
//! rilevatore un pezzo di codice che viola la regola e pretende che lo trovi:
//! senza, una modifica che spegne il rilevatore lascerebbe tutto verde per
//! sempre, ed è il guasto che `AGENTS.md` chiama «se togliendo la riga che
//! dichiari il controllo resta verde, il controllo non controlla niente».

use std::path::{Path, PathBuf};

/// I nomi che non devono decidere niente.
///
/// **SONO NOMI PROPRI, NON PAROLE COMUNI.** `terminal`, `shell`, `window` non
/// stanno qui e non ci devono stare: sono le cose, non i prodotti, e un elenco
/// che le contenesse darebbe errori che nessuno può correggere — il primo che
/// ne incontra uno zittisce la prova insieme a tutti gli altri.
const PRODUCT_NAMES: &[&str] = &[
    "orca", "iterm", "warp", "ghostty", "alacritty", "wezterm", "kitty", "zellij", "tmux",
    "claude", "codex", "gemini", "cursor", "copilot", "aider", "vscode", "jetbrains",
];

/// I segni che una riga **decide** invece di raccontare.
///
/// L'elenco è largo di proposito: una riga che nomina un prodotto e contiene
/// uno di questi va guardata da una persona, e il costo di guardarla è una
/// riga di commento. Il costo di non guardarla è un tracciamento che funziona
/// su una macchina sola.
const SIGNS_OF_A_DECISION: &[&str] = &[
    "if ", "else if", "match ", "while ", "==", "!=", "=>", "contains(", "contains_key(",
    "starts_with(", "ends_with(", "matches!", ".any(", ".all(", ".find(", ".filter(",
    ".position(", "unwrap_or_else(", "then(", "then_some(",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// La riga senza il commento che la chiude.
///
/// **È QUI CHE PASSA IL CONFINE FRA ETICHETTA E CONDIZIONE.** Un commento e un
/// commento di documentazione parlano dei prodotti quanto serve — questo file
/// per primo — e non decidono niente. Il taglio è approssimato per difetto: una
/// riga con `//` dentro una stringa si accorcia troppo, e il risultato è che si
/// guarda **meno** codice, mai di più. Questa prova può lasciar passare, non
/// può accusare a torto.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// The sources of the tracking, and those of the conduit.
///
/// The conduit belongs here because the rule is the same. Whoever holds a
/// command line's terminal and writes into it must not know which one it is: not which
/// window drew it, not which engine runs in it. The first `if` on a name makes
/// Sailor the product of a single command line, and there is no way back.
fn tracking_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = vec![
        root.join("crates/sailor/src/session_cmd.rs"),
        root.join("crates/sailor/src/terminal_cmd.rs"),
    ];
    collect_under(&root.join("crates/sessions"), &mut found);
    collect_under(&root.join("crates/terminal"), &mut found);
    found.retain(|path| path.exists());
    found
}

fn collect_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_under(&path, found);
            continue;
        }
        if path.extension().and_then(|kind| kind.to_str()) == Some("rs") {
            found.push(path);
        }
    }
}

/// Le violazioni in un testo: riga per riga, il nome trovato e il segno che
/// rende quella riga una decisione.
fn decisions_on_a_product_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line).to_lowercase();
        let Some(product) = PRODUCT_NAMES.iter().find(|name| code.contains(**name)) else {
            continue;
        };
        let Some(sign) = SIGNS_OF_A_DECISION.iter().find(|sign| code.contains(**sign)) else {
            continue;
        };
        found.push(format!(
            "riga {}: «{product}» dentro una condizione («{sign}»): {}",
            number + 1,
            line.trim()
        ));
    }
    found
}

#[test]
fn no_product_name_appears_in_a_condition_of_the_tracking() {
    let sources = tracking_sources();
    assert!(
        sources.len() >= 4,
        "guardati {} sorgenti: la scansione non sta guardando dove crede",
        sources.len()
    );

    let mut broken: Vec<String> = Vec::new();
    for path in &sources {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));
        for problem in decisions_on_a_product_in(&text) {
            broken.push(format!("{}: {problem}", path.display()));
        }
    }

    assert!(
        broken.is_empty(),
        "il tracciamento decide sul nome di un prodotto, e non deve:\n{}\n\n\
         Un nome di prodotto è un'etichetta: si stampa e si registra. \
         L'ancora è (tty, albero, capostipite), e il capostipite non si \
         interroga. Se serve distinguere un caso, distinguilo su ciò che il \
         caso ha di diverso, non su come si chiama chi lo ha aperto.",
        broken.join("\n")
    );
}

/// **CHI MISURA VA MISURATO.** Il rilevatore deve trovare la violazione quando
/// c'è, e lasciar stare l'etichetta quando è un'etichetta. Senza queste due
/// righe la prova qui sopra resterebbe verde anche se il rilevatore smettesse
/// di rilevare, e nessuno se ne accorgerebbe.
#[test]
fn the_check_finds_a_violation_that_is_there_and_leaves_a_label_alone() {
    let forbidden = "    if ancestor.as_deref() == Some(\"Orca\") {\n";
    assert_eq!(
        decisions_on_a_product_in(forbidden).len(),
        1,
        "il rilevatore non vede una condizione su un nome di prodotto"
    );

    let allowed = "    println!(\"gira in Orca\");\n";
    assert!(
        decisions_on_a_product_in(allowed).is_empty(),
        "un'etichetta non è una decisione, e questa prova non deve vietarla"
    );

    let commented = "    let found = 1; // qui il capostipite risulta Orca\n";
    assert!(
        decisions_on_a_product_in(commented).is_empty(),
        "un commento parla dei prodotti quanto serve: non decide niente"
    );
}
