//! The typed input of an engine step, as the flow wrote it.

use flow::ValueSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Cosa un passo chiede alla sessione del motore.
///
/// **UN PASSO NOMINA UN PASSO, NON UN IDENTIFICATIVO.** L'identificativo di una
/// sessione nasce mentre la corsa gira; chi scrive un flusso non lo può
/// conoscere, e un flusso che lo contenesse varrebbe per una corsa sola. Si
/// scrive quindi da chi si continua — `{"fork": "scopri"}` — e l'identificativo
/// lo va a cercare il deposito, che è il posto dove il passo `scopri` l'ha
/// posato.
///
/// **RIPRENDERE E RAMIFICARE NON SONO LA STESSA COSA, E CONFONDERLE COSTA.**
/// Chi riprende continua la sessione: due passi che riprendessero lo stesso
/// tronco si scriverebbero addosso a vicenda, e in un fronte parallelo
/// l'ordine con cui lo fanno non è deciso da nessuno. Chi ramifica parte dallo
/// stesso contesto e prosegue per conto suo: è il modo giusto per tre passi
/// indipendenti che guardano lo stesso albero, ed è il caso che rende di più —
/// la scoperta si paga una volta invece di tre.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionUse {
    /// `"session": "open"` — apre una sessione nuova e la registra, così i
    /// passi dopo possono continuarla.
    Open,
    /// `"session": {"resume": "scopri"}` — continua la sessione del passo
    /// nominato.
    Resume(String),
    /// `"session": {"fork": "scopri"}` — parte dal contesto del passo nominato
    /// senza toccarlo.
    Fork(String),
}

impl SessionUse {
    /// Il modo, detto a parole per chi guarda.
    pub(crate) fn word(&self) -> &'static str {
        match self {
            SessionUse::Open => "open a session",
            SessionUse::Resume(_) => "resume a session",
            SessionUse::Fork(_) => "fork a session",
        }
    }
}

/// Chi eseguire: un motore, o una catena di motori da provare in ordine.
///
/// **PERCHÉ UNA CATENA E NON UN RIPIEGO SOLO.** Un ripiego singolo copre il
/// caso di stanotte e non quello di domani: i motori esauriscono a scaglioni,
/// e chi ne ha tre installati vuole che il lavoro trovi il primo che può
/// farlo. La catena si legge nell'ordine in cui è scritta, e quell'ordine è
/// una scelta di chi ha scritto il flusso — il migliore per primo, non il più
/// economico.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolChoice {
    One(String),
    Chain(Vec<String>),
}

impl ToolChoice {
    pub(crate) fn ids(&self) -> &[String] {
        match self {
            ToolChoice::One(id) => std::slice::from_ref(id),
            ToolChoice::Chain(ids) => ids,
        }
    }
}

