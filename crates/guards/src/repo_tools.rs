//! Quando la domanda è «quanti X ci sono», il repo di solito lo stampa già.
//!
//! IL REPERTO CHE LO MOTIVA, 18/08/2026. Il censimento delle rotte del
//! matching-engine è stato fatto con uno script Python e due regex: **168 rotte**
//! pubblicate in un piano, e il numero vero era **214 operazioni** — il 21%
//! perso. Lo strumento giusto stava nel repo, dichiarato in `package.json`:
//! `npm run openapi:dump`. Una regex conta le righe che somigliano a una rotta;
//! il generatore conta le rotte.
//!
//! QUANTO E' FREQUENTE, misurato sui transcript dei tre giorni precedenti: su
//! 15.196 comandi Bash, **359 camminano l'albero per ottenere un numero** (2,4%)
//! in 21 sessioni, e **181 di quelli sono su rotte ed endpoint** — cioè proprio
//! il dominio in cui i repo qui hanno un generatore. Non è un caso isolato: è il
//! modo normale di rispondere a «quanti».
//!
//! IL CRITERIO E' STRETTO DI PROPOSITO. Si nomina soltanto uno script che
//! **esiste** in quel `package.json`, mai un suggerimento generico: un gancio che
//! consiglia strumenti immaginari si spegne dopo tre volte. Se il repo non ha
//! niente per quel dominio, questo modulo tace — ed è il caso normale, non
//! l'eccezione.
//!
//! NON E' UN GANCIO, ED E' LA MISURA A DIRLO. La domanda di partenza era se
//! questo consiglio potesse diventare un controllo automatico invece di una
//! prescrizione. Provato sui comandi veri dei tre giorni precedenti — 73 di
//! quelli che contano camminando l'albero e nominano uno dei quattro repo:
//!
//!   criterio largo  →  parla 29 volte su 73, e i primi otto guardati sono
//!                      **tutti falsi**: script che *scrivono* codice contenente
//!                      la parola «endpoint», letture scambiate per conteggi, e
//!                      in un caso `db:backfill-public-id` — una migrazione dati
//!                      — proposto a chi stava leggendo un commento;
//!   criterio stretto →  parla 2 volte su 73, ed entrambe sono comandi che
//!                      stavano scrivendo questo stesso modulo.
//!
//! In mezzo non c'è niente: o fa rumore su ciò che non è un censimento, o non
//! incontra il censimento. Il §4 del mandato dice di eliminare i gate che non
//! proteggono una decisione reale, e un gancio che sbaglia sette volte su sette
//! verrebbe spento dopo tre giorni — con l'aggravante di sembrare acceso.
//!
//! Resta quindi un **comando**, `claude-hooks repo-tools <repo>`, che risponde
//! alla domanda quando gliela si fa. La prescrizione — «prima di contare, chiedi
//! se il repo lo stampa già» — resta tale, e questo modulo è il modo di
//! verificarla in un secondo invece che a memoria.

use std::collections::BTreeSet;

/// Uno script del repo che risponde alla domanda meglio di una regex.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tool {
    /// Come si lancia: `npm run <nome>`.
    pub name: String,
    /// La riga di comando dichiarata, tagliata a una lunghezza leggibile.
    pub command: String,
}

