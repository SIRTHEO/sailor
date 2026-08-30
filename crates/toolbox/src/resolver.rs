//! Da un identificativo di strumento all'eseguibile che lo è qui.
//!
//! **PERCHÉ STA IN QUESTO CRATE E NON IN QUELLO DELLE AZIONI.** Chi sa quali
//! strumenti esistono è questo elenco di descrittori; chi esegue è il crate
//! delle azioni, che non deve sapere né dove si cerca né cosa esiste. Il legame
//! è un tratto dichiarato là e attuato qui, e chi compone il registro delle
//! azioni li mette insieme — un solo punto, che si vede.
//!
//! **IL MESSAGGIO DI ERRORE È IL PRODOTTO.** Un flusso scritto su un'altra
//! macchina si incontra qui: se lo strumento non c'è, ciò che questa funzione
//! scrive è tutto quello che chi legge avrà per capire cosa installare. Per
//! questo distingue «non c'è» da «non ho potuto guardare», e riporta la nota del
//! descrittore, che è il posto dove sta scritto da dove si installa.

use crate::{default_sources, probe_one, Catalog, Machine, Presence};

/// Il rilevatore, caricato una volta e interrogato tante.
///
/// **NON CHIEDE MAI UNA VERSIONE**, e non è un'ottimizzazione: chiedere la
/// versione significa *eseguire* un binario, e risolvere un nome non deve
/// eseguire niente. Un flusso che nomina tre strumenti avvierebbe tre processi
/// prima di avviare il proprio lavoro.
pub struct Tools {
    catalog: Catalog,
    machine: Machine,
}

impl Tools {
    /// Gli strumenti di questa macchina, con i descrittori spediti più quelli
    /// dell'utente.
    pub fn current() -> Self {
        let mut machine = Machine::current();
        machine.version_probes = false;
        let catalog = Catalog::load(&default_sources(&machine));
        Self { catalog, machine }
    }

    /// Un elenco e un mondo decisi da chi chiama: è così che una prova verifica
    /// la risoluzione senza dipendere da cosa c'è installato su chi la esegue.
    pub fn new(catalog: Catalog, mut machine: Machine) -> Self {
        machine.version_probes = false;
        Self { catalog, machine }
    }

    /// Esiste un descrittore con questo identificativo?
    ///
    /// **NON È «C'È SULLA MACCHINA», ED È TUTTA LA DIFFERENZA.** Un flusso che
    /// chiede `docker` dove docker non è installato non è un flusso rotto: è un
    /// flusso che qui non gira, e altrove sì. Un flusso che chiede un nome che
    /// nessun catalogo dichiara è rotto ovunque, su qualsiasi macchina, e non
    /// c'è niente da installare per farlo funzionare. Chi controlla un flusso
    /// senza eseguirlo può dire la seconda cosa con certezza e la prima no —
    /// per questo sono separate, e per questo solo una delle due è un errore.
    pub fn declares(&self, id: &str) -> bool {
        self.catalog
            .live()
            .into_iter()
            .any(|loaded| loaded.descriptor.id == id)
    }

    /// Gli identificativi dichiarati, in ordine: è ciò che si mostra a chi ne ha
    /// scritto uno che non esiste.
    pub fn declared_ids(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .catalog
            .live()
            .into_iter()
            .map(|loaded| loaded.descriptor.id.clone())
            .collect();
        names.sort_unstable();
        names
    }
}

/// Quanti identificativi si elencano a chi ne ha chiesto uno che non esiste.
const NAMES_SHOWN: usize = 20;

