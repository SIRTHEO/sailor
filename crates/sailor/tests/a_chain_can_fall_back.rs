//! Una catena di motori serve solo se il ripiego può scattare.
//!
//! **IL GUASTO 31.** Un passo che scrive `"tool": ["claude-code", "agy",
//! "codex"]` sembra avere due ripieghi. Ne ha quanti sono i motori che
//! **dichiarano come dicono di non poter lavorare**: `says_it_cannot_work` su
//! un elenco vuoto è `false`, quindi un motore che tace fa morire il passo sul
//! proprio fallimento e i motori dopo di lui non partono mai. `agy` taceva, e
//! stava **in mezzo** a dodici catene di questo albero: `codex`, che dichiara
//! il proprio 401, non partiva mai da nessuna di esse.
//!
//! **CHIUSO IL 01/09/2026 SPOSTANDO `agy`, E RICHIUSO LO STESSO GIORNO
//! MISURANDOLO.** La regola ha due modi di essere rispettata — misurare le
//! parole di chi sta in mezzo, oppure non mettere in mezzo chi non le ha. Il
//! secondo non chiede nessun dato che non esista, ed era la strada giusta finché
//! il dato non c'era; ma è una scorciatoia se il dato si può misurare, e si
//! poteva. Puntando `HOME` a una cartella vuota, `agy` dice con parole sue di
//! non poter lavorare, e adesso quelle parole stanno nel suo descrittore:
//! `what_agy_declares_matches_what_agy_really_said` le confronta con l'uscita
//! vera. La regola la scrive `toolbox::Descriptor::cannot_be_a_fallback`, in un
//! posto solo, e la legge anche `sailor flow check` sui flussi di chi lo lancia.
//!
//! **RESTA NON MISURATO** come `agy` dice di aver finito la **quota**: cercato
//! nell'aiuto, nei sottocomandi annidati e nella tabella delle stringhe del
//! binario, e non trovato. È scritto nel descrittore, dove chi legge una parola
//! deve poter sapere quale metà del campo copre.
//!
//! Il meccanismo è provato in modo ermetico in `crates/actions`
//! (`an_engine_that_declares_no_exhaustion_words_kills_the_chain`, in coppia
//! con `an_engine_that_says_it_is_out_hands_the_work_to_the_next_one`): elenco
//! popolato, il secondo parte; elenco vuoto, il secondo non parte. Qui si
//! guarda se quel difetto è **in casa nostra**, sui flussi e sui descrittori
//! spediti.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// Un motore nominato in una catena, con il posto che occupa.
struct InChain {
    flow: String,
    step: String,
    tool: String,
    last: bool,
}

/// Tutti i motori nominati da una catena in tutti i flussi dell'albero.
///
/// **L'ORDINE CONTA E SI LEGGE DAL FILE.** Un `BTreeSet` direbbe *quali*
/// motori, mai *quale viene dopo quale* — e la domanda qui è esattamente
/// quella: l'ultimo della catena non ha nessuno a cui passare il lavoro, e
/// pretendere da lui una dichiarazione di esaurimento sarebbe pretendere una
/// misura che non serve a niente.
fn engines_in_chains() -> Vec<InChain> {
    // Read from the flows compiled into the binary, not from a directory of the
    // repository. What the product hands out is what has to hold this rule;
    // a check that reads our own workshop is green here and false everywhere.
    let mut found = Vec::new();
    for (name, text) in flow::system::FLOWS {
        let Ok(file) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        let flow = (*name).to_owned();
        let Some(steps) = file["graph"]["steps"].as_array() else {
            continue;
        };
        for step in steps {
            let Some(chain) = step["with"]["tool"].as_array() else {
                continue;
            };
            let names: Vec<&str> = chain.iter().filter_map(|id| id.as_str()).collect();
            for (place, tool) in names.iter().enumerate() {
                found.push(InChain {
                    flow: flow.clone(),
                    step: step["id"].as_str().unwrap_or_default().to_owned(),
                    tool: (*tool).to_owned(),
                    last: place + 1 == names.len(),
                });
            }
        }
    }
    found
}

