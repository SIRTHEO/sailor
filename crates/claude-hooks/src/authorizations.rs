//! Il registro delle autorizzazioni: distingue una decisione vera di Theo da
//! una riferita da un pari, senza che chi esegue debba chiedere a nessuno.
//!
//! Nasce dalla segnalazione del 21/08/2026
//! (`state/plancia/segnalazioni/2026-08-21-la-catena-delle-autorizzazioni-si-e-rotta.md`):
//! la catena — sessione chiede, capitano valuta, Theo decide, capitano scrive,
//! macchinista esegue — è stata saltata perché il pezzo che la rende
//! eseguibile, il registro, non esisteva. La procedura sta in
//! `docs/procedura-autorizzazioni.md`; qui sta solo la lettura.
//!
//! LA TRAPPOLA CHE QUESTO MODULO ESISTE PER EVITARE: un registro **assente**
//! (il file non si apre) e un registro **vuoto** (il file c'è, zero righe)
//! devono rispondere in modo diverso. Il primo vuol dire «non lo so, fermati»;
//! il secondo vuol dire «nessuna autorizzazione, e il controllo ha funzionato
//! davvero». Confonderli autorizzerebbe tutto il giorno in cui il file sparisce.

use std::path::{Path, PathBuf};

/// Una riga del registro: le sei informazioni che la procedura chiede, più la
/// chiave stabile su cui si cerca. I valori restano come scritti dal
/// capitano — italiano compreso — perché `theo_said` è una trascrizione alla
/// lettera, non un riassunto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRecord {
    pub key: String,
    pub when: String,
    pub requested_by: String,
    pub request: String,
    pub theo_said: String,
    pub evaluated_by: String,
    pub executor: String,
}

/// I tre esiti, mai confusi fra loro: `Unreadable` non è un `NotAuthorized`
/// con un motivo in più, è un ramo diverso — chi legge questo tipo non può
/// scambiarli per distrazione, come invece capita con un booleano o un
/// `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Authorized(AuthorizationRecord),
    NotAuthorized,
    Unreadable,
}

/// Interpreta una riga come record. `None` se non è un oggetto JSON o gli
/// manca la chiave — senza la chiave la riga non è cercabile, quindi non è
/// utilizzabile. Gli altri cinque campi mancanti restano vuoti piuttosto che
/// far cadere l'intera riga: un registro scritto a mano con un campo
/// dimenticato non deve nascondere le altre autorizzazioni che contiene.
fn parse_line(line: &str) -> Option<AuthorizationRecord> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let field = |name: &str| {
        v.get(name)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let key = v.get("key").and_then(|x| x.as_str())?.to_string();
    Some(AuthorizationRecord {
        key,
        when: field("when"),
        requested_by: field("requested_by"),
        request: field("request"),
        theo_said: field("theo_said"),
        evaluated_by: field("evaluated_by"),
        executor: field("executor"),
    })
}

/// La domanda pura: dato il contenuto del registro (o l'indicazione che non
/// si è potuto leggere) e la chiave cercata, quale dei tre esiti vale.
///
/// `registry: None` è «il file non si apre o non esiste»: risponde
/// `Unreadable` prima ancora di guardare la chiave, perché non c'è niente da
/// guardare. `Some("")` (il file esiste, zero righe) scende invece nel ciclo,
/// non trova mai una corrispondenza e risponde `NotAuthorized` — è la
/// distinzione che questo modulo esiste per tenere separata.
///
/// Le righe malformate non fermano la ricerca né vengono taciute: finiscono
/// nel secondo elemento della coppia, così chi chiama può avvisarne senza che
/// una riga rotta nasconda una riga buona più avanti nel file.
pub fn check(registry: Option<&str>, key: &str) -> (Verdict, Vec<String>) {
    let Some(content) = registry else {
        return (Verdict::Unreadable, Vec::new());
    };
    let mut warnings = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            // Confronto esatto, non `starts_with`/`contains`: un'autorizzazione
            // stretta non deve coprirne una larga che le somiglia solo nel testo.
            Some(record) if record.key == key => return (Verdict::Authorized(record), warnings),
            Some(_) => {}
            None => warnings.push(format!("riga {}: non è un record valido", i + 1)),
        }
    }
    (Verdict::NotAuthorized, warnings)
}

