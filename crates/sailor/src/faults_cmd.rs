//! `sailor faults`: i guasti incontrati, come dati invece che come documento.
//!
//! **PERCHÉ ESISTE QUESTO COMANDO E NON UN FILE.** La tabella dei guasti era
//! `docs/guasti-incontrati.md`: sei colonne travestite da markdown, con una
//! prova che ne faceva il *parsing* per tenerla onesta — contava le righe e
//! dettava a chi scriveva quale numero mettere nella prosa sotto. Era al
//! contrario, e il file lo diceva di sé stesso in testa: «è il materiale grezzo
//! di ciò che Sailor deve imparare a fare da solo; finché quell'anello non è
//! chiuso, la lista si tiene a mano».
//!
//! **E CHIUDE IL GUASTO 42.** Il numero non lo sceglie più chi scrive guardando
//! l'ultima riga: lo assegna il deposito, che su questa macchina è **uno**. Il
//! 43, il 47 e il 48 sono stati contesi da due sessioni in un pomeriggio, ogni
//! volta scoperti alla fusione; nessuna prova poteva impedirlo, perché una prova
//! guarda un ramo alla volta e i rami non si vedono fra loro.

use faults::{Draft, Fault, Faults};
use std::path::PathBuf;

/// Le forme di `sailor faults`, una per riga.
pub const USAGE: &[&str] = &[
    "sailor faults list      [--open] [--json]   i guasti registrati",
    "sailor faults add       < guasto.json       registra un guasto e gli assegna il numero",
    "sailor faults status <n> <testo>            cambia lo stato di un guasto",
    "sailor faults render    [--file <md>]       riscrive la tabella, per chi la vuole leggere così",
    "sailor faults import    <file.md>           porta dentro una tabella scritta a mano, una volta",
];

const FORMS: &[&str] = &["list", "add", "status", "render", "import"];

const WITHOUT_VALUE: &[&str] = &["open", "json"];

fn usage_text() -> String {
    format!("uso: {}", USAGE.join("\n     "))
}

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(said) => {
            if !said.is_empty() {
                println!("{said}");
            }
            0
        }
        Err(why) => {
            eprintln!("sailor faults: {why}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(usage_text());
    };
    if !FORMS.contains(&verb) {
        return Err(format!(
            "«{verb}» non è una forma di questo comando; ci sono {}\n{}",
            FORMS.join(", "),
            usage_text()
        ));
    }

    let mut options = std::collections::BTreeMap::new();
    let mut loose: Vec<String> = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(word) = rest.next() {
        match word.strip_prefix("--") {
            Some(name) if WITHOUT_VALUE.contains(&name) => {
                options.insert(name.to_owned(), "true".to_owned());
            }
            Some(name) => {
                let value = rest
                    .next()
                    .ok_or_else(|| format!("«--{name}» vuole un valore dopo di sé"))?;
                options.insert(name.to_owned(), value.clone());
            }
            None => loose.push(word.clone()),
        }
    }

    let path = match options.get("store") {
        Some(declared) => PathBuf::from(declared),
        None => Faults::default_path().map_err(|error| error.to_string())?,
    };
    let store = Faults::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;

    match verb {
        "list" => list(&store, &options),
        "add" => add(&store),
        "status" => set_status(&store, &loose),
        "render" => render(&store),
        "import" => import(&store, &loose),
        other => Err(format!("«{other}» non è una forma di questo comando")),
    }
}

fn list(
    store: &Faults,
    options: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    let all = store.all().map_err(|error| error.to_string())?;
    let shown: Vec<&Fault> = if options.contains_key("open") {
        all.iter().filter(|f| f.still_open()).collect()
    } else {
        all.iter().collect()
    };

    if options.contains_key("json") {
        return serde_json::to_string_pretty(&shown).map_err(|error| error.to_string());
    }

    let mut out = String::new();
    for fault in &shown {
        // **LA PRIMA RIGA DICE LO STATO, NON SOLO IL TITOLO.** Un elenco che
        // mostra solo cosa è successo si legge come una storia; questo si legge
        // come lavoro rimasto, che è a cosa serve.
        let standing = if fault.still_open() { "aperto " } else { "chiuso " };
        let title: String = fault.what_happened.chars().take(96).collect();
        out.push_str(&format!("{:>3}  {standing}  {}\n", fault.number, title));
    }
    let open = store.still_open().map_err(|error| error.to_string())?;
    out.push_str(&format!(
        "\n{open} ancora aperti su {}, contati adesso e non ricopiati",
        all.len()
    ));
    Ok(out)
}

/// Registra un guasto letto da standard input, **senza numero**.
///
/// Il numero non è un campo che si possa mandare: se lo fosse, chi scrive
/// tornerebbe a sceglierlo e il guasto 42 tornerebbe con lui.
fn add(store: &Faults) -> Result<String, String> {
    let raw = std::io::read_to_string(std::io::stdin())
        .map_err(|error| format!("non riesco a leggere il guasto: {error}"))?;
    let draft: Draft = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "il guasto va scritto come JSON con happened_on, what_happened, \
             how_it_showed, what_would_prevent e status: {error}"
        )
    })?;
    if draft.what_would_prevent.trim().is_empty() {
        // **UNA VOCE SENZA «COSA LO IMPEDIREBBE» NON È FINITA**, e sta scritto in
        // testa alla tabella dal 28/08/2026. Senza quella colonna è un diario,
        // che è esattamente ciò che quel file dichiara di non essere.
        return Err(
            "manca «what_would_prevent»: un guasto senza il controllo che lo \
             impedirebbe è un aneddoto, non un lavoro"
                .to_owned(),
        );
    }
    let recorded = store.record(&draft).map_err(|error| error.to_string())?;
    Ok(format!(
        "registrato il guasto {}: il numero l'ha dato il deposito",
        recorded.number
    ))
}

fn set_status(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [number, status] = loose else {
        return Err("uso: sailor faults status <numero> <testo dello stato>".to_owned());
    };
    let number: i64 = number
        .parse()
        .map_err(|_| format!("«{number}» non è un numero di guasto"))?;
    let changed = store
        .set_status(number, status)
        .map_err(|error| error.to_string())?;
    Ok(format!("guasto {}: {}", changed.number, changed.status))
}

fn render(store: &Faults) -> Result<String, String> {
    let all = store.all().map_err(|error| error.to_string())?;
    Ok(faults::render(&all).trim_end().to_owned())
}

/// Porta dentro una tabella scritta a mano. **Una volta sola, e lo dice.**
fn import(store: &Faults, loose: &[String]) -> Result<String, String> {
    let [file] = loose else {
        return Err("uso: sailor faults import <file.md>".to_owned());
    };
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let read = faults::parse(&text);
    if read.is_empty() {
        return Err(format!(
            "{file}: non ci ho trovato nessuna riga con sei colonne. Meglio \
             fermarsi che importare il vuoto e dichiararlo fatto"
        ));
    }
    for fault in &read {
        store.restore(fault).map_err(|error| error.to_string())?;
    }
    let now = store.all().map_err(|error| error.to_string())?;
    Ok(format!(
        "portati dentro {} guasti da {file}; nel deposito ora ce ne sono {}",
        read.len(),
        now.len()
    ))
}
