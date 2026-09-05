//! What a call cost, and where it is written down: the price list, the row
//! the ledger keeps for every call, and the counter that keeps two rows apart.

use crate::candidates::Candidate;
use crate::{Reading, EXTERNAL_ENGINE_ACTION};
use flow::SharedState;
use ledger::{EngineIdentity, Ledger, ModelCallRecord};

// ── quanto è costata una chiamata ────────────────────────────────────────

/// Dove sta il listino locale su questa macchina.
///
/// Risposta a «il listino deve essere modificabile senza ricompilare»: è un
/// file JSON nella casa di Sailor, accanto al deposito e ai flussi, e si
/// riscrive con un editor di testo. `SAILOR_PRICING` lo sposta altrove — serve
/// alle prove, e a chi tiene più listini.
///
/// **NON STA IN `modelli.json`**, che è la *scelta* dell'utente su quale
/// modello usare: mescolare «cosa voglio» e «quanto costa» farebbe sì che
/// cambiare una preferenza tocchi un listino, e viceversa.
const PRICING_ENV: &str = "SAILOR_PRICING";
const PRICING_FILE: &str = "pricing.json";

/// Il listino da applicare: quello spedito col prodotto, sovrascritto da quello
/// scritto in casa.
///
/// **PURO, E NON È UN VEZZO.** Il testo di casa entra come argomento e il
/// listino esce: così la regola — «senza niente in casa il costo si sa lo
/// stesso» — si interroga senza toccare il disco e senza scrivere una variabile
/// d'ambiente, che è di **processo** e rovinerebbe le prove che girano in
/// parallelo nello stesso. Chi legge il file sta in [`load_pricing`].
///
/// **UN FILE DI CASA ILLEGGIBILE NON TOGLIE IL LISTINO A TUTTI.** Si torna a
/// quello spedito: prima del 01/09/2026 un JSON scritto male lasciava il costo
/// sconosciuto per l'intera corsa, che è il guasto 35 nella sua forma più
/// silenziosa — un errore di battitura che spegne un tetto di spesa.
pub fn price_list_from(home_text: Option<&str>) -> models::pricing::PriceList {
    let shipped = models::pricing::shipped();
    match home_text.and_then(|text| models::pricing::PriceList::parse(text).ok()) {
        Some(home) => shipped.overridden_by(home),
        None => shipped,
    }
}

/// Il listino di questa macchina: quello spedito, sovrascritto dal file di casa.
///
/// **RILETTO A OGNI CHIAMATA, NON TENUTO IN MEMORIA**: un prezzo cambiato a metà
/// di una corsa lunga vale dalla chiamata dopo, invece che dal prossimo riavvio.
/// Il costo è una lettura di un file piccolo accanto all'avvio di un processo
/// esterno — cioè niente, in confronto a ciò che sta per succedere.
///
/// **PUBBLICA PERCHÉ `sailor flow check` DEVE POTER DIRE COSA NON SA PREZZARE.**
/// Un freno che non frena si deve vedere prima di lanciare, e chi lo mostra è un
/// comando, non questo crate.
pub fn current_price_list() -> models::pricing::PriceList {
    let path = match std::env::var_os(PRICING_ENV).filter(|value| !value.is_empty()) {
        Some(declared) => Some(std::path::PathBuf::from(declared)),
        None => ledger::sailor_home().map(|home| home.join(PRICING_FILE)),
    };
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    price_list_from(text.as_deref())
}

/// Dove registrare quanto si è speso: deposito, corsa e passo.
///
/// Servono tutti e tre. Senza uno solo **non si scrive nessuna riga**, invece
/// di scriverne una attribuita a nessuno: una riga senza corsa non si somma con
/// nessun'altra e sporcherebbe i conti peggio di una riga mancante. È la stessa
/// regola che `sink_for_step` applica già al testo dal vivo.
pub(crate) struct Recording<'a> {
    pub(crate) ledger: &'a Ledger,
    pub(crate) run_id: String,
    pub(crate) step_id: String,
}

pub(crate) fn recording_for<'a>(ledger: &'a Option<Ledger>, shared: &SharedState) -> Option<Recording<'a>> {
    Some(Recording {
        ledger: ledger.as_ref()?,
        run_id: shared.get(flow::CURRENT_RUN)?.as_str()?.to_owned(),
        step_id: shared.get(flow::CURRENT_STEP)?.as_str()?.to_owned(),
    })
}

/// Un contatore di processo, perché due chiamate nello stesso secondo dentro lo
/// stesso passo non si sovrascrivano a vicenda: `call_id` è chiave primaria, e
/// una collisione farebbe sparire una spesa invece di sommarla.
static CALLS_SO_FAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// Ciò che si sa di una chiamata appena finita, prima di darle un prezzo.
pub(crate) struct Spent {
    pub(crate) reading: Reading,
    pub(crate) error_type: Option<&'static str>,
    pub(crate) started_at: i64,
    pub(crate) ended_at: i64,
    /// La sessione sotto cui questa chiamata è girata, quando si sa qual è.
    pub(crate) session_id: Option<String>,
    /// Con quale identità il processo è partito: quale casa, e come è stata
    /// scelta. Senza, due corse dello stesso flusso non sono la stessa misura —
    /// e la riga non porta la ragione per cui i due consumi differiscono.
    pub(crate) identity: EngineIdentity,
    /// The kind of work the step declared, for the sum per kind.
    pub(crate) work_kind: Option<String>,
}