impl actions::ToolResolver for Tools {
    fn resolve(&self, id: &str) -> Result<String, String> {
        let Some(loaded) = self
            .catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
        else {
            let mut names: Vec<&str> = self
                .catalog
                .live()
                .into_iter()
                .map(|loaded| loaded.descriptor.id.as_str())
                .collect();
            names.sort_unstable();
            let shown = if names.len() > NAMES_SHOWN {
                format!(
                    "{}, e altri {}",
                    names[..NAMES_SHOWN].join(", "),
                    names.len() - NAMES_SHOWN
                )
            } else {
                names.join(", ")
            };
            return Err(format!(
                "nessun descrittore dichiara lo strumento «{id}»; quelli dichiarati sono: {shown}. \
                 Se ne aggiunge uno scrivendo un file JSON in ~/.config/sailor/tools.d/, senza ricompilare niente"
            ));
        };
        let descriptor = &loaded.descriptor;
        if descriptor.detect.is_none() {
            return Err(format!(
                "«{id}» è dichiarato come voce di configurazione da leggere, non come qualcosa da eseguire: un passo non può invocarlo"
            ));
        }
        let finding = probe_one(loaded, &self.machine);
        let note = if descriptor.note.is_empty() {
            String::new()
        } else {
            format!(" Nota del descrittore: {}", descriptor.note)
        };
        match (&finding.presence, &finding.executable) {
            (Presence::Present(_), Some(executable)) => Ok(executable.clone()),
            // Trovato per un percorso che esiste — un'applicazione, un file di
            // configurazione — ma senza un eseguibile a cui parlare.
            (Presence::Present(evidence), None) => Err(format!(
                "«{id}» risulta presente ({evidence}), ma il suo descrittore non dice quale eseguibile invocare: aggiungi una sonda `command` per poterlo usare in un passo.{note}"
            )),
            (Presence::Absent(reason), _) => Err(format!(
                "lo strumento «{id}» ({}) non è su questa macchina: {reason}.{note}",
                if descriptor.label.is_empty() { &descriptor.id } else { &descriptor.label }
            )),
            // La differenza che questo crate esiste per tenere: qui non si dice
            // «installalo», perché non si è potuto guardare.
            (Presence::Undetermined(reason), _) => Err(format!(
                "non ho potuto verificare se «{id}» è su questa macchina: {reason}.{note}"
            )),
        }
    }

    /// La ricetta dichiarata dal descrittore, tradotta nella forma che le azioni
    /// conoscono. Nessuna interpretazione: ciò che non è scritto non c'è, e chi
    /// non la dichiara non diventa usabile per indovinamento.
    fn ask_recipe(&self, id: &str) -> Option<actions::AskRecipe> {
        let loaded = self
            .catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)?;
        let ask = loaded.descriptor.ask.as_ref()?;
        Some(actions::AskRecipe {
            args: ask.args.clone(),
            prompt: match ask.prompt {
                crate::descriptor::PromptPlace::Stdin => actions::PromptVia::Stdin,
                crate::descriptor::PromptPlace::LastArg => actions::PromptVia::LastArg,
            },
            unusable_when: ask.unusable_when.clone(),
            usage: loaded.descriptor.usage.as_ref().map(usage_recipe),
        })
    }
}

/// Il blocco `usage` del descrittore tradotto nella forma che le azioni
/// conoscono. Nessuna interpretazione qui dentro: si copia ciò che c'è scritto,
/// e ciò che non è scritto resta `None` fino in fondo.
fn usage_recipe(usage: &crate::descriptor::Usage) -> actions::UsageRecipe {
    actions::UsageRecipe {
        args: usage.args.clone(),
        declared: actions::Declared {
            read: match usage.read {
                crate::descriptor::ReadAs::Json => actions::Shape::Json,
                crate::descriptor::ReadAs::Text => actions::Shape::Text,
            },
            input_tokens: usage.input_tokens.as_ref().map(pointer),
            output_tokens: usage.output_tokens.as_ref().map(pointer),
            cached_tokens: usage.cached_tokens.as_ref().map(pointer),
            cache_write_tokens: usage.cache_write_tokens.as_ref().map(pointer),
            cache_write_long_tokens: usage.cache_write_long_tokens.as_ref().map(pointer),
            total_tokens: usage.total_tokens.as_ref().map(pointer),
            cost: usage.cost.as_ref().map(pointer),
            model: usage.model.as_ref().map(pointer),
            answer: usage.answer.as_ref().map(pointer),
        },
    }
}