/// Perché un motore spedito non può fare da ripiego, se non può.
///
/// **LA REGOLA NON È SCRITTA QUI**, ed è la differenza che conta: sta in
/// `toolbox::Descriptor::cannot_be_a_fallback`, in un posto solo, e da lì la
/// legge anche `sailor flow check` sui flussi e sui descrittori di chi lancia.
/// Una copia scritta dentro questa prova avrebbe sorvegliato i quattro flussi di
/// questo albero e nessuno di quelli di nessun altro.
fn why_it_cannot_be_a_fallback(tool: &str) -> Option<String> {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == tool)
        .and_then(|loaded| loaded.descriptor.cannot_be_a_fallback())
}

/// **LA REGOLA, E DA OGGI GIRA.**
///
/// Ogni motore che compare in una catena e **non è l'ultimo** deve dichiarare
/// almeno una parola con cui dice di non poter lavorare. Chi non la dichiara
/// non è un ripiego: è un tappo.
///
/// **PERCHÉ ERA `#[ignore]`, E PERCHÉ NON LO È PIÙ.** La ragione scritta qui
/// fino al 01/09/2026 era buona a metà: le parole con cui `agy` dice di non
/// poter lavorare non erano mai state viste su questa macchina, e inventarle
/// sarebbe stato peggio del difetto. Ma «non inventarle» non vuol dire «non
/// cercarle», e per giorni ha significato quello — quella era la ragione per non
/// **inventare un dato**, ed era diventata la ragione per non **avere un
/// controllo** e poi per non **fare la misura**.
///
/// **ADESSO LA MISURA C'È.** Con `HOME` su una cartella vuota, `agy` dice di non
/// poter lavorare con parole sue, e quelle parole stanno nel suo descrittore. La
/// regola perciò non ha più nessuna eccezione: nessun motore spedito è esentato,
/// e dove ciascuno sta in catena si decide su un'altra misura — quanto costa,
/// quanto è autenticato, quanto ha risposto — non su chi tace.
///
/// **QUELLO CHE RESTA APERTO, E VA DETTO QUI PERCHÉ QUI SI LEGGE.** Ciò che è
/// misurato di `agy` sono le credenziali mancanti, non la quota finita: un `agy`
/// che avesse esaurito la quota passerebbe ancora per un fallimento qualunque.
/// La riga sta nel descrittore, e la regola non può vederla — perché la regola
/// chiede che il campo non sia vuoto, non che sia completo.
#[test]
fn every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted() {
    let mut silent: BTreeSet<String> = BTreeSet::new();
    for engine in engines_in_chains() {
        if engine.last {
            continue;
        }
        if let Some(why) = why_it_cannot_be_a_fallback(&engine.tool) {
            silent.insert(format!(
                "{} · {} · {}: {why}",
                engine.flow, engine.step, engine.tool
            ));
        }
    }
    assert!(
        silent.is_empty(),
        "questi motori stanno in mezzo a una catena senza dichiarare come dicono \
         di non poter lavorare: quando si esauriscono uccidono il passo, e i \
         motori dopo di loro non partono. È il guasto 31.\n{}",
        silent.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// The rule is not kept by emptying the chains. If `cannot_be_a_fallback`
/// began answering "no" to everyone, the test above would stay green over
/// chains that no longer fall back. The canary used to perch on the flows of
/// this tree; those left the repository, so it asks the shipped descriptors.
#[test]
fn an_engine_that_declares_its_words_is_still_allowed_in_the_middle() {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    let allowed: BTreeSet<String> = catalog
        .live()
        .into_iter()
        .filter(|loaded| loaded.descriptor.cannot_be_a_fallback().is_none())
        .map(|loaded| loaded.descriptor.id.clone())
        .collect();
    assert!(
        !allowed.is_empty(),
        "nessuno strumento spedito può stare in mezzo a una catena: la regola \
         dice «no» a tutti, e la prova qui sopra è verde perché non ha niente \
         da guardare"
    );
}

/// **LA REGOLA QUI SOPRA NON DEVE POTER DIVENTARE VUOTA IN SILENZIO.**
///
/// Questa prova dice una cosa sola: nei flussi di questo albero esistono catene
/// con un motore che non è l'ultimo. Il giorno in cui non ce ne fossero più —
/// perché i flussi cambiano — la regola qui sopra passerebbe senza guardare
/// niente, e nessuno lo saprebbe. È il difetto del guasto 22 applicato a un
/// controllo invece che a un totale: uno zero mai calcolato si presenta come una
/// misura.
///
/// **E NON È IPOTETICA: È SUCCESSA.** Il 01/09/2026 il guasto 31 è stato chiuso
/// spostando `agy` in fondo a tutte e dodici le posizioni. Da quel momento
/// nessun motore muto stava più in mezzo, la regola non guardava più niente su
/// `agy`, e sarebbe rimasta verde qualunque cosa il suo descrittore dicesse —
/// misurato: con `agy` in fondo e `unusable_when` svuotato, la regola resta
/// verde. Questa prova non l'ha vista perché `claude-code` e `codex` erano
/// ancora in mezzo: sorveglia che le catene esistano, non che sorveglino tutti.
#[test]
fn there_are_chains_whose_fallback_can_actually_be_needed() {
    let engines = engines_in_chains();
    let not_last = engines.iter().filter(|engine| !engine.last).count();
    assert!(
        not_last > 0,
        "nessuna catena ha un motore prima dell'ultimo: la regola sul ripiego \
         non guarderebbe più niente, e resterebbe verde per vuoto"
    );
}

/// **LE PAROLE SCRITTE NEL DESCRITTORE SONO QUELLE CHE IL MOTORE HA DETTO.**
///
/// Questa è la prova che il 31/08 non si poteva scrivere, ed è il motivo per cui
/// l'eccezione che stava qui non c'è più. Fino al 01/09/2026 questo posto
/// ospitava una sorveglianza sull'eccezione — «`agy` deve restare in fondo, e
/// non deve dichiarare niente» — cioè un controllo che pretendeva che la misura
/// **non** fosse stata fatta. Un controllo così non protegge il prodotto:
/// protegge la scorciatoia, e diventa rosso il giorno in cui qualcuno lavora.
///
/// **LA MISURA, PER CHI VUOLE RIFARLA.** `HOME` puntato a una cartella vuota, e
/// la riga che Sailor monta davvero: `agy --mode plan --output-format json
/// --print "<domanda>"`. Senza credenziali non chiama nessun fornitore e non
/// costa niente. Esce **1**, e dice le parole che stanno qui sotto — due volte
/// su due, identiche.
///
/// **PERCHÉ IL TESTO STA QUI E IL MOTORE NO.** Una prova che avvia `agy` vero
/// dipende da come sta messa la casa di chi la esegue: verde su una macchina
/// autenticata, rossa su un'altra, e per la ragione sbagliata in tutti e due i
/// casi. Qui il testo è quello misurato, copiato una volta e poi fermo, e ciò
/// che si prova è l'anello che nessuno guardava: **le parole del descrittore
/// spedito combaciano con l'uscita vera**. Scriverne una sbagliata — un refuso,
/// una maiuscola di troppo in un confronto che non le ignorasse — passerebbe
/// sotto a ogni altra prova di questo albero.
///
/// **QUELLO CHE NON COPRE, E STA SCRITTO NEL DESCRITTORE.** Questa è la metà
/// «credenziali mancanti» di `unusable_when`, la stessa metà con cui `codex`
/// dichiara il proprio 401. Le parole con cui `agy` dice di aver finito la
/// **quota** restano non misurate: non stanno nell'aiuto, né in quello dei
/// sottocomandi annidati, e nella tabella delle stringhe del binario
/// l'esaurimento compare solo come motivo di ritentativo interno, mai come
/// messaggio stampato.
#[test]
fn what_agy_declares_matches_what_agy_really_said() {
    // L'uscita vera del 01/09/2026, presa come è arrivata. Lo stdout è il JSON
    // che `--output-format json` produce quando la casa non ha credenziali.
    let stdout = r#"{"conversation_id":"","status":"ERROR","response":"","error":"authentication failed or timed out","duration_seconds":0,"num_turns":0,"usage":{"input_tokens":0,"output_tokens":0,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":0}}"#;
    let stderr = "Authentication required. Please visit the URL to log in:\n  \
                  https://accounts.google.com/o/oauth2/auth?access_type=offline\n\n\
                  Waiting for authentication (timeout 60s)...\n\
                  Or, paste the authorization code here and press Enter:\n\
                  Error: authentication timed out.";

    // La ricetta non si riscrive qui: si chiede a chi la compone per davvero,
    // altrimenti la prova sorveglierebbe una copia e il descrittore spedito
    // potrebbe dire altro.
    let tools = toolbox::Tools::new(
        toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]),
        toolbox::Machine::current(),
    );
    let recipe = actions::ToolResolver::ask_recipe(&tools, "agy")
        .expect("«agy» è spedito e dichiara come lo si interroga");

    match actions::judge_dry_run(&recipe, stdout, stderr) {
        actions::ProbeVerdict::CannotWork { said } => {
            assert!(
                said.contains("authentication"),
                "il motivo deve portare le parole del motore: {said}"
            );
        }
        other => panic!(
            "«agy» senza credenziali deve risultare «non può lavorare adesso», \
             invece è {other:?}. Le parole scritte in `unusable_when` non \
             combaciano più con quello che agy dice davvero"
        ),
    }
}