/// Il percorso del registro. La cartella base si inietta per le prove, stessa
/// convenzione di `hook_census`/`reachability`: senza, un test che vuole
/// vedere «registro assente» dovrebbe cancellare il file vero.
fn registry_path() -> PathBuf {
    let home = std::env::var_os("AUTORIZZAZIONI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/someone/.claude"));
    home.join("state").join("autorizzazioni.jsonl")
}

/// La sola lettura da disco: `Ok` con qualunque contenuto (anche vuoto)
/// diventa `Some`, ogni errore (file assente, permessi, altro) diventa
/// `None`. È qui, e non nella funzione pura sopra, che «non esiste» e «non si
/// apre» si fondono nello stesso `Unreadable` — la distinzione che conta è
/// leggibile/non leggibile, non la causa dell'errore.
fn read_registry(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn print_record(key: &str, record: &AuthorizationRecord) {
    println!("AUTORIZZATA: {key}");
    println!("  quando:         {}", record.when);
    println!("  chi ha chiesto: {}", record.requested_by);
    println!("  cosa:           {}", record.request);
    println!("  Theo ha detto:  «{}»", record.theo_said);
    println!("  valutata da:    {}", record.evaluated_by);
    println!("  esegue:         {}", record.executor);
}

/// Sottocomando `authorization-check <chiave>`, per chi esegue e non deve
/// chiedere a nessuno. Uscita 0 = autorizzata, 1 = non autorizzata, 3 = non
/// si sa (registro illeggibile) — tre codici diversi perché confondere
/// l'ultimo col secondo è esattamente il difetto per cui questo strumento
/// esiste: un registro che sparisce non deve leggersi come «via libera».
pub fn run() -> i32 {
    let Some(key) = std::env::args().nth(2) else {
        eprintln!("uso: claude-hooks authorization-check <chiave>");
        return 64;
    };
    let path = registry_path();
    let registry = read_registry(&path);
    let (verdict, warnings) = check(registry.as_deref(), &key);
    for w in &warnings {
        eprintln!("autorizzazioni: {w}, ignorata");
    }
    match verdict {
        Verdict::Authorized(record) => {
            print_record(&key, &record);
            0
        }
        Verdict::NotAuthorized => {
            println!("NON AUTORIZZATA: nessuna riga del registro corrisponde a «{key}».");
            1
        }
        Verdict::Unreadable => {
            eprintln!(
                "NON SO SE È AUTORIZZATA: il registro ({}) non esiste o non si apre. \
Non è \"non autorizzata\": è \"non si può dire\". Fermati, non eseguire, passa dal capitano.",
                path.display()
            );
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(key: &str) -> String {
        format!(
            r#"{{"key":"{key}","when":"2026-08-21T12:00:00+02:00","requested_by":"prova","request":"cosa richiesta","theo_said":"parole di Theo","evaluated_by":"capitano","executor":"macchinista"}}"#
        )
    }

    // La riga che c'è: il caso base.
    // Mutazione che lo fa rosso: invertire il ramo `Some(record) if record.key
    // == key` in `check`, così una riga che combacia esce come `NotAuthorized`.
    #[test]
    fn authorized_when_the_key_is_present() {
        let registry = line("gancio-nuovo:foo");
        let (verdict, warnings) = check(Some(&registry), "gancio-nuovo:foo");
        assert!(warnings.is_empty());
        match verdict {
            Verdict::Authorized(r) => {
                assert_eq!(r.theo_said, "parole di Theo");
                assert_eq!(r.executor, "macchinista");
            }
            other => panic!("atteso Authorized, trovato {other:?}"),
        }
    }

    // La riga che non c'è: una chiave diversa nel registro.
    // Mutazione che lo fa rosso: far restituire `Verdict::Authorized` per la
    // prima riga incontrata, indipendentemente dalla chiave cercata.
    #[test]
    fn not_authorized_when_the_key_is_absent() {
        let registry = line("gancio-nuovo:foo");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:bar");
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // Il registro illeggibile: `None`, cioè il file non si apre o non esiste.
    // Mutazione che lo fa rosso: sostituire il primo `let Some(content) =
    // registry else { return Unreadable }` con `registry.unwrap_or_default()`,
    // che tratta l'assenza come un registro vuoto — È IL MUTANTE DEL COMPITO:
    // un registro assente passerebbe per «nessuna autorizzazione» invece di
    // fermare tutto. Provato per davvero, esito sotto nel rapporto.
    #[test]
    fn unreadable_when_the_registry_could_not_be_read() {
        let (verdict, warnings) = check(None, "qualunque-chiave");
        assert_eq!(verdict, Verdict::Unreadable);
        assert!(warnings.is_empty());
    }

    // Il registro vuoto: il file esiste, zero righe. Deve rispondere
    // `NotAuthorized`, non `Unreadable` — è il gemello della prova sopra, e
    // insieme sono la distinzione che questo modulo esiste per tenere.
    // Mutazione che lo fa rosso: far rispondere `Unreadable` anche quando
    // `content.is_empty()`.
    #[test]
    fn empty_registry_is_not_authorized_not_unreadable() {
        let (verdict, warnings) = check(Some(""), "qualunque-chiave");
        assert_eq!(verdict, Verdict::NotAuthorized);
        assert!(warnings.is_empty());
    }

    // Una riga malformata insieme a una buona: non deve far crollare la
    // ricerca né nascondere la riga buona che segue.
    // Mutazione che lo fa rosso: interrompere il ciclo (`break`/`return
    // NotAuthorized`) alla prima riga che `parse_line` non riconosce, invece
    // di segnalarla e proseguire.
    #[test]
    fn malformed_line_is_reported_but_does_not_hide_a_good_line_after_it() {
        let registry = format!("{{questo non è JSON valido\n{}", line("gancio-nuovo:foo"));
        let (verdict, warnings) = check(Some(&registry), "gancio-nuovo:foo");
        assert_eq!(warnings.len(), 1, "la riga rotta deve produrre un avviso, uno solo");
        match verdict {
            Verdict::Authorized(r) => assert_eq!(r.key, "gancio-nuovo:foo"),
            other => panic!("la riga buona dopo quella rotta deve comunque farsi trovare, trovato {other:?}"),
        }
    }

    // Nessun combaciamento parziale: una chiave che contiene l'altra non deve
    // corrispondere, altrimenti un'autorizzazione larga ne coprirebbe una
    // stretta (o viceversa).
    // Mutazione che lo fa rosso: cambiare `record.key == key` in
    // `record.key.starts_with(key)` o `.contains(key)`.
    #[test]
    fn a_similar_key_is_not_a_match() {
        let registry = line("gancio-nuovo:foo-esteso");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:foo");
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // La lettura pura di una riga: campi mancanti restano vuoti, non fanno
    // cadere l'intera riga — solo la chiave è obbligatoria.
    #[test]
    fn parse_line_defaults_missing_optional_fields_to_empty() {
        let r = parse_line(r#"{"key":"solo-la-chiave"}"#).expect("la chiave basta a fare un record");
        assert_eq!(r.key, "solo-la-chiave");
        assert_eq!(r.theo_said, "");
    }

    #[test]
    fn parse_line_rejects_a_record_without_a_key() {
        assert!(parse_line(r#"{"when":"oggi"}"#).is_none());
    }
}