fn pointer(place: &crate::descriptor::Where) -> actions::Pointer {
    match place {
        crate::descriptor::Where::Path(keys) => actions::Pointer::Path(keys.clone()),
        crate::descriptor::Where::Pattern(pattern) => {
            actions::Pointer::Pattern(pattern.clone())
        }
        crate::descriptor::Where::FirstKeyOf { first_key_of } => {
            actions::Pointer::FirstKey(first_key_of.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::Source;
    use actions::ToolResolver;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Una macchina finta: due cartelle nel percorso, nessuna variabile. Le
    /// prove non devono dipendere da cosa è installato su chi le esegue.
    fn machine(dir: &std::path::Path) -> Machine {
        Machine {
            path_dirs: vec![dir.to_path_buf()],
            home: dir.to_path_buf(),
            env: BTreeMap::new(),
            version_probes: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-resolver-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("creare la cartella di prova");
        dir
    }

    fn catalog_of(text: &str, dir: &std::path::Path) -> Catalog {
        let file = dir.join("descrittori.json");
        std::fs::write(&file, text).expect("scrivere i descrittori di prova");
        Catalog::load(&[Source::File(file)])
    }

    fn fake_executable(dir: &std::path::Path, name: &str) {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("scrivere il finto binario");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
    }

    /// **LA MISURA CHE POTEVA VENIRE DIVERSA**: lo stesso identificativo, le
    /// stesse due righe di prova, e l'unica differenza è che il binario c'è o
    /// non c'è. Una sola delle due non proverebbe niente.
    #[test]
    fn an_id_becomes_the_path_of_the_executable_that_is_here() {
        let dir = temp_dir("presente");
        fake_executable(&dir, "un-motore");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "label": "Il motore",
                 "detect": {"command": "un-motore"}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let path = tools.resolve("il-motore").expect("il binario è nel percorso");

        assert_eq!(path, dir.join("un-motore").to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tool_that_is_not_here_says_what_it_looked_for_and_where_it_comes_from() {
        let dir = temp_dir("assente");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "label": "Il motore",
                 "detect": {"command": "un-motore"},
                 "note": "si installa con `npm i -g un-motore`"}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools.resolve("il-motore").expect_err("non c'è nessun binario");

        assert!(reason.contains("il-motore"), "{reason}");
        assert!(reason.contains("un-motore"), "{reason}");
        assert!(reason.contains("npm i -g"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_nobody_declared_lists_the_ones_that_exist() {
        let dir = temp_dir("ignoto");
        let catalog = catalog_of(
            r#"[{"id": "il-motore", "family": "ai_cli", "detect": {"command": "un-motore"}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools.resolve("un-altro").expect_err("nessuno lo dichiara");

        assert!(reason.contains("un-altro"), "{reason}");
        assert!(reason.contains("il-motore"), "{reason}");
        assert!(reason.contains("tools.d"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I server MCP si scoprono leggendo un file: sono voci, non binari, e
    /// chiederne uno per eseguirlo è un errore di chi ha scritto il passo.
    #[test]
    fn a_configuration_entry_is_not_something_to_invoke() {
        let dir = temp_dir("voce");
        let catalog = catalog_of(
            r#"[{"id": "i-server", "family": "mcp_server",
                 "enumerate": {"json_keys": {"files": ["~/x.json"], "pointer": ["mcpServers"]}}}]"#,
            &dir,
        );
        let tools = Tools::new(catalog, machine(&dir));

        let reason = tools.resolve("i-server").expect_err("non è un eseguibile");

        assert!(reason.contains("non come qualcosa da eseguire"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **IL DATO NUOVO DEVE ARRIVARE FINO AL PUNTO DI INVOCAZIONE.** Un blocco
    /// `usage` scritto in un descrittore e mai tradotto in `AskRecipe` è un
    /// campo che si carica e non serve a niente: la misura resterebbe sempre
    /// sconosciuta, e nessuna prova del motore se ne accorgerebbe — perché al
    /// motore non arriverebbe mai una ricetta che lo dichiara. Questo è il
    /// pezzo di strada che nessun'altra prova percorre.
    #[test]
    fn the_usage_block_travels_from_the_descriptor_into_the_recipe() {
        let dir = temp_dir("usage-in-ricetta");
        fake_executable(&dir, "misurabile");
        let catalog = catalog_of(
            r#"[{
              "id": "misurabile", "family": "ai_cli",
              "detect": { "command": "misurabile" },
              "ask": { "args": ["-p"], "prompt": "stdin" },
              "usage": {
                "args": ["--output-format", "json"],
                "read": "json",
                "input_tokens": ["usage", "input_tokens"],
                "cached_tokens": ["usage", "cache_read_input_tokens"],
                "model": ["model"],
                "answer": ["result"]
              }
            }]"#,
            &dir,
        );
        let tools = Tools { catalog, machine: machine(&dir) };

        let recipe = tools.ask_recipe("misurabile").expect("la ricetta c'è");
        let usage = recipe.usage.expect("e porta con sé il consumo");
        assert_eq!(usage.args, vec!["--output-format", "json"]);
        assert_eq!(usage.declared.read, actions::Shape::Json);
        assert_eq!(
            usage.declared.input_tokens,
            Some(actions::Pointer::Path(vec![
                "usage".to_owned(),
                "input_tokens".to_owned()
            ]))
        );
        assert_eq!(
            usage.declared.cached_tokens,
            Some(actions::Pointer::Path(vec![
                "usage".to_owned(),
                "cache_read_input_tokens".to_owned()
            ])),
            "la cache arriva con un puntatore suo, o il criterio 3 cade qui"
        );
        assert_eq!(
            usage.declared.answer,
            Some(actions::Pointer::Path(vec!["result".to_owned()]))
        );
        assert_eq!(usage.declared.output_tokens, None, "e ciò che non è scritto resta assente");
    }

    /// Un descrittore senza `usage` produce una ricetta senza consumo: quel
    /// motore si invoca come prima, e i suoi token restano sconosciuti.
    #[test]
    fn a_descriptor_without_usage_gives_a_recipe_without_it() {
        let dir = temp_dir("usage-assente");
        fake_executable(&dir, "muto");
        let catalog = catalog_of(
            r#"[{
              "id": "muto", "family": "ai_cli",
              "detect": { "command": "muto" },
              "ask": { "args": ["-p"], "prompt": "stdin" }
            }]"#,
            &dir,
        );
        let tools = Tools { catalog, machine: machine(&dir) };

        let recipe = tools.ask_recipe("muto").expect("la ricetta c'è");
        assert!(recipe.usage.is_none());
        assert_eq!(recipe.args, vec!["-p"], "il resto della ricetta è intatto");
    }

    /// Il descrittore spedito di `codex` arriva fino alla ricetta con la sua
    /// espressione, e senza opzioni in più: la riga di comando di codex non
    /// cambia perché adesso lo si misura.
    #[test]
    fn the_shipped_codex_recipe_carries_its_pattern_and_adds_no_arguments() {
        let dir = temp_dir("codex-spedito");
        fake_executable(&dir, "codex");
        let catalog = Catalog::load(&[Source::Builtin]);
        let tools = Tools { catalog, machine: machine(&dir) };

        let recipe = tools.ask_recipe("codex").expect("codex ha una ricetta");
        let usage = recipe.usage.expect("e dichiara come si legge il suo consumo");
        assert!(usage.args.is_empty(), "niente opzioni aggiunte a codex");
        assert_eq!(usage.declared.read, actions::Shape::Text);
        let Some(actions::Pointer::Pattern(pattern)) = usage.declared.total_tokens else {
            panic!("codex dichiara il totale con un'espressione")
        };
        // E l'espressione riconosce davvero l'uscita di codex, non una che le
        // somiglia: se fosse scritta male, il consumo resterebbe sconosciuto
        // per sempre senza che nessuno se ne accorga.
        let reading = actions::Reading::default();
        let _ = reading;
        let letto = models_read(&pattern, "roba\ntokens used\n13.910\naltro");
        assert_eq!(letto, Some(13_910));
    }

    /// Legge un totale con l'espressione data, passando dalla stessa funzione
    /// che il motore userà.
    fn models_read(pattern: &str, said: &str) -> Option<u64> {
        let declared = actions::Declared {
            read: actions::Shape::Text,
            total_tokens: Some(actions::Pointer::Pattern(pattern.to_owned())),
            ..actions::Declared::default()
        };
        actions::read_declared(said, &declared).total_tokens
    }

}
