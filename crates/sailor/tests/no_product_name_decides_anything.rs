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

/// The field the rule is really about: the ancestor is a **label**, so it is
/// recorded and printed and never interrogated.
const THE_LABEL_THAT_MUST_NOT_DECIDE: &str = "ancestor";

/// A comparison against a written-down value, spaces removed so `== "x"` and
/// `=="x"` read alike. **This is the half that needs no list of names**:
/// whatever the eighteenth emulator is called, comparing the ancestor with a
/// constant is the defect. Bare `Some("…")` is deliberately absent — it was the
/// first draft's defect, since `ancestor: Some("x".to_owned())` builds a row
/// rather than questioning one.
const COMPARED_WITH_A_WRITTEN_VALUE: &[&str] = &[
    "==\"", "!=\"", "==some(\"", "!=some(\"", "contains(\"", "starts_with(\"", "ends_with(\"",
    "eq(\"", "eq_ignore_ascii_case(\"",
];

/// The names that must decide nothing. **The wide net, not the check**: it
/// catches a product name wherever it decides anything, ancestor or not, but it
/// is walked around by picking the entry that is not in it. **Proper names
/// only** — `terminal`, `shell`, `window` are the things, not the products, and
/// a list holding them would raise errors nobody can fix, so the first person
/// to hit one would silence the test along with everything else.
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

/// I sorgenti del tracciamento.
fn tracking_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = vec![root.join("crates/sailor/src/session_cmd.rs")];
    collect_under(&root.join("crates/sessions"), &mut found);
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

/// Where the ancestor's label is compared with a value written into the code,
/// whatever that value says.
///
/// The literal has to sit **inside** the comparison, or `if ancestor.is_none()
/// { return "unknown"; }` would be accused: a condition on the ancestor and a
/// string on the same line, but the string is the answer, not the comparand.
fn the_label_compared_with_a_constant_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line).to_lowercase();
        if !code.contains(THE_LABEL_THAT_MUST_NOT_DECIDE) {
            continue;
        }
        // An assertion compares what was **recorded**, and a test must be able
        // to. What is forbidden is deciding at run time, not checking.
        if code.contains("assert") {
            continue;
        }
        let tight: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let matched_on = tight.contains("match") && tight.contains("some(\"");
        let shape = if matched_on {
            "match … Some(\"…\")"
        } else {
            match COMPARED_WITH_A_WRITTEN_VALUE
                .iter()
                .find(|shape| tight.contains(**shape))
            {
                Some(shape) => shape,
                None => continue,
            }
        };
        found.push(format!(
            "riga {}: il capostipite confrontato con un valore scritto («{shape}»): {}",
            number + 1,
            line.trim()
        ));
    }
    found
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
        for problem in the_label_compared_with_a_constant_in(&text) {
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

/// **THE EIGHTEENTH NAME, WHICH THE LIST DOES NOT HAVE.** This is the whole
/// reason the shape check exists: a list is walked around by picking the entry
/// that is not in it, and `PRODUCT_NAMES` has seventeen entries.
#[test]
fn a_product_the_list_has_never_heard_of_is_caught_all_the_same() {
    let unheard_of = "    if ancestor.as_deref() == Some(\"Rossignol\") {\n";
    assert!(
        decisions_on_a_product_in(unheard_of).is_empty(),
        "premessa di questa prova: il nome non è nell'elenco, o non prova niente"
    );
    assert_eq!(
        the_label_compared_with_a_constant_in(unheard_of).len(),
        1,
        "il capostipite confrontato con una costante è il difetto, comunque si chiami"
    );

    for shape in [
        "    match ancestor.as_deref() { Some(\"Rossignol\") => 1, _ => 0 }\n",
        "    if ancestor.contains(\"Rossignol\") {\n",
        "    if ancestor.starts_with(\"Rossignol\") {\n",
    ] {
        assert_eq!(
            the_label_compared_with_a_constant_in(shape).len(),
            1,
            "forma non vista: {shape}"
        );
    }
}

/// And the shapes that **must** pass, or the check would be silenced on day one
/// by somebody who cannot write a legitimate line.
#[test]
fn recording_the_label_and_asking_whether_it_is_there_stay_allowed() {
    for allowed in [
        "        ancestor: Some(\"Whatever\".to_owned()),\n",
        "    if ancestor.is_none() { return \"sconosciuto\"; }\n",
        "    println!(\"capostipite: {ancestor}\");\n",
        "    row.ancestor = arrival.ancestor.clone();\n",
        // An assertion compares what was recorded, and must be able to.
        "        assert_eq!(row.ancestor.as_deref(), Some(\"Whatever\"));\n",
    ] {
        assert!(
            the_label_compared_with_a_constant_in(allowed).is_empty(),
            "accusata a torto una riga che registra invece di decidere: {allowed}"
        );
    }
}