/// Scrive nel deposito la riga di **questa** chiamata.
///
/// **SI SCRIVE ANCHE QUANDO È ANDATA MALE**, ed è una scelta deliberata: un
/// turno interrotto brucia comunque la quota, e azzerarne il costo
/// sottostimerebbe la spesa esattamente nei minuti che precedono un
/// esaurimento — cioè quando la misura serve. Separare «lavoro utile» da
/// «quota consumata» è compito di chi legge le righe, non di chi le scrive.
///
/// **E SI SCRIVE ANCHE QUANDO I TOKEN SONO SCONOSCIUTI.** «Questo motore è
/// stato chiamato quaranta volte, token non dichiarati» è un'informazione su
/// cui si può agire; il silenzio nasconde il buco, e un totale che si presenta
/// come completo mentre è parziale è la bugia da cui questo lavoro nasce.
///
/// Un fallimento del deposito non rompe il passo: la misura è al servizio del
/// lavoro, non il contrario, e far fallire una chiamata già riuscita perché non
/// si è potuto annotarla sarebbe il contrario di ciò che si sta costruendo.
pub(crate) fn record_the_call(
    record: &Recording<'_>,
    candidate: &Candidate,
    tried_before: &[String],
    spent: Spent,
) {
    let Some(cli) = candidate.id.as_deref() else {
        // Un `bin` scritto a mano nel passo non è una chiamata a un modello:
        // `sh -c echo` non consuma nessuna quota, e riempirne il deposito
        // renderebbe illeggibile proprio la vista che questo lavoro esiste per
        // rendere leggibile.
        return;
    };
    if !candidate.can_be_asked {
        // **E NON LO È NEMMENO UNO STRUMENTO CHE NON SI PUÒ INTERROGARE.**
        // `git` e `cargo` stanno nel catalogo, si eseguono da un passo, e non
        // consumano quota di nessun abbonamento. Contarli fra le chiamate ai
        // modelli non è solo rumore: arrivano senza costo, quindi rendono
        // `Spend::is_complete()` falso su **ogni** corsa vera — misurato il
        // 31/08/2026 sul deposito di questa macchina, tre righe su ventiquattro
        // — e la frase d'onestà del tetto («la spesa vera è più alta») si
        // accende sempre, anche quando non c'è niente di ignoto. Un avviso
        // sempre acceso non lo legge nessuno, e a perdersi è quello vero.
        return;
    }
    let reading = spent.reading;
    let price_list = current_price_list();
    // Il legame col listino passa dal nome che il motore stesso dichiara, non
    // da un'ipotesi: un modello presunto sarebbe un numero inventato con la
    // faccia di una misura, creduto per sempre da chiunque lo legga.
    let entry = reading
        .model
        .as_deref()
        .and_then(|name| price_list.find(name));
    let prices = entry
        .map(models::pricing::Price::micros)
        .unwrap_or_default();
    let cost_micros = models::pricing::cost_micros(
        models::pricing::TokenCounts {
            input: reading.input_tokens,
            output: reading.output_tokens,
            cached: reading.cached_tokens,
            cache_write: reading.cache_write_tokens,
            cache_write_long: reading.cache_write_long_tokens,
        },
        prices,
    );
    let sequence = CALLS_SO_FAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let written = ModelCallRecord {
        call_id: format!(
            "{}:{}:{}:{sequence}",
            record.run_id, record.step_id, spent.started_at
        ),
        run_id: record.run_id.clone(),
        step_id: Some(record.step_id.clone()),
        purpose: EXTERNAL_ENGINE_ACTION.to_owned(),
        cli: cli.to_owned(),
        // Un passo nomina lo strumento, non il modello: nessuno qui *chiede* un
        // modello, e scriverne uno sarebbe inventarlo. Vuoto vuol dire «non
        // dichiarato», e la finestra lo mostra come tale.
        requested_model: String::new(),
        actual_model: reading.model.clone().unwrap_or_default(),
        // I turni arrivano dalla stessa uscita da cui arrivano i token, e
        // finora venivano buttati via. Sono la quantita' che spiega perche' una
        // catena di passi costa piu' di una sessione sola.
        turns: reading.turns,
        input_tokens: reading.input_tokens,
        output_tokens: reading.output_tokens,
        cached_tokens: reading.cached_tokens,
        cache_write_tokens: reading.cache_write_tokens,
        cache_write_long_tokens: reading.cache_write_long_tokens,
        total_tokens: reading.total_tokens,
        cost_micros,
        // Il costo del motore accanto al nostro, mai al posto suo.
        declared_cost_micros: reading
            .declared_cost
            .map(|usd| (usd * 1_000_000.0).round() as i64),
        // La valuta è quella del listino con cui si è calcolato: senza un conto
        // fatto non c'è nessuna valuta da dichiarare.
        price_currency: cost_micros.map(|_| price_list.currency.clone()),
        input_price_micros_per_million: prices.input,
        output_price_micros_per_million: prices.output,
        cached_price_micros_per_million: prices.cached,
        cache_write_price_micros_per_million: prices.cache_write,
        cache_write_long_price_micros_per_million: prices.cache_write_long,
        // **CON QUALE IDENTITÀ QUESTO PROCESSO È PARTITO.** Non «sotto quale
        // profilo»: quale casa, e come è stata scelta. La differenza è il difetto
        // che questa riga esisteva per avere e non aveva — un passo che scriveva
        // da sé la variabile di casa faceva partire il motore altrove, e qui
        // finiva scritto il nome del profilo attivo.
        engine_identity: spent.identity,
        retry_chain: tried_before.to_vec(),
        error_type: spent.error_type.map(str::to_owned),
        started_at: spent.started_at,
        ended_at: Some(spent.ended_at),
        session_id: spent.session_id,
        work_kind: spent.work_kind,
    };
    let _ = record.ledger.record_model_call(&written);
}

#[cfg(test)]
mod what_it_cost {
    //! Le prove della misura: quanto ha consumato una chiamata, dove finisce
    //! scritta, e che cosa succede a chi non lo dichiara.
    //!
    //! **NESSUN MOTORE VERO E NESSUNA CHIAMATA A PAGAMENTO.** I motori qui
    //! dentro sono script di shell scritti al volo, come quelli che il resto di
    //! questo file usa già: sono l'unico modo di provare una misura senza
    //! comprarla.

    use super::*;
    use crate::cooldown;
    use crate::engine::ExternalEngineAction;
    use crate::recipe::{AskRecipe, PromptVia, ToolResolver, UsageRecipe};
    use crate::{Declared, Pointer, Shape};
    use flow::{Action, ActionOutcome};
    use ledger::Ledger;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-consumo-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// Uno script eseguibile che si comporta come gli si dice.
    fn fake_engine(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("scrivere il finto motore");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("renderlo eseguibile");
        path.to_string_lossy().into_owned()
    }

    /// Il listino di prova, con la cache a un decimo dell'ingresso: è la
    /// differenza che il criterio 3 del mandato esiste per non perdere.
    const PRICE_LIST: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        { "id": "modello-di-prova", "aliases": ["prova"],
          "input_per_million": 3.0, "output_per_million": 15.0,
          "cached_per_million": 0.3 }
      ]
    }"#;

    fn write_price_list(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("pricing.json");
        std::fs::write(&path, PRICE_LIST).expect("scrivere il listino");
        path
    }

    /// Uno stato condiviso come quello che l'esecutore prepara prima di ogni
    /// azione: corsa e passo, sotto le chiavi riservate di `flow`.
    fn shared(run: &str, step: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
        shared
    }

    /// Un risolutore che punta a uno script e gli attacca la ricetta che gli si
    /// dà: è il posto dove, nella vita vera, arriva un descrittore.
    struct Declares {
        bin: String,
        recipe: Option<AskRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "motore-di-prova" => Ok(self.bin.clone()),
                other => Err(format!("«{other}» non è su questa macchina")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            self.recipe.clone()
        }
    }

    fn path(keys: &[&str]) -> Option<Pointer> {
        Some(Pointer::Path(
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ))
    }

    /// La ricetta di un motore che sa dire quanto ha consumato: chiede
    /// l'involucro e dichiara dove stanno i numeri, il modello e la risposta.
    fn declaring_recipe() -> AskRecipe {
        AskRecipe {
            args: Vec::new(),
            prompt: PromptVia::Stdin,
            args_before_prompt: Vec::new(),
            unusable_when: Vec::new(),
            silent_without_prompt: false,
            refuses_without_prompt: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            usage: Some(UsageRecipe {
                args: vec!["--output-format".to_owned(), "json".to_owned()],
                declared: Declared {
                    read: Shape::Json,
                    from: models::usage::Heard::Stdout,
                    input_tokens: path(&["usage", "input_tokens"]),
                    output_tokens: path(&["usage", "output_tokens"]),
                    cached_tokens: path(&["usage", "cache_read_input_tokens"]),
                    cache_write_tokens: path(&["usage", "cache_creation_input_tokens"]),
                    cache_write_long_tokens: None,
                    total_tokens: None,
                    turns: None,
                    cost: path(&["total_cost_usd"]),
                    model: path(&["model"]),
                    answer: path(&["result"]),
                },
            }),
        }
    }

    /// Un motore che risponde con l'involucro **solo** se gli si è chiesto
    /// `--output-format json`, e in chiaro altrimenti: è il comportamento vero
    /// di una riga di comando, e senza di lui la prova sull'uscita invariata
    /// non proverebbe niente.
    const WRAPS_ON_DEMAND: &str = r#"cat > /dev/null