/// **E UNA RISPOSTA QUALUNQUE NON DEVE COMBACIARE.**
///
/// Il pericolo di `unusable_when` non è che sia vuoto: è che sia **largo**. Una
/// parola generica manderebbe un mandato sano giù per tutta la catena finché
/// qualcuno risponde comunque, e nessuno saprebbe perché la risposta è di un
/// altro motore.
///
/// **E IL CASO CHE LO FA SUCCEDERE È BANALE**, per questo la risposta qui sotto
/// è fatta così: un passo che chiede a `agy` di *parlare* di autenticazione
/// riceve una risposta che contiene quella parola. Il confronto guarda l'uscita
/// intera, e l'uscita di un motore contiene la sua risposta: una parola sola
/// scritta in `unusable_when` farebbe scivolare la catena al motore dopo **su
/// una chiamata riuscita**, cioè pagandola due volte e attribuendola a chi non
/// l'ha data. È il difetto che il campo dichiara di voler evitare — «si
/// dichiarano le parole del fornitore, non una regola generale» — visto dal lato
/// in cui costa.
///
/// Senza questa prova, sostituire le tre frasi misurate con `"authentication"`
/// resterebbe verde.
#[test]
fn a_real_answer_from_agy_is_not_mistaken_for_an_exhausted_engine() {
    let stdout = r#"{"conversation_id":"c-1","status":"OK","response":"Il difetto sta nel middleware di authentication: il token scade e nessuno lo rinnova.","duration_seconds":2,"num_turns":1,"usage":{"input_tokens":12,"output_tokens":3,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":15}}"#;
    let tools = toolbox::Tools::new(
        toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]),
        toolbox::Machine::current(),
    );
    let recipe = actions::ToolResolver::ask_recipe(&tools, "agy")
        .expect("«agy» è spedito e dichiara come lo si interroga");

    assert!(
        !matches!(
            actions::judge_dry_run(&recipe, stdout, ""),
            actions::ProbeVerdict::CannotWork { .. }
        ),
        "una risposta buona di «agy» viene letta come un motore che non può \
         lavorare: le parole di `unusable_when` sono troppo larghe, e la catena \
         scivolerebbe al motore dopo su ogni chiamata riuscita"
    );
}