/// Il comando chiede un **numero**, non del testo da leggere?
///
/// `grep -c`, `| wc -l`, `--count`, e i conteggi degli script Python. Un
/// `grep -n` che cerca dove sta una cosa non è un censimento e qui non entra:
/// il reperto riguarda chi conta, non chi cerca.
pub fn asks_for_a_count(command: &str) -> bool {
    // `len(` e `Counter(` erano qui, e sono stati tolti dopo la misura: compaiono
    // in **qualunque** script Python, non solo in un censimento. Sui comandi veri
    // dei tre giorni precedenti facevano parlare il consiglio 29 volte su 72, e
    // sette degli otto casi guardati erano falsi. Restano i marcatori che non
    // hanno altro uso: chiedere un numero a una ricerca.
    const COUNTERS: &[&str] = &["wc -l", "grep -c", "--count"];
    if COUNTERS.iter().any(|c| command.contains(c)) {
        return true;
    }
    // `grep -c` si scrive anche con le opzioni unite (`-rc`, `-rIc`): si guarda
    // ogni parola corta che comincia con un solo trattino e contiene una `c`,
    // ma solo se nel comando c'è davvero una ricerca — altrimenti `-c` di
    // qualunque altro comando farebbe parlare il gancio.
    (command.contains("grep") || command.contains("rg "))
        && command
            .split_whitespace()
            .any(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c') && w.len() <= 6)
}

/// Di che cosa si sta contando, se si riesce a dirlo.
///
/// Le parole sono quelle che compaiono nei comandi veri misurati, non un
/// vocabolario immaginato: `router.get(`, `endpoint`, `schema.prisma`, `describe(`.
pub fn domain_of(command: &str) -> Option<&'static str> {
    // Le barre rovesciate se ne vanno prima del confronto: nei comandi veri la
    // parola si scrive dentro una regex — `router\.(get|post)` — e cercare
    // «router.» alla lettera non la trova. Il caso stava gia' fra le prove e le
    // ha fatte cadere alla prima misura sui dati veri.
    let lower = command.to_lowercase().replace('\\', "");
    // Le parole portano il loro contorno — `router.`, `app.get(`, `.spec.` — e
    // «route» nudo non c'è: sui comandi veri prendeva `tsrouter` dentro il
    // comando di uno script che non c'entrava niente. Il dominio «dipendenze» è
    // stato tolto del tutto: si reggeva su `import `, che compare in ogni
    // script, e proponeva `knip` a chi stava leggendo un file.
    // RESTA UN DOMINIO SOLO, dopo la misura sui comandi veri. «schema» e «prove»
    // sono stati tolti: il primo faceva proporre uno script di **migrazione dati**
    // a chi stava leggendo un commento in `schema.prisma`, il secondo rispondeva
    // «npm run test» a chi cercava dove stanno i file di prova — e nessuno dei due
    // risponde alla domanda «quanti». Le parole `endpoint` e `openapi` sono uscite
    // dal riconoscimento del comando per la stessa ragione: comparivano dentro
    // script che **scrivevano** codice, non che lo contavano.
    //
    // Resta il segnale che il reperto ha misurato davvero: un metodo HTTP chiamato
    // su un router. E' stretto, e deve esserlo — su 73 comandi veri che contavano
    // camminando l'albero, la versione larga parlava 29 volte e sbagliava sempre.
    const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];
    let on_a_router =
        lower.contains("router") || lower.contains("app.") || lower.contains("route(");
    // Due forme, ed entrambe compaiono nei comandi veri: la chiamata scritta per
    // esteso (`router.get(`) e l'alternativa dentro una regex
    // (`router\.(get|post|put)`), che e' la forma del comando da cui e' nato il
    // reperto. Cercare solo la prima lo lasciava fuori.
    let literal = HTTP_METHODS
        .iter()
        .any(|m| lower.contains(&format!(".{m}(")));
    let alternation = HTTP_METHODS
        .iter()
        .filter(|m| lower.contains(&format!("{m}|")) || lower.contains(&format!("|{m}")))
        .count()
        >= 2;
    if on_a_router && (literal || alternation) {
        return Some("rotte");
    }
    None
}