/// **PERCHÉ QUESTA STRUTTURA RACCOGLIE CIÒ CHE NON CONOSCE INVECE DI SCARTARLO.**
///
/// Il 30/08/2026 un flusso di prova scriveva `"prompt"` dove va `"stdin"`. Il
/// passo è partito lo stesso, il motore ha ricevuto una riga di comando monca,
/// e l'errore che è tornato era suo: «Input must be provided either through
/// stdin». Una chiamata a pagamento spesa per un refuso, e nessuno che potesse
/// dirlo prima. È il guasto 20.
///
/// **`deny_unknown_fields` NON È LA RISPOSTA, E ROMPEREBBE TUTTO.** A tempo di
/// esecuzione l'ingresso di un passo *è* l'uscita della sua dipendenza, col
/// `with` sovrapposto: arriva quindi ogni campo che il passo prima ha prodotto.
/// Rifiutarli renderebbe impossibile ogni passo con una dipendenza — lo stesso
/// motivo per cui `toolbox::needs::NeedsSpec` lo dichiara e lo rifiuta.
///
/// Qui i campi non riconosciuti finiscono in `extra` e vengono ignorati come
/// prima. La differenza è che adesso **si possono chiedere**, e `flow check` li
/// chiede sul `with` — che è testo scritto a mano, dove un campo di troppo non è
/// l'uscita di nessuno: è un refuso.
#[derive(Debug, Deserialize)]
pub(crate) struct EngineSpec {
    /// Il comando così com'è. Resta per un comando qualunque — `sh`, `cat`, uno
    /// script — non per un motore: un motore si chiede per identificativo, o il
    /// flusso gira solo dove quel nome è nel percorso di chi esegue.
    #[serde(default)]
    pub(crate) bin: Option<String>,
    /// L'identificativo dello strumento voluto — lo stesso che il rilevatore
    /// della macchina restituisce — oppure una **catena** di identificativi da
    /// provare in ordine.
    #[serde(default)]
    pub(crate) tool: Option<ToolChoice>,
    /// What the text of this step is: `private` never resolves to an engine
    /// whose data pact is `trains` or `unknown`. Absent is `public`.
    #[serde(default)]
    pub(crate) data: Option<DataClass>,
    /// The kind of work (`mechanical`, `research`, `implementation`,
    /// `judgement`, `writing`): the strengths table puts its engines first.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// This step is given what it is handed and nothing else: no session of
    /// another step is continued, whatever it asks. Whoever writes the flow
    /// declares it — a step is not read as a judge by the words in it.
    #[serde(default)]
    pub(crate) blind: bool,
    /// `fuel`: among the chain, the engine whose subscription window would
    /// otherwise expire unused goes first, and the why is said.
    #[serde(default)]
    pub(crate) prefer: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) workdir: Option<String>,
    /// `"tree": "own"` gives this step a git worktree of the project to
    /// itself, named after the run and the step. Two steps of one front then
    /// write over each other's work only if somebody wrote the same name
    /// twice, which the graph does not allow.
    #[serde(default)]
    pub(crate) tree: Option<String>,
    /// Il testo dell'ingresso, se il motore lo legge da lì invece che da un
    /// argomento: JSON non porta byte grezzi, un motore binario sull'ingresso
    /// non è un caso che questa azione copre.
    #[serde(default)]
    pub(crate) stdin: Option<String>,
    /// Gli esiti di fallimento che questo passo dichiara accettabili invece che
    /// rossi. Vuoto — il valore predefinito — significa che ogni fallimento
    /// rompe il passo.
    #[serde(default)]
    pub(crate) accept: Vec<String>,
    /// La forma che questo passo pretende dalla propria risposta.
    ///
    /// **PERCHÉ NON È UN CONTROLLO IN PIÙ MA UN CONTRATTO.** Senza, un passo
    /// restituisce un blocco di testo libero e il passo dopo ci pesca dentro con
    /// un rinvio sperando che la forma sia quella: un motore che un giorno
    /// risponde più prolisso rompe la catena in silenzio. Con, la forma è
    /// scritta una volta, viene chiesta al motore (deve comparire nel prompt, e
    /// qui si controlla che ci sia) e viene fatta rispettare sulla risposta.
    ///
    /// **E PASSA SOLO CIÒ CHE LA FORMA DICHIARA.** I preamboli, i ragionamenti
    /// e i saluti non entrano nell'uscita del passo: al passo dopo arriva
    /// l'oggetto potato sui campi dichiarati. È il risparmio che si paga a ogni
    /// chiamata a valle, ed è la ragione per cui la potatura avviene anche
    /// quando la forma tollererebbe campi in più.
    #[serde(default)]
    pub(crate) answer_shape: Option<ValueSchema>,
    /// Le capacità che questo passo chiede al motore: `response_shape`,
    /// `resume_session`, e qualunque altro nome un descrittore dichiari.
    ///
    /// **DICHIARATO QUI E NON ANCORA USATO, DI PROPOSITO.** Chi lo legge oggi è
    /// `sailor flow check`, che avvisa prima di spendere quando il motore
    /// scelto non dichiara quella capacità. L'esecuzione non cambia: chi non sa
    /// imporre una forma alla risposta continua a farsela chiedere nel prompt
    /// con `answer_shape`, e paga più token — è il vincolo permanente
    /// «indipendenza dal modello», e quel ripiego resta il ripiego.
    ///
    /// **E STA NELLA SPECIFICA PER NON DIVENTARE UN REFUSO.** I campi che
    /// questa azione non riconosce finiscono in `extra`, e il controllo li
    /// nomina come «campi che l'azione non conosce»: un passo che dichiara
    /// onestamente ciò che gli serve si vedrebbe accusare di un errore di
    /// battitura.
    ///
    /// Nessuno lo legge da qui dentro finché le azioni non useranno le
    /// capacità: il permesso è sulla riga sopra e non su tutta la struttura,
    /// così il giorno che qualcuno lo usa il permesso sparisce con lui.
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) needs_capabilities: Vec<String>,
    /// Se questo passo apre una sessione, ne riprende una, o ne ramifica una.
    ///
    /// Assente — il valore predefinito — vuol dire che il passo apre un
    /// processo che non sa niente di ciò che è già stato letto: è come ha
    /// sempre funzionato, ed è ciò che il 31/08/2026 è stato misurato costare
    /// 2,79 volte un prompt solo, perché quattro passi hanno riscoperto lo
    /// stesso albero quattro volte.
    #[serde(default)]
    pub(crate) session: Option<SessionUse>,
    pub(crate) timeout_secs: u64,
    /// Tutto ciò che questa azione non riconosce.
    ///
    /// A tempo di esecuzione è l'uscita della dipendenza e si ignora; a tempo di
    /// controllo, sul solo `with`, è l'elenco dei refusi. Vedi il commento sopra
    /// la struttura.
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

/// What the text of a step is, for the pact an engine must hold to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DataClass {
    Private,
    Public,
}

/// The field a step declares to be given nothing it was not handed.
pub const BLIND: &str = "blind";

/// The field a step declares to work in a tree nobody else touches, and the
/// only word it takes.
pub const TREE: &str = "tree";
pub const A_TREE_OF_ITS_OWN: &str = "own";