printf '%s\n' "$@" > "$(dirname "$0")/argv"
if [ "$1" = "--output-format" ] && [ "$2" = "json" ]; then
  printf '{"result":"la risposta vera","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'
else
  printf 'la risposta vera'
fi"#;

    /// **UN MOTORE CHE RISPONDE IN JSON SENZA CHE NESSUNO GLIEL'ABBIA CHIESTO.**
    /// Serve a provare che il consumo si legge perché un DESCRITTORE lo
    /// dichiara, non perché l'uscita per caso somiglia a un formato noto: se
    /// qui dentro comparisse un ramo cablato su chiavi di un fornitore, i suoi
    /// token verrebbero letti lo stesso, ed è esattamente ciò che il vincolo di
    /// indipendenza dal modello vieta.
    const ALWAYS_WRAPS: &str = r#"cat > /dev/null
printf '{"result":"la risposta vera","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'"#;

    /// La riga di comando con cui il finto motore è stato davvero invocato.
    fn argv_of(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("argv"))
            .expect("il motore finto ha scritto la propria riga di comando")
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn calls_in(dir: &std::path::Path) -> Vec<ledger::ModelCallRecord> {
        let ledger = Ledger::open(dir).expect("riaprire il deposito");
        let dump = ledger
            .projection_dump()
            .expect("il deposito sa dire cosa contiene");
        // **UNA SOLA LETTURA DELLA PROIEZIONE, E NON È PIÙ QUI.**
        //
        // Fino al 01/09/2026 questo modulo teneva una copia privata di
        // `ui::parse::parse_model_call_row` — ventotto indici scritti a mano,
        // uguali a quelli dell'originale — perché «`actions` non dipende da
        // `ui`, e la dipendenza inversa sarebbe un ciclo». Non lo era: `ui` non
        // ha mai dipeso da `actions`, e comunque un ciclo di sole prove cargo lo
        // ammette apposta, com'è scritto nel `Cargo.toml` di `flow`.
        //
        // Il costo di quella copia era preciso: una colonna spostata avrebbe
        // fatto sbagliare **le due letture allo stesso modo**, e le prove che
        // confrontano l'una con l'altra sarebbero rimaste verdi. Adesso a
        // leggere è una sola, e a tenerla onesta c'è
        // `ledger::MODEL_CALL_DUMP_COLUMNS`, che non è né la lettura né la
        // scrittura.
        ui::parse::parse_model_calls(&dump)
    }

    /// Il listino vive in un file, e le prove non devono contendersi la casa di
    /// chi le esegue: `SAILOR_PRICING` lo sposta. Una serratura perché le prove
    /// girano in parallelo nello stesso processo e la variabile d'ambiente è una
    /// sola — senza, due prove si toglierebbero il listino a vicenda.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_price_list<T>(price_list: Option<&std::path::Path>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match price_list {
            Some(path) => std::env::set_var(PRICING_ENV, path),
            None => std::env::set_var(PRICING_ENV, "/nessun/listino/qui"),
        }
        let out = body();
        std::env::remove_var(PRICING_ENV);
        out
    }

    // ── (a) chi dichiara: token veri, cache a parte, costo dal listino ─

    /// **IL CRITERIO 2 E IL CRITERIO 3 INSIEME.** Un motore che dichiara come si
    /// legge il suo consumo produce una riga nel deposito con i token veri, la
    /// cache in una colonna sua, e il costo calcolato dal listino locale — non
    /// quello che il motore stesso dichiara.
    #[test]
    fn a_declaring_engine_writes_a_row_with_true_tokens_and_a_cost_from_the_price_list() {
        let dir = scratch("dichiara");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");
        let ActionOutcome::Went(output) = outcome else {
            panic!("un motore che risponde è sempre Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        let call = &calls[0];
        assert_eq!(call.run_id, "corsa-1");
        assert_eq!(call.step_id.as_deref(), Some("passo-1"));
        assert_eq!(call.cli, "motore-di-prova");
        assert_eq!(call.actual_model, "modello-di-prova");
        assert_eq!(call.input_tokens, Some(1_000_000));
        assert_eq!(call.output_tokens, Some(1_000_000));
        assert_eq!(
            call.cached_tokens,
            Some(1_000_000),
            "la cache ha una colonna sua e non finisce dentro l'ingresso"
        );
        // 1M a 3 $ + 1M a 15 $ + 1M di cache a 0,30 $ = 18,30 $ = 18 300 000 micro.
        assert_eq!(call.cost_micros, Some(18_300_000));
        assert_eq!(call.price_currency.as_deref(), Some("USD"));
        assert_eq!(call.cached_price_micros_per_million, Some(300_000));
        // Il costo che il motore dichiara di suo sta accanto, mai al posto:
        // 0,5 $ è volutamente diverso dal conto del listino.
        assert_eq!(call.declared_cost_micros, Some(500_000));
        assert_eq!(call.error_type, None);
        assert!(call.ended_at.is_some());

        // E l'uscita del passo è il testo, non l'involucro.
        assert_eq!(output["stdout"], "la risposta vera");
    }

    /// **IL CRITERIO 3, DALLA PARTE IN CUI SI ROMPE.** Se la cache fosse contata
    /// al prezzo dell'ingresso invece che al suo, questo costo verrebbe dieci
    /// volte più caro sulla parte della cache. La prova sopra fissa il numero;
    /// questa dice perché quel numero e non un altro.
    #[test]
    fn cache_priced_as_input_would_cost_ten_times_more() {
        let solo_cache = models::pricing::cost_micros(
            models::pricing::TokenCounts {
                input: Some(0),
                output: Some(0),
                cached: Some(1_000_000),
                ..models::pricing::TokenCounts::default()
            },
            models::pricing::PriceList::parse(PRICE_LIST)
                .unwrap()
                .find("prova")
                .unwrap()
                .micros(),
        );
        assert_eq!(solo_cache, Some(300_000), "1M di cache costa 0,30 $");
        assert!(
            solo_cache.unwrap() * 5 < 3_000_000,
            "e non i 3,00 $ che costerebbe come ingresso fresco"
        );
    }

    // ── (b) chi non dichiara niente: identico, e sconosciuto ───────────

    /// **IL CRITERIO 4.** Un motore senza blocco `usage` produce la stessa
    /// identica uscita di prima, e la sua riga porta i token a SCONOSCIUTO.
    /// Mai zero: uno zero si somma, e nessuna vista a valle può correggerlo.
    #[test]
    fn an_engine_that_declares_nothing_is_unchanged_and_leaves_the_tokens_unknown() {
        let dir = scratch("non-dichiara");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-2", "passo-2"))
        })
        .expect("il motore risponde");
        let ActionOutcome::Went(output) = outcome else {
            panic!("un motore che risponde è sempre Went")
        };

        // Stessa uscita di sempre: nessun campo in più, nessun involucro.
        assert_eq!(output["status"], "ok");
        assert_eq!(output["stdout"], "la risposta vera");
        assert_eq!(
            output.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["status", "stdout", "stderr"],
            "l'uscita del passo non guadagna campi perché qualcuno misura"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata si registra comunque");
        let call = &calls[0];
        assert_eq!(call.input_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.output_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.cached_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.total_tokens, None);
        assert_eq!(call.cost_micros, None, "senza token non c'è nessun costo");
        assert_eq!(call.actual_model, "", "nessun modello dichiarato");
    }

    // ── (c) una chiamata fallita scrive comunque la sua riga ───────────

    /// **IL CRITERIO 5.** Un motore uscito in errore scrive la sua riga con la
    /// causa: un turno interrotto brucia comunque la quota, e azzerarne il
    /// costo sottostimerebbe la spesa proprio nei minuti che precedono un
    /// esaurimento.
    #[test]
    fn a_failed_call_still_writes_its_row_with_the_cause() {
        let dir = scratch("fallita");
        let bin = fake_engine(
            &dir,
            "motore",
            "cat > /dev/null\necho 'è andata male' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-3", "passo-3"))
        })
        .expect_err("un'uscita diversa da zero rompe il passo");
        assert_eq!(error.class, "engine_exit_error");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "anche un fallimento lascia la sua riga");
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
        assert_eq!(calls[0].cli, "motore-di-prova");
        assert_eq!(calls[0].input_tokens, None, "non ha fatto in tempo a dirlo");
    }

    /// **ESAURITO E ROTTO SONO DUE COSE, ANCHE QUANDO IL MOTORE È UNO SOLO.**
    ///
    /// Il guasto 14, per esteso. Il 29/08/2026 Claude era al limite settimanale
    /// e il passo si è fermato con un errore che diceva «uscito in errore»: chi
    /// l'ha letto è andato a cercare un guasto che non c'era, mentre la cosa da
    /// fare era aspettare le sette o cambiare motore. La distinzione esisteva
    /// già nel codice ma valeva **solo con una catena** (`!solo && ...`), cioè
    /// mai nel caso in cui è capitato.
    ///
    /// Si guarda in due posti perché sono due lettori diversi: la classe
    /// dell'errore la legge una persona adesso, `error_type` nel deposito la
    /// legge una somma fra un mese — e una somma che mescola le quote finite coi
    /// guasti veri non dice niente a nessuno.
    #[test]
    fn a_single_engine_that_ran_out_is_not_reported_as_broken() {
        let dir = scratch("esaurito-da-solo");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-esaurita", "passo-1"))
        })
        .expect_err("un motore esaurito e solo non può fare il lavoro");

        assert_eq!(
            error.class, "engine_exhausted",
            "non «engine_exit_error»: chi legge deve sapere che è finita la quota"
        );
        assert!(
            error.said.contains("quota"),
            "e il messaggio lo dice a parole: {}",
            error.said
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata ha bruciato quota: la riga c'è");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "e la riga distingue la quota finita da un guasto"
        );
    }

    /// **UN GUASTO VERO RESTA UN GUASTO.** La gemella della prova sopra: stesso
    /// motore solo, stessa ricetta con le stesse parole di esaurimento, ma
    /// un'uscita che quelle parole non le contiene. Senza questa, far dire
    /// «esaurito» a *qualunque* fallimento passerebbe verde.
    #[test]
    fn a_single_engine_that_truly_broke_is_still_reported_as_broken() {
        let dir = scratch("rotto-da-solo");
        let bin = fake_engine(
            &dir,
            "motore-rotto",
            "cat > /dev/null\necho 'errore: il mandato non ha senso' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-rotta", "passo-1"))
        })
        .expect_err("un'uscita diversa da zero rompe il passo");

        assert_eq!(error.class, "engine_exit_error");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
    }

    /// A spent quota is its own class, and the engine is set aside for the
    /// time its descriptor declares: the second step in the same window does
    /// not knock on it. Without `exhausted_when` the same output stays the
    /// plain `exhausted` of before, and nobody is set aside.
    #[test]
    fn a_spent_quota_is_its_own_class_and_sets_the_engine_aside() {
        let dir = scratch("quota-spesa");
        let bin = fake_engine(
            &dir,
            "motore-a-secco",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(1800);
        let aside = dir.join("cooldowns.json");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(ledger))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-a-secco", "passo-1"))
        })
        .expect_err("a spent engine alone cannot do the work");
        assert_eq!(error.class, "engine_exhausted");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("quota_exhausted"));
        let set = cooldown::set_aside_until(&aside, "motore-di-prova", now_secs()).expect("set aside");
        assert!(set.said.contains("weekly limit"), "{set:?}");

        // The second knock is refused before spending, and says until when.
        let again = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-a-secco", "passo-2"))
        })
        .expect_err("an engine set aside is not tried");
        assert_eq!(again.class, "no_usable_engine");
        assert!(again.said.contains("set aside until"), "{}", again.said);
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "nothing was spent on the second knock");

        // The control: the same words without `exhausted_when` are the old class, and nobody is aside.
        let plain = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe { exhausted_when: Vec::new(), cooldown_secs: None, ..recipe }),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("second ledger")))
        .cooling_down_in(Some(dir.join("cooldowns-2.json")));
        with_price_list(None, || plain.execute(&input, &mut shared("corsa-piana", "passo-1")))
            .expect_err("still cannot work");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].error_type.as_deref(), Some("exhausted"));
        assert!(cooldown::set_aside_until(&dir.join("cooldowns-2.json"), "motore-di-prova", now_secs()).is_none());
    }

    /// A cap per engine on a window, declared by the person in a file: the
    /// first priced call fits, the second finds the window full and is refused
    /// before spending, naming the sum. A cap on another engine changes nothing.
    #[test]
    fn an_engine_over_its_budget_is_refused_before_spending() {
        let dir = scratch("tetto");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let budgets = dir.join("budgets.json");
        // One priced call costs 18.30 $ and is checked before it is made: under
        // a cap of 10 $ the first goes through, and fills the window.
        std::fs::write(
            &budgets,
            r#"{"motore-di-prova": {"cap_micros": 10000000, "window_secs": 3600}}"#,
        )
        .expect("write the caps");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger))
        .budgeted_by(Some(budgets));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("the first call fits under the cap");
        let refused = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-2", "passo-1"))
        })
        .expect_err("the second call finds the window full");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("over its budget: spent 18.3000 $ of 10.0000 $"), "{}", refused.said);
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "the refusal spent nothing");

        // The control: a cap declared for some other engine does not bind this one.
        let others = dir.join("budgets-others.json");
        std::fs::write(&others, r#"{"another-engine": {"cap_micros": 1, "window_secs": 3600}}"#)
            .expect("write the other caps");
        let unbound = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("reopen")))
        .budgeted_by(Some(others));
        with_price_list(Some(&price_list), || {
            unbound.execute(&input, &mut shared("corsa-3", "passo-1"))
        })
        .expect("a cap on another engine is not this engine's");
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// **A DOOR KNOWN TO BE SHUT IS NOT KNOCKED ON AGAIN.** The file's
    /// arithmetic was proved; the chain that fills it was not. Here a real
    /// engine says its quota is spent, and the next chain refuses it without
    /// starting it — naming until when, and what it said.
    #[test]
    fn an_engine_that_said_its_quota_was_spent_is_not_started_again() {
        let dir = scratch("da-parte");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
        );
        let aside = dir.join("cooldowns.json");
        let mut recipe = declaring_recipe();
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(3_600);
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("aprire il deposito")))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        // The first chain runs it, and it says the quota is spent.
        let broke = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect_err("a spent quota is not a step that went");
        assert_eq!(broke.class, "engine_exhausted");
        assert!(aside.exists(), "nothing was set aside: {}", broke.said);

        // The second chain does not start it at all: the refusal is the list's.
        let refused = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-2", "passo-1"))
        })
        .expect_err("the door is known to be shut");
        assert!(
            refused.said.contains("set aside until") && refused.said.contains("weekly limit"),
            "the refusal says neither until when nor what it said: {}",
            refused.said
        );
        assert_eq!(
            calls_in(&dir.join("deposito")).len(),
            1,
            "the second chain started the engine again"
        );

        // THE CONTROL: past its time the same engine is knocked on again, or
        // the code could set one aside for ever and pass. The instant is read
        // from the list and never recomputed: guessing it as `now + 3600`
        // guesses what the clock said when the file was written, and one second
        // of load left the file untouched and this arm red for nothing.
        let past = std::fs::read_to_string(&aside).expect("the list");
        let mut written: serde_json::Value = serde_json::from_str(&past).expect("the list is JSON");
        let now = now_secs();
        for (_, aside) in written.as_object_mut().expect("one entry per engine") {
            aside["until"] = json!(now - 1);
        }
        std::fs::write(&aside, serde_json::to_string(&written).expect("write it back"))
            .expect("bring its time forward");
        let again = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-3", "passo-1"))
        })
        .expect_err("it is spent again, but it was asked");
        assert!(
            !again.said.contains("set aside until"),
            "past its time it was still refused from the list: {}",
            again.said
        );
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// An engine that resolves, with the pact its descriptor would declare.
    struct Pacted {
        bin: String,
        pact: models::pact::DataPact,
    }

    impl ToolResolver for Pacted {
        fn resolve(&self, _id: &str) -> Result<String, String> {
            Ok(self.bin.clone())
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn data_pact(&self, _id: &str) -> models::pact::DataPact {
            self.pact
        }
    }

    /// A step that says its text is private never resolves to an engine whose
    /// pact is `trains` or `unknown`, and the refusal names the pact; the same
    /// step said public, or the same engine under `does_not_train`, runs.
    #[test]
    fn a_private_step_never_goes_where_the_pact_is_not_a_no() {
        use models::pact::DataPact;
        let dir = scratch("patto");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let run = |pact: DataPact, input: serde_json::Value| {
            let action = ExternalEngineAction::resolving_with(Pacted { bin: bin.clone(), pact });
            with_price_list(None, || action.execute(&input, &mut shared("corsa", "passo")))
        };
        let private = json!({"tool": "motore-di-prova", "data": "private", "stdin": "ciao", "timeout_secs": 10});
        let public = json!({"tool": "motore-di-prova", "data": "public", "stdin": "ciao", "timeout_secs": 10});
        let unsaid = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let refused = run(DataPact::Trains, private.clone()).expect_err("a training engine is refused");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("data pact is «trains»"), "{}", refused.said);
        let unknown = run(DataPact::Unknown, private.clone()).expect_err("unknown is not a no");
        assert!(unknown.said.contains("data pact is «unknown»"), "{}", unknown.said);

        run(DataPact::DoesNotTrain, private).expect("a pact that does not train may read it");
        run(DataPact::Trains, public).expect("a public step goes anywhere");
        run(DataPact::Unknown, unsaid).expect("a step that says nothing is public");
    }

    /// Two engines that both answer, told apart by the id on the ledger row.
    struct TwoEngines {
        bins: std::collections::BTreeMap<&'static str, String>,
    }

    impl ToolResolver for TwoEngines {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
    }

    /// An engine that answers on stdout and states its counts on stderr, the
    /// way a local model runner does: the descriptor says which pipe, and the
    /// row carries the counts; read from stdout instead, they stay unknown.
    #[test]
    fn counts_stated_on_stderr_are_read_when_the_descriptor_says_so() {
        let dir = scratch("stderr-counts");
        let bin = fake_engine(
            &dir,
            "locale",
            "cat > /dev/null\necho \"the answer\"\necho \"prompt eval count:    26 token(s)\" >&2\necho \"eval count:           298 token(s)\" >&2",
        );
        let recipe = |from: models::usage::Heard| AskRecipe {
            usage: Some(UsageRecipe {
                args: vec!["--verbose".to_owned()],
                declared: Declared {
                    read: Shape::Text,
                    from,
                    input_tokens: Some(Pointer::Pattern(r"prompt eval count:\s*(\d+)".to_owned())),
                    output_tokens: Some(Pointer::Pattern(r"(?m)^eval count:\s*(\d+)".to_owned())),
                    ..Declared::default()
                },
            }),
            ..declaring_recipe()
        };
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        let run = |from, ledger: &str| {
            let action = ExternalEngineAction::resolving_with(Declares { bin: bin.clone(), recipe: Some(recipe(from)) })
                .recording_to(Some(Ledger::open(dir.join(ledger)).expect("open")));
            with_price_list(None, || action.execute(&input, &mut shared("corsa", "passo"))).expect("answers");
            calls_in(&dir.join(ledger)).remove(0)
        };

        let heard = run(models::usage::Heard::Stderr, "deposito");
        assert_eq!((heard.input_tokens, heard.output_tokens), (Some(26), Some(298)));
        // The control: the same engine read on stdout states nothing.
        let unheard = run(models::usage::Heard::Stdout, "deposito-2");
        assert_eq!((unheard.input_tokens, unheard.output_tokens), (None, None));
    }

    /// Two engines with a subscription window each, read as fuel.
    struct Fuelled {
        bins: std::collections::BTreeMap<&'static str, String>,
        fuels: std::collections::BTreeMap<&'static str, models::fuel::Fuel>,
    }

    impl ToolResolver for Fuelled {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn fuel(&self, id: &str) -> Vec<models::fuel::Fuel> {
            self.fuels.get(id).cloned().into_iter().collect()
        }
    }

    /// Under `prefer: fuel` the engine whose window expires unused soonest
    /// goes first even when the chain wrote it second; without it the chain
    /// stays as written.
    #[test]
    fn a_window_that_would_expire_unused_is_spent_first() {
        let dir = scratch("carburante");
        let long = fake_engine(&dir, "a-lungo", WRAPS_ON_DEMAND);
        let short = fake_engine(&dir, "a-breve", WRAPS_ON_DEMAND);
        let fuel = |engine: &str, left: f64, resets_in: i64| models::fuel::Fuel {
            engine: engine.to_owned(),
            unit: "five_hour".to_owned(),
            left_fraction: left,
            resets_in_secs: Some(resets_in),
        };
        let engines = || Fuelled {
            bins: [("a-lungo", long.clone()), ("a-breve", short.clone())].into_iter().collect(),
            fuels: [
                ("a-lungo", fuel("a-lungo", 0.80, 6 * 86_400)),
                ("a-breve", fuel("a-breve", 0.10, 3_600)),
            ]
            .into_iter()
            .collect(),
        };
        let by_fuel = json!({"tool": ["a-lungo", "a-breve"], "prefer": "fuel", "stdin": "ciao", "timeout_secs": 10});
        let as_written = json!({"tool": ["a-lungo", "a-breve"], "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")));
        with_price_list(None, || action.execute(&by_fuel, &mut shared("corsa-1", "passo")))
            .expect("the short window answers");
        assert_eq!(calls_in(&dir.join("deposito"))[0].cli, "a-breve");

        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")));
        with_price_list(None, || plain.execute(&as_written, &mut shared("corsa-2", "passo")))
            .expect("the chain's first answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "a-lungo");

        // A word `prefer` does not know is refused by name, not read as silence.
        let by_luck = json!({"tool": ["a-lungo"], "prefer": "luck", "stdin": "ciao", "timeout_secs": 10});
        let refused = with_price_list(None, || plain.execute(&by_luck, &mut shared("corsa-3", "passo")))
            .expect_err("an unknown preference is refused");
        assert_eq!(refused.class, "invalid_input");
        assert!(refused.said.contains("«luck»"), "{}", refused.said);
    }

    /// A step that would start under a profile whose endpoint cannot be
    /// reached is refused before spending, with the profile's reason.
    #[test]
    fn a_profile_whose_endpoint_is_refused_holds_the_engine_back_before_spending() {
        let dir = scratch("endpoint-rifiutato");
        let bin = fake_engine(&dir, "codex", WRAPS_ON_DEMAND);
        let store = dir.join("profili.json");
        std::fs::write(
            &store,
            format!(
                r#"{{"profiles": [{{"name": "altrove", "cli_id": "codex", "home_dir": "{}",
                    "endpoint": {{"url": "http://localhost:1/v1", "key_var": "NO_SUCH_KEY_VAR_HERE",
                    "protocol": "anthropic-messages"}}}}],
                  "active": {{"codex": "altrove"}}}}"#,
                dir.join("casa").display()
            ),
        )
        .expect("write the store");
        let action = ExternalEngineAction::resolving_with(Declares { bin, recipe: Some(declaring_recipe()) });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let refused = with_profiles_state(&store, || action.execute(&input, &mut shared("corsa", "passo")));

        let refused = refused.expect_err("a profile that cannot be pointed there refuses the launch");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("anthropic-messages"), "{}", refused.said);
    }

    /// A step that declares its kind goes first to the engines the strengths
    /// table names for that kind, then to the chain as written; without a row
    /// for the kind the chain's first answers. The ledger row names the kind.
    #[test]
    fn a_kind_of_work_goes_first_where_the_table_says_and_the_ledger_names_it() {
        let dir = scratch("forze");
        let local = fake_engine(&dir, "locale", WRAPS_ON_DEMAND);
        let chained = fake_engine(&dir, "catena", WRAPS_ON_DEMAND);
        let engines = || TwoEngines {
            bins: [("locale", local.clone()), ("catena", chained.clone())].into_iter().collect(),
        };
        let table = dir.join("strengths.json");
        std::fs::write(&table, r#"{"measured_on": "a test", "rows": {"mechanical": ["locale"]}}"#)
            .expect("write the table");
        let empty = dir.join("strengths-empty.json");
        std::fs::write(&empty, r#"{"measured_on": "a test", "rows": {}}"#).expect("write the empty table");
        let input = json!({"tool": "catena", "kind": "mechanical", "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")))
            .strong_by(Some(table));
        with_price_list(None, || action.execute(&input, &mut shared("corsa-1", "passo")))
            .expect("the local engine answers");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].cli, "locale", "the table's engine went first, ahead of the chain");
        assert_eq!(calls[0].work_kind.as_deref(), Some("mechanical"));

        // The control: without a row for the kind, the chain as written.
        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")))
            .strong_by(Some(empty));
        with_price_list(None, || plain.execute(&input, &mut shared("corsa-2", "passo")))
            .expect("the chain's engine answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "catena");
    }

    /// **E IL DEPOSITO DEVE DIRLO ANCHE QUANDO L'USCITA È ZERO.**
    ///
    /// La metà che non si vede dal comportamento del passo. Le due
    /// prove gemelle in `tests` guardano se il ripiego scatta; questa guarda la
    /// riga che resta scritta, ed è quella che qualcuno leggerà domani. Fino al
    /// 01/09/2026 nasceva con `error_type: None`, cioè **indistinguibile da una
    /// chiamata riuscita**: una somma che le mescola dice che quel motore ha
    /// risposto, e chi la legge non va a cercare niente.
    ///
    /// Senza questa prova un mutante che lascia scattare il ripiego ma scrive
    /// `None` invece di `exhausted` passerebbe sotto alle altre due.
    #[test]
    fn a_zero_exit_refusal_is_recorded_as_exhausted_not_as_a_clean_call() {
        let dir = scratch("esaurito-a-zero-nel-deposito");
        let bin = fake_engine(
            &dir,
            "motore-esaurito-a-zero",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 0",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-esaurita-a-zero", "passo-1"))
        })
        .expect_err("un motore che dice di non poter lavorare non ha risposto");
        assert_eq!(error.class, "engine_exhausted");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata è stata fatta, e va registrata");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "un motore esaurito che esce zero non è una chiamata pulita: la riga \
             che lo dice è l'unica traccia che resta"
        );
    }

    /// **E LA SPECIE NON DIPENDE DA `accept`, IN NESSUNO DEI DUE RAMI.**
    ///
    /// La tolleranza di un passo riguarda **cosa fa la corsa** — se il
    /// fallimento è un dato che il passo si tiene, o una ragione per fermarsi —
    /// e non deve toccare **cosa resta scritto**. Nel ramo `ExitError` questo
    /// vale da sempre, perché lì il `note(...)` sta prima del controllo di
    /// tolleranza; nel primo tentativo di chiudere questo guasto, nel ramo `Ok`
    /// stava **dopo**, e con `accept: ["exit_error"]` dichiarato la riga
    /// nasceva di nuovo `NULL` — cioè indistinguibile da una risposta vera.
    /// Il difetto sopravviveva in un angolo del proprio rimedio, e l'ha trovato
    /// un giudice che non aveva scritto il lavoro.
    ///
    /// Le due metà stanno insieme apposta: sono la stessa affermazione — «la
    /// specie è la stessa e non dipende dalla tolleranza» — su tutti e due i
    /// codici d'uscita, ed è quella che il commento accanto al codice dichiara.
    #[test]
    fn a_tolerated_refusal_is_recorded_as_exhausted_whatever_the_exit_code() {
        for (name, exit, script) in [
            (
                "zero",
                0,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
            ),
            (
                "uno",
                1,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 1",
            ),
        ] {
            let dir = scratch(&format!("esaurito-tollerato-{name}"));
            let bin = fake_engine(&dir, "motore-esaurito", script);
            let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
            let mut recipe = declaring_recipe();
            recipe.unusable_when = vec!["weekly limit".to_owned()];
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(recipe),
            })
            .recording_to(Some(ledger));
            // Il passo dichiara di volersi tenere il fallimento di questo
            // motore: la corsa non si ferma, e infatti il passo va avanti.
            let input = json!({
                "tool": "motore-di-prova",
                "stdin": "ciao",
                "timeout_secs": 10,
                "accept": ["exit_error"]
            });

            let outcome = with_price_list(None, || {
                action.execute(&input, &mut shared(&format!("corsa-{name}"), "passo-1"))
            })
            .unwrap_or_else(|error| {
                panic!(
                    "il passo tollera il fallimento, non deve rompersi: {}",
                    error.said
                )
            });
            assert!(
                matches!(outcome, ActionOutcome::Went(_)),
                "la tolleranza resta quella di prima: il passo prosegue (uscita {exit})"
            );

            let calls = calls_in(&dir.join("deposito"));
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].error_type.as_deref(),
                Some("exhausted"),
                "uscita {exit}: il passo ha tollerato il fallimento, ma la riga del \
                 deposito deve dire lo stesso che quel motore non poteva lavorare. \
                 La tolleranza decide cosa fa la corsa, non cosa resta scritto"
            );
        }
    }

    /// Un motore che non parte lascia comunque traccia, con la causa sua: senza
    /// questa riga una catena che ripiega sembrerebbe aver scelto il secondo
    /// motore per primo.
    #[test]
    fn an_engine_that_never_starts_leaves_its_own_row_too() {
        let dir = scratch("mai-partito");
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: "/nessun/binario/qui-di-sicuro".to_owned(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-4", "passo-4"))
        })
        .expect_err("un binario che non c'è rompe il passo");
        assert_eq!(error.class, "engine_spawn_failed");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].error_type.as_deref(), Some("spawn_failed"));
    }

    // ── l'uscita del passo non cambia perché si misura ─────────────────

    /// **IL VINCOLO CHE NESSUNO HA CHIESTO E CHE ROMPEREBBE UN FLUSSO VERO.**
    /// `flows/come-lo-risolvono-gli-altri.flow.json` dichiara `allow_extra: false`
    /// sulla forma della risposta di un passo motore. Se chiedere l'involucro
    /// lasciasse l'involucro dentro `stdout`, quel flusso diventerebbe rosso per
    /// una misura che non ha chiesto. Qui si guarda che il testo che esce sia
    /// **identico** con e senza la misura accesa.
    #[test]
    fn asking_for_a_json_envelope_does_not_change_what_the_step_receives() {
        let dir = scratch("involucro");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let without = {
            let ledger = Ledger::open(dir.join("senza")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin: bin.clone(),
                recipe: Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: Vec::new(),
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    usage: None,
                }),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &mut shared("corsa-5", "passo-5"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        let with = {
            let ledger = Ledger::open(dir.join("con")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(declaring_recipe()),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &mut shared("corsa-6", "passo-6"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        assert_eq!(
            without, with,
            "misurare non deve cambiare di una virgola ciò che il passo consegna a valle"
        );
        // E la misura c'è stata davvero: senza questo, la prova passerebbe anche
        // se il blocco `usage` non fosse mai arrivato al punto di invocazione.
        assert_eq!(
            calls_in(&dir.join("con"))[0].input_tokens,
            Some(1_000_000),
            "l'involucro è stato chiesto e letto"
        );
        assert_eq!(calls_in(&dir.join("senza"))[0].input_tokens, None);
    }

    // ── senza il posto dove scrivere, non si scrive niente ─────────────

    /// Una riga attribuita a nessuno sporcherebbe le somme peggio di una riga
    /// mancante: senza deposito, o senza corsa, non si registra.
    #[test]
    fn without_a_ledger_or_without_a_run_nothing_is_written() {
        let dir = scratch("senza-appigli");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let recipe = declaring_recipe();

        // Senza deposito: il passo funziona lo stesso.
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        assert!(action
            .execute(&input, &mut shared("corsa-7", "passo-7"))
            .is_ok());

        // Col deposito ma senza la chiave della corsa: nessuna riga.
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let mut only_the_step = SharedState::new();
        only_the_step.insert(flow::CURRENT_STEP.to_owned(), json!("passo-8"));
        assert!(action.execute(&input, &mut only_the_step).is_ok());
        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "senza corsa non si attribuisce nessuna spesa a nessuno"
        );
    }

    /// Un `bin` scritto a mano nel passo non è una chiamata a un modello:
    /// `sh -c echo` non consuma nessuna quota, e riempirne il deposito
    /// renderebbe illeggibile la vista che questo lavoro esiste per rendere
    /// leggibile.
    #[test]
    fn a_hand_written_bin_is_not_a_model_call() {
        let dir = scratch("bin-a-mano");
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::new().recording_to(Some(ledger));
        let input = json!({"bin": "echo", "args": ["ciao"], "timeout_secs": 10});

        assert!(action
            .execute(&input, &mut shared("corsa-9", "passo-9"))
            .is_ok());
        assert!(calls_in(&dir.join("deposito")).is_empty());
    }

    /// **`cargo` E `git` NON SONO CHIAMATE A UN MODELLO, E IL DEPOSITO NON DEVE
    /// CONTARLE.**
    ///
    /// Misurato sul deposito di questa macchina il 31/08/2026: su ventiquattro
    /// righe di `model_calls`, due sono `git` e una `cargo`. Nessuna delle tre
    /// consuma quota di nessun abbonamento, e tutte e tre arrivano senza costo —
    /// quindi `Spend::is_complete()` è **falso su ogni corsa vera**, e la frase
    /// d'onestà del tetto («la spesa vera è più alta») si accende sempre, anche
    /// quando non c'è niente di ignoto. Un avviso sempre acceso non lo legge
    /// nessuno, ed è così che si perde quello vero — la riga di codex, che il
    /// costo davvero non lo dichiara.
    ///
    /// **CHI DECIDE È IL DESCRITTORE, NON UN ELENCO DI NOMI SCRITTO QUI.** Uno
    /// strumento è un motore se dichiara **come gli si fa una domanda**
    /// (`ask`): `git` e `cargo` non lo dichiarano, e nessun elenco di nomi qui
    /// dentro invecchierebbe bene. È la stessa regola del guasto 3 — quello che
    /// il catalogo dichiara vale più di quello che il codice indovina.
    #[test]
    fn a_tool_that_cannot_be_asked_anything_is_not_a_model_call() {
        let dir = scratch("non-e-un-motore");
        let bin = fake_engine(&dir, "finto-cargo", "printf 'ok'");
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Nessuna ricetta `ask`: è com'è dichiarato `cargo` nel catalogo
            // spedito, e il passo si scrive le proprie opzioni.
            recipe: None,
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["test"], "timeout_secs": 10
        });

        assert!(action
            .execute(&input, &mut shared("corsa-cargo", "prove"))
            .is_ok());

        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "una riga di `cargo` nel conto delle chiamate ai modelli rende falso \
             ogni totale che la somma: {:?}",
            calls_in(&dir.join("deposito"))
        );
    }

    /// Le opzioni scritte dal passo vincono, e con loro il consumo resta
    /// sconosciuto: allungare alle spalle di chi ha scritto quella riga di
    /// comando una domanda che non ha fatto sarebbe decidere al posto suo. La
    /// riga però si scrive, e dice proprio questo.
    ///
    /// **È LA GEMELLA DELLA PROVA QUI SOPRA**, e le due vanno lette insieme: un
    /// motore vero interrogato con le opzioni del passo **resta** nel conto, e
    /// solo chi non è un motore ne esce. Senza questa, il filtro potrebbe
    /// svuotare la tabella e la prova sopra sarebbe verde lo stesso.
    #[test]
    fn when_the_step_writes_its_own_args_the_usage_is_not_asked_for() {
        let dir = scratch("args-del-passo");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["--a-modo-mio"],
            "stdin": "ciao", "timeout_secs": 10
        });

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-10", "passo-10"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        assert_eq!(output["stdout"], "la risposta vera");
        // **IL BRACCIO CHE CONTA**: la riga di comando è ESATTAMENTE quella che
        // il passo ha scritto. Accodarci le opzioni del consumo sarebbe
        // allungare alle spalle di chi l'ha scritta una domanda che non ha
        // fatto, e da fuori non si vedrebbe: solo guardando l'argv del processo
        // la differenza salta fuori.
        assert_eq!(
            argv_of(&dir),
            vec!["--a-modo-mio".to_owned()],
            "nessuna opzione aggiunta di nascosto"
        );
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata si registra lo stesso");
        assert_eq!(calls[0].input_tokens, None, "ma non misurata");
    }

    /// **IL VINCOLO DI INDIPENDENZA DAL MODELLO, NEL PUNTO IN CUI SI ROMPE.**
    /// Un motore che non dichiara `usage` resta non misurato ANCHE SE la sua
    /// uscita è un involucro JSON con dentro chiavi che qualcuno riconoscerebbe.
    /// Se il codice avesse un ramo cablato su un fornitore — «se somiglia a
    /// questo, leggi qui» — questa prova diventerebbe rossa, e deve.
    #[test]
    fn output_that_merely_looks_familiar_is_not_read_without_a_declaration() {
        let dir = scratch("nessun-ramo-cablato");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", ALWAYS_WRAPS);
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-11", "passo-11"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].input_tokens, None,
            "quei numeri ci sono, ma nessun descrittore ha detto di leggerli"
        );
        assert_eq!(calls[0].cost_micros, None);
        assert_eq!(calls[0].actual_model, "");
        // E l'uscita del passo è quella grezza: senza `answer` dichiarato non
        // si spacchetta niente, perché nessuno ha detto dove guardare.
        assert!(
            output["stdout"].as_str().unwrap().starts_with('{'),
            "l'involucro resta tale e quale: {}",
            output["stdout"]
        );
    }

    // ── (f) sotto quale dotazione la chiamata è girata ─────────────────

    /// La stessa serratura di `with_price_list`, per lo stato dei profili:
    /// `PROFILES_STATE_PATH` è una variabile sola come `SAILOR_PRICING`, e due
    /// prove che la scrivessero insieme si toglierebbero la dotazione a vicenda.
    /// Il listino si punta al vuoto apposta — qui si guarda il profilo, non il
    /// costo, e dipendere dal file di casa di chi esegue le prove sarebbe un
    /// modo di venire diversi senza che niente sia cambiato.
    fn with_profiles_state<T>(state: &std::path::Path, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var("PROFILES_STATE_PATH", state);
        std::env::set_var(PRICING_ENV, "/nessun/listino/qui");
        let out = body();
        std::env::remove_var("PROFILES_STATE_PATH");
        std::env::remove_var(PRICING_ENV);
        out
    }

    /// **LA DOTAZIONE SOTTO CUI LA CHIAMATA È GIRATA FINISCE NELLA SUA RIGA.**
    ///
    /// Guasto 18, seconda metà. Senza, due corse dello stesso flusso non sono la
    /// stessa misura: la stessa catena di passi, sotto due profili, dà due
    /// consumi diversi per una ragione che la riga non porta. Fino al
    /// 01/09/2026 questa colonna la scriveva vuota ogni chiamata.
    ///
    /// **E IL PERCORSO DELLA CASA CI STA DENTRO**, che è il dato su cui una
    /// diagnostica si appoggia: un nome di profilo si riusa, si sposta e si
    /// cancella, un percorso è il posto dove si va a guardare.
    ///
    /// *Mutante eseguito*: rimettere `engine_identity: EngineIdentity::default()`
    /// in `record_the_call`. Questa diventa rossa e la gemella qui sotto resta
    /// verde — ed è per questo che ci sono tutte e due.
    #[test]
    fn the_row_says_under_which_equipment_the_call_ran() {
        let dir = scratch("dotazione");
        // Il nome del file È il legame: `cli_for_executable` riconosce la riga
        // di comando dall'eseguibile, non dall'identificativo del descrittore.
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: dir.join("casa"),
                endpoint: None,
            },
            "la riga non dice con quale identità la chiamata è girata"
        );
    }

    /// La gemella: senza nessun profilo attivo la riga dice **ereditata**, non un
    /// nome inventato e nemmeno un vuoto. Senza di lei un mutante che scrivesse
    /// sempre la stessa identità passerebbe la prova qui sopra.
    ///
    /// **«EREDITATA» È IL PUNTO DELLA CURA.** Prima qui c'era la stringa vuota,
    /// la stessa che usciva quando il binario non era un motore conosciuto,
    /// quando il profilo era sparito, e quando la casa non si sposta con una
    /// variabile. Quattro fatti diversi e un vuoto solo: adesso questo dice che
    /// il processo è partito con la casa di chi ha aperto il terminale, e quale
    /// riga di comando era.
    #[test]
    fn with_no_profile_in_force_the_row_says_the_identity_was_inherited() {
        let dir = scratch("nessuna-dotazione");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(&state, r#"{"profiles":[],"active":{}}"#)
            .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "codex".to_owned()
            }
        );
    }

    /// Un finto `codex` che dice **con quale casa è partito davvero**: la scrive
    /// su un file accanto a sé, e poi risponde nell'involucro come gli altri.
    /// Senza questo file la prova qui sotto guarderebbe solo il deposito, cioè
    /// solo metà del difetto.
    const WRITES_DOWN_ITS_HOME: &str = r#"cat > /dev/null