/// Gli script di quel `package.json` che parlano di quel dominio.
///
/// `package_json` è il testo del file, non un percorso: la lettura sta nel
/// chiamante, come per il resto di questo crate, così il giudizio si prova senza
/// toccare il disco.
pub fn tools_for(domain: &str, package_json: &str) -> Vec<Tool> {
    // Un nome solo, e di sola lettura: `openapi`. Uno strumento che *stampa* non
    // cambia niente se lo si lancia per curiosita', mentre `db:backfill-public-id`
    // — che la versione larga proponeva — scrive sul database.
    let needles: &[&str] = match domain {
        "rotte" => &["openapi"],
        _ => return Vec::new(),
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(package_json) else {
        return Vec::new();
    };
    let Some(scripts) = parsed.get("scripts").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    let mut found: BTreeSet<Tool> = BTreeSet::new();
    for (name, value) in scripts {
        let command = value.as_str().unwrap_or_default();
        // Si guarda il NOME dello script, non la sua riga di comando: `openapi:dump`
        // dice cosa fa, mentre dentro un comando qualunque la parola può capitare
        // per caso — `create-db@latest --user-agent tanstack/tsrouter` conteneva
        // «route» e faceva proporre uno script d'inizializzazione a chi contava
        // schemi Zod. E la parola dev'essere intera: i nomi degli script sono
        // separati da `:` e `-`, quindi si spezza su quelli.
        let parts: Vec<String> = name
            .to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(|s| s.to_string())
            .collect();
        if needles.iter().any(|n| parts.iter().any(|p| p == n)) {
            found.insert(Tool {
                name: name.clone(),
                command: command.chars().take(90).collect(),
            });
        }
    }
    found.into_iter().collect()
}

/// Il messaggio, o vuoto se non c'è niente da dire.
///
/// Vuoto è il caso normale: si parla solo quando il comando conta camminando
/// l'albero **e** il repo ha davvero uno strumento per quel dominio.
pub fn advice(command: &str, package_json: &str) -> String {
    if !asks_for_a_count(command) {
        return String::new();
    }
    let Some(domain) = domain_of(command) else {
        return String::new();
    };
    let tools = tools_for(domain, package_json);
    if tools.is_empty() {
        return String::new();
    }
    let elenco: Vec<String> = tools
        .iter()
        .map(|t| format!("  npm run {}   ({})", t.name, t.command))
        .collect();
    format!(
        "Stai contando {domain} con una ricerca testuale. Questo repo le stampa \
         già, e il conto del generatore è quello vero:\n{}\n\
         Il 18/08/2026 la stessa domanda sul matching-engine ha dato 168 rotte \
         contro 214 operazioni vere: una regex conta le righe che somigliano a \
         una rotta, il generatore conta le rotte.",
        elenco.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il `package.json` vero del matching-engine, ridotto ai campi che contano.
    const PKG: &str = r#"{
      "scripts": {
        "openapi:dump": "LOG_LEVEL=error tsx scripts/openapi-dump.ts",
        "db:deploy": "tsx scripts/db-deploy.ts",
        "build": "tsc -p tsconfig.json"
      }
    }"#;

    #[test]
    fn a_count_is_recognised_in_the_forms_that_were_measured() {
        assert!(asks_for_a_count("grep -rnE \"router\\.(get|post)\" src | wc -l"));
        assert!(asks_for_a_count("grep -rEc \"router.get\" src"));
        assert!(asks_for_a_count("rg --count endpoint src"));
        // Cercare dove sta una cosa non è contarla: qui il gancio deve tacere.
        assert!(!asks_for_a_count("grep -rn \"router.get\" src"));
        assert!(!asks_for_a_count("sed -n '1,40p' src/routes.ts"));
        // MUTANTE: un `-c` che non appartiene a una ricerca non conta niente.
        assert!(!asks_for_a_count("tsc -p tsconfig.json"));
    }

    #[test]
    fn the_domain_comes_from_the_words_that_appear_in_real_commands() {
        assert_eq!(domain_of("grep -rn \"router.get(\" src | wc -l"), Some("rotte"));
        assert_eq!(domain_of("grep -rc \"app.post(\" src"), Some("rotte"));
        // MUTANTE: le parole che comparivano negli script che SCRIVONO codice non
        // aprono piu' la porta — «endpoint» e «openapi» da sole non bastano.
        assert_eq!(domain_of("python3 - <<PY  # aggiunge un endpoint a main.rs"), None);
        assert_eq!(domain_of("grep -c model prisma/schema.prisma"), None);
        assert_eq!(domain_of("grep -rc TODO src"), None);
    }

    #[test]
    fn only_scripts_that_exist_are_named() {
        let t = tools_for("rotte", PKG);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, "openapi:dump");
        // MUTANTE: un dominio che non e' fra quelli riconosciuti non inventa niente.
        assert!(tools_for("schema", PKG).is_empty());
        // MUTANTE: un package.json illeggibile non fa parlare il gancio.
        assert!(tools_for("rotte", "{ non json").is_empty());
        assert!(tools_for("rotte", "{}").is_empty());
    }

    #[test]
    fn it_speaks_only_when_all_three_conditions_hold() {
        let detto = advice("grep -rnE \"router\\.(get|post)\" src | wc -l", PKG);
        assert!(detto.contains("npm run openapi:dump"), "{detto}");
        assert!(detto.contains("214"), "la misura che lo motiva resta nel messaggio");
        // Cerca, non conta.
        assert_eq!(advice("grep -rn \"router.get\" src", PKG), "");
        // Conta, ma un dominio senza strumento in questo repo.
        assert_eq!(advice("grep -rc TODO src | wc -l", PKG), "");
        // Conta rotte, ma il repo non ha niente.
        assert_eq!(advice("grep -rc \"router.get\" src | wc -l", "{\"scripts\":{}}"), "");
    }
}