printf '%s' "$CODEX_HOME" > "$(dirname "$0")/casa"
printf '{"result":"la risposta vera","model":"modello-di-prova","usage":{"input_tokens":1,"output_tokens":1}}'"#;

    /// **IL DEPOSITO REGISTRA UN'IDENTITÀ CHE IL PROCESSO NON HA USATO.**
    ///
    /// Che il passo vinca è la decisione, non il difetto. Il difetto è che la
    /// riga continua a nominare il profilo attivo: il motore è partito nella
    /// casa scritta nel passo, e chi legge il deposito per sapere con quali
    /// credenziali quel processo ha girato legge il nome di un profilo che non è
    /// mai stato messo in forza. È il caso in cui qualcuno ha cambiato identità
    /// apposta — cioè esattamente quello che una diagnostica o un controllo di
    /// sicurezza esiste per vedere — ed è il caso in cui il dato mente.
    ///
    /// **LE DUE METÀ SI GUARDANO INSIEME.** Cosa ha ricevuto il processo, e cosa
    /// dice la riga. Separate, ognuna delle due resta verde col difetto dentro.
    #[test]
    fn the_row_does_not_name_a_profile_the_step_replaced() {
        let dir = scratch("dotazione-scavalcata");
        let bin = fake_engine(&dir, "codex", WRITES_DOWN_ITS_HOME);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa-del-profilo")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"CODEX_HOME": "/una/casa/scritta/nel/passo"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let home_it_started_in =
            std::fs::read_to_string(dir.join("casa")).expect("il motore ha scritto la sua casa");
        assert_eq!(
            home_it_started_in, "/una/casa/scritta/nel/passo",
            "il verso della sovrapposizione è cambiato: il profilo ha scavalcato il passo"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ChosenByTheStep {
                cli_id: "codex".to_owned(),
                home_dir: PathBuf::from("/una/casa/scritta/nel/passo"),
            },
            "la riga nomina un'identità che il processo non ha usato: è partito in {home_it_started_in}"
        );
    }

    /// **IL GETTONE NON ENTRA IN NESSUN CAMPO DELL'IDENTITÀ.**
    ///
    /// Un passo può portare nel proprio ambiente qualunque variabile, chiavi
    /// comprese. Ciò che finisce nel deposito è **quale casa** e **come è stata
    /// scelta**, mai cosa c'era intorno: una riga di registro si legge in una
    /// diagnostica, si copia in un rapporto e si manda a qualcuno.
    ///
    /// *Mutante eseguito*: vedi la consegna — far portare all'identità l'intero
    /// ambiente rende rossa questa e nessun'altra.
    #[test]
    fn no_secret_from_the_step_ends_up_in_the_recorded_identity() {
        let dir = scratch("nessun-gettone");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        // Un gettone riconoscibile: se comparisse da qualche parte, si vede.
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"OPENAI_API_KEY": "sk-questo-non-deve-comparire"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        let written = calls[0].engine_identity.to_column();
        assert!(
            !written.contains("sk-questo-non-deve-comparire"),
            "un gettone del passo è finito nell'identità registrata: {written}"
        );
        assert!(
            !calls[0].engine_identity.to_string().contains("sk-"),
            "un gettone del passo è finito in ciò che si stampa a una persona"
        );
    }
}
