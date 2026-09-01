//! Il formato del descrittore e il suo caricamento.
//!
//! UN DESCRITTORE È UN DATO, NON UN RAMO DI CODICE. Qui non compare il nome di
//! nessuno strumento: il codice sa eseguire *una forma* di verifica, mai una
//! verifica in particolare. Aggiungere una riga di comando — la CLI di
//! OpenRouter, quella di domani — è scrivere un oggetto JSON, non ricompilare.
//!
//! UN FILE ROTTO NON DEVE FAR CADERE IL RILEVAMENTO. Ogni elemento si legge da
//! solo: quello che non si legge diventa una segnalazione con dentro il perché,
//! e gli altri passano. Un inventario che tace perché una riga era sbagliata è
//! peggio di un inventario incompleto, perché sembra vuoto invece che parziale.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// I descrittori che il prodotto si porta dietro.
///
/// Sono incorporati nel binario, non cercati in una cartella di installazione:
/// un binario copiato altrove continua a rispondere, e non c'è nessun percorso
/// da indovinare. Restano comunque dati — chi non li vuole li spegne per `id`,
/// chi li vuole diversi li riscrive per `id`, senza toccare il codice.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

/// I cataloghi spediti col prodotto, per nome.
///
/// **PERCHÉ PIÙ DI UNO, DAL 29/08/2026.** Un descrittore risponde a «questa cosa
/// c'è?» e a «quali voci dichiara questo file?»: sono due domande buone per
/// molte cose diverse, non solo per gli strumenti che un passo può invocare. La
/// migrazione a Sailor — trovare i ganci, i servizi e gli script che una persona
/// ha già — è la stessa domanda posta su altri percorsi, e riscriverne il
/// meccanismo sarebbe una seconda copia che diverge dalla prima.
///
/// **RESTANO SEPARATI, E NON È PIGNOLERIA.** Un'automazione altrui non è uno
/// strumento che un passo può invocare: se stesse nello stesso catalogo, il suo
/// `id` comparirebbe fra quelli che [`crate::Tools::resolve`] offre a chi ha
/// scritto un nome sbagliato, e un passo potrebbe perfino nominarla. Il catalogo
/// è l'unità di separazione, e chi vuole un catalogo lo chiede per nome.
pub const BUILTIN_CATALOGS: &[(&str, &str)] = &[
    ("tools", BUILTIN),
    ("automations", include_str!("../descriptors/automations.json")),
];

/// Come si dice a chi legge da dove viene un descrittore.
pub const BUILTIN_SOURCE: &str = "incorporato";

/// Da dove si prendono i descrittori.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Quelli spediti col prodotto: il catalogo `tools`.
    Builtin,
    /// Un altro catalogo spedito col prodotto, per nome. Un nome che nessun
    /// catalogo porta diventa una segnalazione: sbagliarlo in un flusso darebbe
    /// altrimenti un elenco vuoto indistinguibile da «qui non c'è niente».
    BuiltinNamed(String),
    /// Un singolo file JSON.
    File(PathBuf),
    /// Ogni `*.json` dentro una cartella, in ordine di nome.
    Dir(PathBuf),
}

/// Come si verifica che una cosa ci sia.
///
/// Due forme, e bastano entrambe: una riga di comando si riconosce da un
/// eseguibile raggiungibile, un server MCP spesso non ha un eseguibile suo — lo
/// avvia chi lo ospita — e si riconosce dal file che lo dichiara.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// Il nome di un eseguibile da cercare nelle cartelle del percorso.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Un percorso che deve esistere. Ammette `~/`, `$VAR` e `*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Una o più sonde. Un JSON con un oggetto solo si scrive senza le parentesi
/// quadre: chi aggiunge uno strumento nel caso semplice non deve conoscere il
/// caso complicato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Probes {
    One(Probe),
    Many(Vec<Probe>),
}

impl Probes {
    pub fn as_slice(&self) -> &[Probe] {
        match self {
            Probes::One(probe) => std::slice::from_ref(probe),
            Probes::Many(probes) => probes,
        }
    }
}

/// Come si chiede la versione: gli argomenti da passare all'eseguibile trovato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionProbe {
    pub args: Vec<String>,
    /// Il tetto di tempo. Un binario che si mette ad aspettare qualcosa
    /// sull'ingresso non deve poter fermare il rilevamento degli altri.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// La riga giusta è quella che contiene questo testo.
    ///
    /// SERVE, E SI È VISTO SU QUESTA MACCHINA: `ollama --version` stampa prima
    /// un avvertimento su un servizio non raggiungibile, e prendere la prima
    /// riga registrava quell'avvertimento come se fosse un numero di versione.
    /// Il rimedio sta nel dato — chi scrive il descrittore sa che forma ha la
    /// risposta del suo binario — non in un ramo di codice per quel binario.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub must_contain: String,
}

fn default_timeout() -> u64 {
    10
}

/// Come si fa una domanda secca a un motore, e come quel motore dice di non
/// poter lavorare.
///
/// **PERCHÉ STA NEL DESCRITTORE E NON NEL FLUSSO.** Finché `-p` per uno e
/// `--mode plan --print` per un altro stanno scritti dentro i passi, un flusso
/// è legato al motore per cui è stato scritto, e «indipendente dal modello»
/// resta una frase. Qui la differenza fra due motori è un dato, e un motore che
/// non esiste ancora si aggiunge scrivendo un descrittore — senza ricompilare
/// niente, e senza che nessun flusso cambi.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Ask {
    /// Le opzioni che vogliono una risposta sola e non una conversazione,
    /// **senza** il testo della domanda.
    #[serde(default)]
    pub args: Vec<String>,
    /// Dove va il testo della domanda: `stdin` o `last_arg`.
    #[serde(default)]
    pub prompt: PromptPlace,
    /// Le opzioni che devono stare **subito prima della domanda**, dopo tutto
    /// il resto — comprese quelle che servono a farsi dire il consumo.
    ///
    /// **PERCHÉ NON BASTA `args`.** La riga si compone mettendo `args`, poi le
    /// opzioni di `usage`, poi la domanda. Per un motore che prende la domanda
    /// come ultimo argomento questo infila le opzioni del consumo **fra** la
    /// bandiera che introduce la domanda e la domanda stessa: `agy` risponde
    /// «--print took "--output-format" as its prompt», e il testo vero viene
    /// ignorato. È il guasto 1 ricomparso da un'altra porta — allora l'ordine
    /// sbagliato erano due opzioni di `ask` fra loro, oggi è una di `ask`
    /// contro una di `usage` — e non si cura riordinando `args`, perché il
    /// vincolo non è fra le opzioni di uno stesso blocco.
    ///
    /// Un ordine globale non lo risolve: `codex` ha `exec` come sottocomando e
    /// vuole stare **primo**, `agy` vuole `--print` **ultimo**. Sono due
    /// vincoli opposti sullo stesso posto, quindi li dichiara il descrittore.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args_before_prompt: Vec<String>,
    /// Come questo motore dice di **non poter lavorare** — quota finita,
    /// credenziali mancanti — invece di dire che il lavoro era sbagliato.
    ///
    /// È ciò che permette a un passo con una catena di motori di passare al
    /// successivo. Chi non lo dichiara non fa scattare nessun ripiego: si
    /// funziona peggio, mai in silenzio. E si dichiarano **le parole del
    /// fornitore**, non una regola generale: «errore» combacerebbe con
    /// qualunque fallimento e manderebbe un mandato sbagliato giù per tutta la
    /// catena finché qualcuno non risponde comunque.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unusable_when: Vec<String>,
    /// Come questo motore rifiuta la riga **montata senza la domanda**: le
    /// parole con cui dice «la riga andava bene, mancava solo il testo».
    ///
    /// **A COSA SERVE.** È la sola forma innocua di provare una riga di comando
    /// vera. Un motore invocato senza domanda non chiama nessun fornitore e non
    /// costa niente, ma percorre lo stesso parsing di argomenti di una chiamata
    /// vera: se la riga è malformata lo dice qui, gratis, invece di dirlo alla
    /// prima corsa che si paga. È la cura scritta accanto al guasto 1 e rimasta
    /// scoperta fino al guasto 27 — **eseguire davvero** la riga composta.
    ///
    /// **PERCHÉ È UN DATO E NON UNA REGOLA SCRITTA NEL CODICE.** Perché i
    /// motori non si somigliano, e la misura del 31/08/2026 su questa macchina
    /// lo dice in due modi. Primo: ognuno rifiuta con parole sue — «Input must
    /// be provided either through stdin or as a prompt argument when using
    /// --print», «No prompt provided via stdin.», «flag needs an argument:
    /// -print» — e nessuna regola generale le copre tutte e tre senza combaciare
    /// anche con un fallimento vero. Secondo, ed è il motivo per cui il verdetto
    /// non può venire dal codice d'uscita: i quattro motori rifiutano con **due
    /// soli codici** (1 per `claude` e `codex`, 2 per `agy`), e i **due** esiti
    /// di `agy` che contano — il rifiuto sano e la riga malformata del guasto 27
    /// — escono **tutti e due 2**. Chi giudicasse dall'esito li vedrebbe uguali,
    /// e passerebbe sopra al guasto 27 come ci è passato sopra chi l'ha scritto.
    ///
    /// **VUOTO VUOL DIRE «NESSUNO HA GUARDATO», MAI «VA BENE».** È la stessa
    /// distinzione del blocco `capabilities`, applicata alla riga invece che
    /// alle capacità: un motore che non dichiara come rifiuta non è un motore la
    /// cui riga è sana — è un motore la cui riga non è stata provata, e chi
    /// legge il rapporto deve poter distinguere le due cose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refuses_without_prompt: Vec<String>,
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Come si chiede a un motore **se la casa da cui parte è autenticata**.
///
/// **PERCHÉ NON SI GUARDA IL DISCO.** Un controllo che cercasse `auth.json`
/// sarebbe una seconda copia della verità: da riscrivere per ogni motore, e da
/// tenere allineata a mano mentre i motori cambiano dove mettono le proprie
/// cose. Chi sa rispondere è il motore. Qui si dichiara soltanto **come si
/// chiede** e **con quali parole risponde**, che è la disciplina già seguita da
/// `unusable_when` e da `refuses_without_prompt`.
///
/// **PERCHÉ NON BASTA IL VAGLIO A SECCO, ED È IL GUASTO 39.** `flow check` prova
/// la riga **senza la domanda**: il motore si ferma su «non mi hai dato niente
/// da fare» e non arriva mai ai controlli che vengono dopo — le credenziali
/// stanno di là. Rimisurato il 01/09/2026 su questa macchina, casa vuota contro
/// casa piena: `codex exec < /dev/null` risponde **la stessa cosa** — «Reading
/// prompt from stdin...», «No prompt provided via stdin.» — e **esce 1** tutte e
/// due le volte. È l'identità delle due risposte che conta, non il numero: il
/// vaglio non ha nessun difetto da riparare, gli manca una domanda, e questa è
/// quella domanda.
///
/// **NON C'È UN CAMPO PER LA FORMA, PERCHÉ LA FORMA LA DICE IL PUNTATORE.**
/// Stessa scelta di `models::usage::read_text`: un cammino di chiavi vale su un
/// involucro JSON, un'espressione regolare sul testo. Chiedere anche la forma
/// permetterebbe di rispondere in modo incoerente, e quell'incoerenza non darebbe
/// un errore — darebbe una risposta sconosciuta senza motivo visibile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LoginStatus {
    /// Le opzioni, o il sottocomando, con cui si fa la domanda: `["login",
    /// "status"]` per `codex`, `["auth", "status"]` per `claude`.
    ///
    /// **DEVE ESSERE UNA DOMANDA, MAI UN GESTO.** Il controllo la esegue davvero.
    /// `codex login` e `claude auth login` aprono un browser e cambiano lo stato
    /// della macchina: scriverli qui vorrebbe dire che un controllo di routine
    /// riautentica il computer di chi lo lancia.
    #[serde(default)]
    pub args: Vec<String>,
    /// Dove sta la risposta dentro ciò che il motore ha detto.
    ///
    /// **È IL PUNTATORE DI `usage`, NON UN SECONDO MECCANISMO**, perché il
    /// problema è lo stesso: due motori dicono la stessa cosa in due forme. In
    /// prosa — `codex` risponde «Logged in using ChatGPT» — non c'è niente da
    /// puntare, e il soggetto è tutto ciò che ha detto. In JSON serve il cammino:
    /// `claude` mette la risposta in un campo booleano, `{"loggedIn": true}`, e
    /// `["loggedIn"]` ci arriva.
    ///
    /// Assente non vuol dire «non guardare»: vuol dire «il soggetto è l'uscita
    /// intera», che è il caso più comune e non chiede di dichiarare niente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Where>,
    /// Le parole con cui questo motore dichiara di **essere** autenticato.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logged_in_when: Vec<String>,
    /// Le parole con cui dichiara di **non** esserlo.
    ///
    /// **SERVONO TUTTE E DUE, E MEZZA DICHIARAZIONE SPEGNE IL CONTROLLO.** Chi
    /// sapesse riconoscere solo il sì chiamerebbe «non capito» ogni no, e chi
    /// legge non distinguerebbe più una casa senza credenziali da un motore che
    /// ha risposto qualcosa di strano. Meglio tacere che dire la cosa comoda.
    ///
    /// **E SI LEGGONO PRIMA DI QUELLE DEL SÌ**, perché «Not logged in» contiene
    /// «logged in»: il modo di dire di no è quasi sempre il modo di dire di sì
    /// con una negazione davanti. L'ordine lo impone `judge_login_status`, così
    /// non dipende da quanto è stato attento chi ha scritto il descrittore.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logged_out_when: Vec<String>,
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Come si legge **quanto ha consumato** un motore, dichiarato dal descrittore.
///
/// **PERCHÉ STA QUI E NON NEL CODICE.** È la stessa ragione di `Ask`, applicata
/// al conto invece che alla domanda: finché «chiedi `--output-format json` e
/// guarda sotto la chiave `usage`» sta scritto in un ramo `if` per un
/// fornitore, misurare un motore nuovo vuol dire ricompilare Sailor. Qui la
/// differenza fra due motori è un dato, e chi ne aggiunge uno scrive un file.
///
/// **CHI NON LO DICHIARA NON PEGGIORA.** Il campo è facoltativo: un motore
/// senza `usage` si invoca esattamente come prima, produce la stessa uscita, e
/// lascia i propri token a **sconosciuto** — mai a zero. Uno zero scritto al
/// posto di «non lo so» è una bugia che nessuna vista a valle può correggere.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Usage {
    /// Le opzioni da aggiungere alla domanda per farsi dire il consumo — per
    /// esempio `["--output-format", "json"]`.
    ///
    /// Si accodano a quelle di `ask`, e **solo** quando è la ricetta del
    /// descrittore a dettare la riga di comando: un passo che scrive i propri
    /// argomenti sta dicendo qualcosa di preciso su *quella* chiamata, e
    /// allungargliela alle spalle sarebbe decidere al posto suo. In quel caso
    /// il consumo resta sconosciuto, che è il prezzo giusto da pagare.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// In che forma leggere l'uscita: `json` (i puntatori sono cammini di
    /// chiavi) o `text` (i puntatori sono espressioni regolari con un gruppo di
    /// cattura).
    #[serde(default)]
    pub read: ReadAs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<Where>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<Where>,
    /// I token d'ingresso **letti dalla cache**. Vanno dichiarati a parte
    /// perché costano a parte, spesso un ordine di grandezza meno: un solo
    /// numero d'ingresso renderebbe la misura falsa proprio dove conta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<Where>,
    /// I token d'ingresso **scritti nella cache**. Non è la stessa cosa che
    /// leggerla: scrivere costa **più** dell'ingresso normale, leggere molto
    /// meno. Su una chiamata misurata il 30/08/2026 questa sola voce era il 96%
    /// della spesa — chi non la dichiara sbaglia il conto verso il basso.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<Where>,
    /// I token scritti in una cache **a lunga durata**, dove esiste e ha un
    /// prezzo suo, più alto di quella breve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_long_tokens: Option<Where>,
    /// Il totale, per i motori che dicono solo quello senza separare i lati.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<Where>,
    /// Quanti turni ha fatto la chiamata: quante volte il modello e' tornato a
    /// parlare dentro una sola invocazione.
    ///
    /// **E' LA VOCE CHE SPIEGA IL CONTO DI UNA CATENA DI PASSI.** Misurato il
    /// 31/08/2026: un flusso di quattro passi legge per turno l'8% in piu' di
    /// una sola sessione che fa lo stesso lavoro, ma fa il doppio dei turni --
    /// e il suo consumo e' il doppio. Chi vuole far costare meno un flusso deve
    /// muovere questo numero, e finora non era misurato da nessuna parte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<Where>,
    /// Dove il motore dichiara quanto ha fatto pagare. Si registra come
    /// confronto: il listino locale resta la fonte di verità.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Where>,
    /// Dove il motore nomina il modello che ha davvero servito la chiamata. È
    /// l'unico legame onesto fra una riga di comando e una voce di listino: chi
    /// non lo dichiara lascia il costo sconosciuto, e va bene così.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Where>,
    /// Dove sta il testo della risposta dentro l'involucro.
    ///
    /// **VA DICHIARATO DA CHI CHIEDE UN INVOLUCRO.** Farsi dire i token in JSON
    /// avvolge anche la risposta: senza questo puntatore un passo a valle
    /// riceverebbe l'involucro al posto del testo, e un flusso che dichiara la
    /// forma della propria risposta diventerebbe rosso per una misura che non
    /// ha chiesto. Misurare non deve cambiare ciò che si misura.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Where>,
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// In che forma un motore dice il proprio consumo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadAs {
    /// L'uscita è un involucro JSON.
    #[default]
    Json,
    /// L'uscita è testo in chiaro.
    Text,
}

/// Dove sta un valore dentro ciò che il motore ha detto: un cammino di chiavi
/// (`["usage", "input_tokens"]`) se si legge JSON, un'espressione regolare col
/// valore nel primo gruppo se si legge testo.
///
/// Un puntatore della forma sbagliata non trova niente e lascia il valore
/// sconosciuto. È il modo giusto di sbagliare: un descrittore impreciso
/// peggiora la misura, non rompe la chiamata che stava misurando, e non
/// inventa mai un numero al posto di quello che non ha trovato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Where {
    Path(Vec<String>),
    Pattern(String),
    /// `{"first_key_of": ["modelUsage"]}` — il valore è **il nome della prima
    /// chiave** dell'oggetto che sta a quel cammino.
    ///
    /// Serve ai motori che il modello non lo mettono in un campo ma lo usano
    /// come chiave. Senza questa forma quel nome è inarrivabile, e senza il nome
    /// nessuna voce di listino si trova: il costo resta sconosciuto anche
    /// avendo tutti i token e un listino giusto.
    FirstKeyOf { first_key_of: Vec<String> },
}

/// Dove va il testo della domanda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPlace {
    /// Sull'ingresso standard. È il caso più comune, e il valore predefinito.
    #[default]
    Stdin,
    /// Come ultimo argomento della riga di comando. Misurato su `agy` il
    /// 28/08/2026: il prompt va in un argomento, e le opzioni vanno prima.
    LastArg,
}

/// Una delle forme con cui si ottiene una capacità: le opzioni da mettere sulla
/// riga di comando, e se l'ultima di loro vuole un valore dopo di sé.
///
/// **PERCHÉ UNA FORMA E NON UNA STRINGA.** `--session-id <uuid>` e
/// `--fork-session` si scrivono uguali in una tabella e si compongono in modo
/// diverso: la prima vuole un valore attaccato, la seconda no. Chi comporrà la
/// riga deve saperlo dal dato, non indovinarlo dal nome — è la stessa ragione
/// per cui `ask.args_before_prompt` esiste invece di un ordine globale.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct CapabilityForm {
    /// Le opzioni, o il sottocomando, con cui la capacità si ottiene.
    /// `["exec", "resume"]` è tanto valido quanto `["--resume"]`: un
    /// sottocomando è un argomento come gli altri.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// L'ultima delle `args` vuole un valore subito dopo.
    #[serde(default)]
    pub takes_value: bool,
    /// I valori ammessi, quando sono un insieme chiuso e misurato — le fonti di
    /// `--setting-sources`, i formati di `--output-format`. Vuoto non vuol dire
    /// «nessuno»: vuol dire che non è un insieme chiuso, o che nessuno l'ha
    /// scritto.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// Il vincolo che la tabella non porta: «solo con `--print`», «perde le
    /// credenziali». È testo per chi legge, e non entra in nessuna decisione.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Che cosa un descrittore dichiara su una capacità.
///
/// **TRE STATI E NON DUE, ED È IL PUNTO DI TUTTO IL BLOCCO.** `false` dice
/// «qualcuno ha guardato e non c'è»; tacere dice «nessuno ha guardato». Sono due
/// fatti diversi, e l'unico modo per non confonderli è avere un modo di scrivere
/// il primo. Un blocco che permettesse solo di elencare ciò che c'è farebbe
/// sembrare misurata ogni assenza, compresa quella di uno strumento che nessuno
/// ha mai aperto.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Capability {
    /// `false`: misurata e non c'è. `true`: c'è, e non ci sono opzioni da
    /// dichiarare — la capacità è nel comportamento, non in una bandiera.
    Known(bool),
    /// Un modo solo, scritto senza le parentesi quadre: chi dichiara il caso
    /// semplice non deve conoscere quello complicato. Stessa scelta di
    /// [`Probes`].
    One(CapabilityForm),
    /// Più modi per la stessa capacità: `claude` riprende una sessione con
    /// `--resume`, con `--session-id` o con `--continue`, e sono tre.
    Many(Vec<CapabilityForm>),
}

impl Capability {
    /// Vero se lo strumento la offre.
    pub fn is_available(&self) -> bool {
        match self {
            Capability::Known(has) => *has,
            Capability::One(_) => true,
            // Un elenco vuoto dichiara di aver guardato senza trovare nessun
            // modo: è un'assenza, non una presenza senza istruzioni.
            Capability::Many(forms) => !forms.is_empty(),
        }
    }

    /// I modi dichiarati, che possono essere zero anche quando c'è.
    pub fn forms(&self) -> &[CapabilityForm] {
        match self {
            Capability::Known(_) => &[],
            Capability::One(form) => std::slice::from_ref(form),
            Capability::Many(forms) => forms,
        }
    }
}

/// Come sta messo uno strumento rispetto a una capacità chiesta da un passo.
///
/// **PERCHÉ NON BASTA UN BOOLEANO.** È la stessa distinzione che
/// [`crate::Presence`] tiene fra «non c'è» e «non ho potuto guardare», portata
/// dal mondo al vocabolario: chi legge un avviso deve sapere se rimediare
/// cambiando motore o misurando quello che ha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    /// Dichiarata, e si ottiene.
    Available,
    /// Dichiarata assente: qualcuno ha guardato, e questo strumento non ce l'ha.
    Absent,
    /// Il descrittore non la nomina. Non è «non ce l'ha»: è «nessuno ha
    /// guardato», e il rimedio è misurare, non cambiare motore.
    NotLookedAt,
}

/// Un descrittore che, invece di dire «questa cosa o c'è o non c'è», **scopre**
/// più voci leggendo un file di configurazione.
///
/// PERCHÉ ESISTE, E PERCHÉ NON È UN CASO SPECIALE PER I SERVER MCP. Elencare a
/// mano i server MCP di una macchina sarebbe l'elenco cablato che questo crate
/// esiste per evitare: cambiano quando l'utente ne aggiunge uno, e nessuno
/// ricompila per questo. Il descrittore dice *dove guardare* e *sotto quale
/// chiave*; il codice apre il file e riporta le chiavi che ci trova, senza
/// sapere che cosa siano. Lo stesso meccanismo elenca i profili di un altro
/// strumento il giorno che qualcuno scrive quel descrittore.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonKeys {
    /// I file da leggere. Ammettono `~/`, `$VAR` e `*`.
    pub files: Vec<String>,
    /// Il cammino fino all'oggetto le cui chiavi sono le voci. Un `*` sta per
    /// «tutte le chiavi di questo livello»: `["projects", "*", "mcpServers"]`
    /// raccoglie i server dichiarati progetto per progetto.
    pub pointer: Vec<String>,
}

/// Come si scoprono più voci invece di rispondere «c'è o non c'è».
///
/// Due forme, e la seconda è nata col catalogo delle automazioni: le chiavi di
/// un file JSON dicono quali ganci una riga di comando dichiara, ma un servizio
/// del sistema operativo è **un file per servizio** in una cartella, e sapere
/// che la cartella non è vuota non serve a niente a chi deve decidere cosa
/// migrare. Chi legge vuole i nomi.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enumerate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_keys: Option<JsonKeys>,
    /// Gli schemi di percorso i cui file esistenti sono le voci. Ammettono
    /// `~/`, `$VAR` e `*`. Una voce è il percorso stesso, per intero: due file
    /// con lo stesso nome in due cartelle diverse sono due automazioni diverse,
    /// e chiamarle allo stesso modo le farebbe contare per una.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

impl Enumerate {
    /// Vero se non dice in nessun modo dove guardare.
    pub fn is_empty(&self) -> bool {
        self.json_keys.is_none() && self.paths.is_none()
    }
}

/// The line that empties a session that is already open, typed as a person
/// would type it.
///
/// A line and not a flag. A flag is how a command line is launched; this is
/// what is said to one already running, and no launch flag can reach a session
/// that is already open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResetContext {
    pub line: String,
    /// For whoever reads: how it was established, and what was not checked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Una riga dell'elenco di cosa cercare.
///
/// **NON RIFIUTA I CAMPI CHE NON CONOSCE, E PRIMA SÌ.** Fino al 31/08/2026
/// portava `deny_unknown_fields`: un descrittore scritto per una versione più
/// nuova di Sailor — o copiato da un esempio più recente — veniva **scartato
/// intero** per un campo in più, e con lui spariva lo strumento. È il guasto 8.
///
/// Adesso il campo di troppo finisce in `extra`, la voce vive, e chi carica
/// lascia una nota che lo nomina. `extra` è anche serializzato: chi rilegge e
/// riscrive un descrittore non perde i campi che questa versione non capisce —
/// il difetto opposto, e altrettanto silenzioso.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Descriptor {
    /// L'identità della riga. Due descrittori con lo stesso `id` non
    /// convivono: l'ultimo caricato vince, ed è così che un utente riscrive un
    /// descrittore spedito senza doverlo cancellare.
    pub id: String,
    /// A quale famiglia appartiene: `ai_cli`, `mcp_server`, `tool`, o qualunque
    /// altra parola. Il codice non ne conosce nessuna — la usa solo per
    /// raggruppare e filtrare, e un nome nuovo funziona il giorno che qualcuno
    /// lo scrive.
    pub family: String,
    /// Come si chiama per chi legge.
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<Probes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enumerate: Option<Enumerate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionProbe>,
    /// Come gli si fa una domanda secca, per chi ne accetta una. Senza questo,
    /// un passo che lo vuole usare deve scrivere da sé le opzioni.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
    /// Come si legge quanto ha consumato. Facoltativo, e deve restarlo: un
    /// descrittore scritto prima che questo campo esistesse deve continuare a
    /// caricarsi identico, altrimenti una versione nuova di Sailor spegnerebbe
    /// gli strumenti dichiarati con la vecchia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Come gli si chiede se la casa da cui parte è autenticata.
    ///
    /// **CHI NON LO DICHIARA NON FA SCATTARE NIENTE, E IL VERSO È LA
    /// DECISIONE.** Assente vuol dire «nessuno ha guardato», mai «è
    /// autenticato»: la stessa frase scritta per `refuses_without_prompt`, e
    /// qui vale identica. Un predefinito che dicesse di sì renderebbe silenziosa
    /// proprio la condizione che questo blocco esiste per rendere visibile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_status: Option<LoginStatus>,
    /// Che cosa questo strumento sa fare oltre a rispondere: riprendere una
    /// sessione, imporre una forma alla risposta, isolarsi dalla configurazione
    /// di chi lo ospita, ricevere una dotazione, tenere un tetto di spesa suo.
    ///
    /// **IL CODICE NON CONOSCE NESSUN NOME DI CAPACITÀ, E NON DEVE.** È una
    /// mappa da nome a dichiarazione: aggiungere una capacità a uno strumento
    /// nuovo è scrivere un file JSON, mai ricompilare. È il vincolo permanente
    /// «programmiamo a codice solo ciò che tocca il mondo» applicato a un
    /// vocabolario — e il giorno che una riga di comando espone qualcosa che
    /// oggi non esiste, il suo descrittore lo dichiara e questo campo non cambia.
    ///
    /// **CHI NON DICHIARA NIENTE CONTINUA A FUNZIONARE.** Una capacità assente
    /// non è un errore: è una condizione dichiarata, e chi non ce l'ha paga di
    /// più — la forma della risposta chiesta nel prompt invece che imposta dal
    /// motore. È il vincolo permanente «indipendenza dal modello», e il ripiego
    /// di oggi resta il ripiego.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capabilities: BTreeMap<String, Capability>,
    /// How a session of this command line that is already running is told to
    /// drop what it holds.
    ///
    /// Absent means nobody measured it, never «it cannot be done». Anything
    /// that read absence as a default would type a guess into a working
    /// session, and a wrong line typed into one cannot be taken back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_context: Option<ResetContext>,
    /// Dove vive la sua configurazione. Ammette `~/`, `$VAR` e `*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<String>,
    /// Una nota per chi legge l'elenco: da dove si installa, come si chiama il
    /// pacchetto. Non entra in nessuna decisione.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Spegne un descrittore senza cancellarlo. È il modo per togliere di mezzo
    /// uno di quelli spediti: si riscrive il suo `id` con `disabled: true`.
    #[serde(default)]
    pub disabled: bool,
    /// I campi che questa versione di Sailor non conosce.
    ///
    /// Vivono qui invece di far cadere la voce, e vengono riscritti tali e
    /// quali quando il descrittore si serializza. Chi carica li nomina in una
    /// nota: ignorare in silenzio sarebbe il guasto 20 su un altro oggetto.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Descriptor {
    /// I campi che non sono stati capiti, col percorso per trovarli.
    ///
    /// Guarda anche dentro `ask` e `usage`: un campo ignoto là sotto faceva
    /// cadere la voce esattamente come uno di primo livello, e chi legge la nota
    /// deve sapere **dove** cercarlo, non solo che c'è.
    pub fn unknown_fields(&self) -> Vec<String> {
        let mut found: Vec<String> = self.extra.keys().cloned().collect();
        if let Some(ask) = &self.ask {
            found.extend(ask.extra.keys().map(|key| format!("ask.{key}")));
        }
        if let Some(usage) = &self.usage {
            found.extend(usage.extra.keys().map(|key| format!("usage.{key}")));
        }
        if let Some(login) = &self.login_status {
            found.extend(login.extra.keys().map(|key| format!("login_status.{key}")));
        }
        if let Some(reset) = &self.reset_context {
            found.extend(reset.extra.keys().map(|key| format!("reset_context.{key}")));
        }
        // Il nome della capacità non è un campo ignoto — nessun nome lo è, per
        // costruzione. Ignoto è quello che sta dentro una delle sue forme.
        for (name, capability) in &self.capabilities {
            for form in capability.forms() {
                found.extend(
                    form.extra
                        .keys()
                        .map(|key| format!("capabilities.{name}.{key}")),
                );
            }
        }
        found
    }

    /// The line that empties a running session, when somebody has measured it.
    ///
    /// `None` is the answer that must reach whoever asks: it says the machine
    /// does not know, and knowing nothing is a reason to refuse, never a reason
    /// to fall back on a line that belongs to a different command line.
    pub fn reset_line(&self) -> Option<&str> {
        self.reset_context
            .as_ref()
            .map(|reset| reset.line.as_str())
            .filter(|line| !line.is_empty())
    }

    /// Come sta messo questo strumento rispetto a una capacità chiesta.
    pub fn capability(&self, name: &str) -> CapabilityState {
        match self.capabilities.get(name) {
            None => CapabilityState::NotLookedAt,
            Some(capability) if capability.is_available() => CapabilityState::Available,
            Some(_) => CapabilityState::Absent,
        }
    }

    /// Dove questo descrittore dice due cose diverse sullo stesso fatto.
    ///
    /// **È IL GUASTO 32, E LA CURA NON È RIPARARE I DUE DESCRITTORI DI OGGI.**
    /// `gemini-cli` dichiarava `capabilities.ask_without_interaction` senza
    /// avere un blocco `ask`: la riga non si componeva, nessuna catena lo
    /// nominava, e in ventitré chiamate registrate non era mai stato invocato —
    /// cioè una capacità vera e inservibile somigliava in tutto a una capacità
    /// che c'è. Ripararlo a mano avrebbe lasciato il quarto descrittore libero
    /// di rifare lo stesso, perché **il difetto non è in un file: è nel non
    /// aver mai confrontato i due blocchi**.
    ///
    /// **PERCHÉ IN LIBRERIA E NON DENTRO UNA PROVA.** Una prova guarda solo i
    /// descrittori spediti. Chi ne scrive uno in `~/.config/sailor/tools.d/` —
    /// che è il modo previsto di aggiungere un motore, senza ricompilare —
    /// starebbe fuori da ogni controllo. La regola sta in un posto solo, e la
    /// interrogano sia la prova sui descrittori spediti sia `sailor flow
    /// check` sui descrittori di chi lo lancia.
    ///
    /// L'elenco vuoto vuol dire che i blocchi si reggono. Non vuol dire che
    /// siano giusti: dice che non si smentiscono fra loro.
    pub fn contradictions(&self) -> Vec<String> {
        let mut found = Vec::new();
        let can_be_asked = self.ask.is_some();
        let says_it_can = self.capability(ASK_WITHOUT_INTERACTION) == CapabilityState::Available;

        // **LE DUE DIREZIONI SONO DUE DIFETTI DIVERSI, E SERVONO TUTTE E DUE.**
        // Chi ha la riga e tace sulla capacità fa credere a chi legge le
        // capacità che quel motore non si possa interrogare; chi dichiara la
        // capacità senza la riga fa credere il contrario. Una sola delle due
        // lascerebbe passare metà dei casi.
        if can_be_asked && !says_it_can {
            found.push(format!(
                "dichiara come gli si fa una domanda (blocco `ask`) e non dichiara di \
                 saperne ricevere (`capabilities.{ASK_WITHOUT_INTERACTION}`): due blocchi \
                 dello stesso file che si smentiscono"
            ));
        }
        if says_it_can && !can_be_asked {
            found.push(format!(
                "dichiara `capabilities.{ASK_WITHOUT_INTERACTION}` e non ha nessun blocco \
                 `ask`: nessuna riga si può montare per fargli la domanda, quindi la \
                 capacità è vera e inservibile"
            ));
        }

        if let (Some(ask), Some(capability)) = (
            self.ask.as_ref(),
            self.capabilities.get(ASK_WITHOUT_INTERACTION),
        ) {
            // La riga che si monta davvero è quella di `ask`: un'opzione
            // dichiarata solo fra le capacità sta descrivendo un altro motore.
            let composed: Vec<&str> = ask
                .args
                .iter()
                .chain(ask.args_before_prompt.iter())
                .map(String::as_str)
                .collect();
            for form in capability.forms() {
                for option in &form.args {
                    if !composed.contains(&option.as_str()) {
                        found.push(format!(
                            "dichiara la capacità con «{option}», che nel suo blocco `ask` non \
                             compare: la riga che si monta davvero è quella di `ask`"
                        ));
                    }
                }
            }
        }

        // **UN FRAMMENTO VUOTO È UNA DICHIARAZIONE CHE COMBACIA CON TUTTO.**
        // Chi lo scrive non se ne accorgerebbe mai: il descrittore funziona, e
        // risponde di sì a qualunque uscita. In `unusable_when` farebbe scendere
        // la catena a ogni fallimento — cioè il difetto che la catena esiste per
        // non introdurre — e in `refuses_without_prompt` farebbe passare per
        // sana ogni riga rotta.
        if let Some(ask) = self.ask.as_ref() {
            for (field, marks) in [
                ("unusable_when", &ask.unusable_when),
                ("refuses_without_prompt", &ask.refuses_without_prompt),
            ] {
                if marks.iter().any(|mark| mark.trim().is_empty()) {
                    found.push(format!(
                        "dichiara un frammento vuoto in `ask.{field}`, che è contenuto in \
                         qualunque uscita: combacerebbe sempre"
                    ));
                }
            }
        }

        found
    }

    /// Perché questo strumento non può fare da ripiego dentro una catena, se
    /// non può. `None` quando può.
    ///
    /// **È IL GUASTO 31, ED È UNA REGOLA DI POSIZIONE, NON DI DESCRITTORE.** Un
    /// motore che non dichiara con quali parole dice di non poter lavorare non
    /// è un descrittore sbagliato: è un descrittore onesto su una misura che
    /// nessuno ha fatto, e l'elenco vuoto vuol dire «nessuno ha guardato», mai
    /// «va bene». Diventa un difetto solo dove qualcuno gli mette qualcuno
    /// dietro: `says_it_cannot_work` su un elenco vuoto è `false`, quindi il suo
    /// esaurirsi passa per un fallimento qualunque, il passo muore lì, e i
    /// motori dopo di lui non partono mai. In fondo a una catena la stessa
    /// assenza non toglie niente a nessuno — non c'è nessuno a cui passare il
    /// lavoro — e pretendere lì una misura sarebbe pretenderla per niente.
    pub fn cannot_be_a_fallback(&self) -> Option<String> {
        let Some(ask) = self.ask.as_ref() else {
            return Some(
                "il suo descrittore non dichiara come gli si fa una domanda (`ask`), quindi \
                 non c'è nessuna riga da montare quando il lavoro gli arriva"
                    .to_owned(),
            );
        };
        if ask
            .unusable_when
            .iter()
            .any(|mark| !mark.trim().is_empty())
        {
            return None;
        }
        Some(
            "non dichiara con quali parole dice di non poter lavorare (`ask.unusable_when`), \
             quindi il suo esaurirsi passa per un fallimento qualunque: il passo muore su di \
             lui e i motori dopo non partono mai"
                .to_owned(),
        )
    }
}

/// Il nome della capacità che parla di domande secche.
///
/// **È L'UNICO NOME DI CAPACITÀ CHE IL CODICE PRONUNCIA, E HA UNA RAGIONE.**
/// Il vocabolario resta un dato — aggiungerne una a uno strumento nuovo è
/// scrivere un file JSON — ma questa sola capacità risponde alla **stessa
/// domanda** di un altro blocco dello stesso file: «si può interrogare questo
/// motore senza aprirci una conversazione?». Il blocco `ask` la risponde
/// montando una riga, `capabilities` la risponde dichiarandola, e due copie
/// della stessa verità divergono da sole se nessuno le confronta. Il confronto
/// ha bisogno del nome; nessun'altra capacità ne ha bisogno, e nessun'altra sta
/// qui.
pub const ASK_WITHOUT_INTERACTION: &str = "ask_without_interaction";

/// Un descrittore che dice due cose diverse sullo stesso fatto.
///
/// Porta il nome dello strumento perché chi legge un elenco di contraddizioni
/// deve sapere quale voce aprire: «un descrittore si contraddice» non si può
/// riparare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Contradiction {
    pub tool: String,
    pub said: String,
}

impl Contradiction {
    /// Una riga per chi legge un rapporto.
    pub fn line(&self) -> String {
        format!("«{}»: {}", self.tool, self.said)
    }
}

/// Un descrittore caricato, con da dove viene: chi legge il risultato deve
/// poter risalire al file che lo ha prodotto, o «da quale descrittore» non è una
/// risposta verificabile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: Descriptor,
    pub source: String,
}

/// Qualcosa che non si è potuto caricare, col perché e col dove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    /// L'`id` se si è riusciti a leggerlo, altrimenti la posizione nel file.
    pub about: String,
    pub reason: String,
}

/// L'elenco di cosa cercare, più le righe che non si sono lette.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub descriptors: Vec<Loaded>,
    /// Le voci **perse**: non sono nel catalogo, e qui c'è il perché.
    pub problems: Vec<Problem>,
    /// Le voci **tenute**, con qualcosa che è stato ignorato.
    ///
    /// **NON STANNO IN `problems`, E LA DIFFERENZA È TUTTA.** Un problema dice
    /// «questo strumento non c'è»; una nota dice «c'è, e di lui ho ignorato un
    /// campo». Metterle insieme farebbe contare come guasti delle voci che
    /// funzionano — e ci sono prove che contano `problems` una per una, che
    /// diventerebbero rosse per un descrittore perfettamente vivo.
    pub notes: Vec<Problem>,
}

/// Il testo di un catalogo spedito, per nome.
pub fn builtin_catalog(name: &str) -> Option<&'static str> {
    BUILTIN_CATALOGS
        .iter()
        .find(|(catalog, _)| *catalog == name)
        .map(|(_, text)| *text)
}

impl Catalog {
    /// Carica in ordine: chi arriva dopo vince sull'`id` di chi c'era.
    pub fn load(sources: &[Source]) -> Catalog {
        let mut catalog = Catalog::default();
        for source in sources {
            match source {
                Source::Builtin => catalog.absorb(BUILTIN_SOURCE, BUILTIN),
                Source::BuiltinNamed(name) => match builtin_catalog(name) {
                    Some(text) => catalog.absorb(&format!("{BUILTIN_SOURCE}:{name}"), text),
                    None => catalog.problems.push(Problem {
                        source: BUILTIN_SOURCE.to_string(),
                        about: name.clone(),
                        reason: format!(
                            "nessun catalogo spedito si chiama così; quelli spediti sono: {}",
                            BUILTIN_CATALOGS
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }),
                },
                Source::File(path) => catalog.absorb_file(path),
                Source::Dir(dir) => {
                    let Ok(entries) = fs::read_dir(dir) else {
                        // Una cartella che non c'è non è un guasto: è il caso
                        // normale di chi non ha mai aggiunto un descrittore suo.
                        // Una cartella che c'è ma non si legge lo è, e lo si
                        // distingue guardando il disco, non l'errore.
                        if dir.exists() {
                            catalog.problems.push(Problem {
                                source: dir.to_string_lossy().into_owned(),
                                about: "la cartella".to_string(),
                                reason: "non si è potuta leggere".to_string(),
                            });
                        }
                        continue;
                    };
                    let mut files: Vec<PathBuf> = entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
                        .collect();
                    files.sort();
                    for file in files {
                        catalog.absorb_file(&file);
                    }
                }
            }
        }
        catalog
    }

    fn absorb_file(&mut self, path: &Path) {
        let label = path.to_string_lossy().into_owned();
        match fs::read_to_string(path) {
            Ok(text) => self.absorb(&label, &text),
            Err(error) => self.problems.push(Problem {
                source: label,
                about: "il file".to_string(),
                reason: format!("non si è potuto leggere: {error}"),
            }),
        }
    }

    /// IL TESTO SI LEGGE DUE VOLTE, DI PROPOSITO. Prima come JSON generico, poi
    /// elemento per elemento: leggere l'array intero come `Vec<Descriptor>`
    /// farebbe cadere venti descrittori buoni per una virgola sbagliata nel
    /// ventunesimo, e la segnalazione non direbbe nemmeno quale.
    fn absorb(&mut self, source: &str, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "il file".to_string(),
                    reason: format!("non è JSON valido: {error}"),
                });
                return;
            }
        };
        // Un array nudo o `{"tools": [...]}`: chi aggiunge uno strumento scrive
        // la forma che gli viene, e nessuna delle due è sbagliata.
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("tools") {
                Some(Value::Array(items)) => items.clone(),
                _ => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about: "il file".to_string(),
                        reason: "non contiene né un array né un campo `tools`".to_string(),
                    });
                    return;
                }
            },
            _ => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "il file".to_string(),
                    reason: "non contiene né un array né un campo `tools`".to_string(),
                });
                return;
            }
        };
        for (index, item) in items.iter().enumerate() {
            let about = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.to_string())
                .unwrap_or_else(|| format!("la voce numero {}", index + 1));
            let descriptor: Descriptor = match serde_json::from_value(item.clone()) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            // **UN CAMPO IGNOTO È UNA NOTA, NON UN RIFIUTO.** Prima faceva
            // cadere la voce intera, e con lei spariva lo strumento: guasto 8.
            // La nota va in `notes` e non in `problems`, perché il descrittore
            // c'è ed è vivo — chi conta i problemi conta le voci perse.
            let unknown = descriptor.unknown_fields();
            if !unknown.is_empty() {
                self.notes.push(Problem {
                    source: source.to_string(),
                    about: about.clone(),
                    reason: format!(
                        "campi che questa versione non conosce, ignorati: {}",
                        unknown.join(", ")
                    ),
                });
            }
            if descriptor.detect.is_none() && descriptor.enumerate.is_none() {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "non dice come si verifica: manca `detect` e manca `enumerate`"
                        .to_string(),
                });
                continue;
            }
            // UN `enumerate` VUOTO NON SCOPRE NIENTE, e senza questa riga
            // risponderebbe «nessuna voce» — che si legge come «qui non c'è
            // niente» invece che come «l'elenco è scritto male».
            if descriptor
                .enumerate
                .as_ref()
                .is_some_and(|enumerate| enumerate.is_empty())
            {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "`enumerate` non dice dove guardare: manca `json_keys` e manca `paths`"
                        .to_string(),
                });
                continue;
            }
            self.replace(Loaded {
                descriptor,
                source: source.to_string(),
            });
        }
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .descriptors
            .iter_mut()
            .find(|l| l.descriptor.id == loaded.descriptor.id)
        {
            Some(existing) => *existing = loaded,
            None => self.descriptors.push(loaded),
        }
    }

    /// Ogni contraddizione di ogni descrittore vivo, in ordine stabile.
    ///
    /// Guarda i vivi e non i disabilitati: un descrittore spento non compone
    /// nessuna riga e non dichiara niente a nessuno — chiamarlo contraddittorio
    /// manderebbe a riparare un file che non è in servizio.
    pub fn contradictions(&self) -> Vec<Contradiction> {
        let mut found = Vec::new();
        for loaded in self.live() {
            for said in loaded.descriptor.contradictions() {
                found.push(Contradiction {
                    tool: loaded.descriptor.id.clone(),
                    said,
                });
            }
        }
        found
    }

    /// Quelli da eseguire: senza gli spenti, in ordine stabile per `id`, perché
    /// due letture di seguito devono dare la stessa sequenza o il confronto fra
    /// un giorno e l'altro non vale niente.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .descriptors
            .iter()
            .filter(|l| !l.descriptor.disabled)
            .collect();
        out.sort_by(|a, b| {
            (&a.descriptor.family, &a.descriptor.id).cmp(&(&b.descriptor.family, &b.descriptor.id))
        });
        out
    }
}

#[cfg(test)]
mod the_new_field_is_optional {
    //! Che cosa succede a un descrittore quando questa versione di Sailor
    //! impara un campo che la precedente non conosceva.

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-descrittori-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    fn loaded(name: &str, text: &str) -> Catalog {
        let dir = scratch(name);
        let file = dir.join("descrittori.json");
        std::fs::write(&file, text).expect("scrivere i descrittori");
        Catalog::load(&[Source::File(file)])
    }

    /// **IL CRITERIO (d) DEL MANDATO.** Un descrittore scritto prima che `usage`
    /// esistesse si carica identico. Questo crate ha un guasto aperto e noto —
    /// un campo che questa versione non conosce scarta il descrittore intero —
    /// e il campo nuovo non deve peggiorarlo: chi non ce l'ha continua a
    /// funzionare, altrimenti una versione nuova di Sailor spegnerebbe in
    /// silenzio gli strumenti dichiarati con la vecchia.
    #[test]
    fn a_descriptor_written_before_usage_existed_still_loads() {
        let catalog = loaded(
            "senza-usage",
            r#"[{
              "id": "vecchio", "family": "ai_cli", "label": "Vecchio",
              "detect": { "command": "vecchio" },
              "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert_eq!(catalog.descriptors.len(), 1);
        let descriptor = &catalog.descriptors[0].descriptor;
        assert!(descriptor.usage.is_none(), "assente vuol dire assente");
        assert!(descriptor.ask.is_some(), "e il resto arriva intatto");
    }

    /// Ogni descrittore spedito col prodotto si carica: se il campo nuovo
    /// rendesse illeggibile anche uno solo di loro, quello strumento sparirebbe
    /// dalla macchina di chiunque aggiorni.
    #[test]
    fn every_shipped_descriptor_still_loads() {
        let catalog = Catalog::load(&[Source::Builtin]);
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert!(catalog.descriptors.len() > 5);
    }

    /// Il blocco `usage` nella forma con i cammini di chiavi.
    #[test]
    fn a_json_usage_block_is_read_pointer_by_pointer() {
        let catalog = loaded(
            "usage-json",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "ask": { "args": ["-p"], "prompt": "stdin" },
              "usage": {
                "args": ["--output-format", "json"],
                "read": "json",
                "input_tokens": ["usage", "input_tokens"],
                "cached_tokens": ["usage", "cache_read_input_tokens"],
                "cost": ["total_cost_usd"],
                "model": ["model"],
                "answer": ["result"]
              }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let usage = catalog.descriptors[0]
            .descriptor
            .usage
            .as_ref()
            .expect("il blocco c'è");
        assert_eq!(usage.args, vec!["--output-format", "json"]);
        assert_eq!(usage.read, ReadAs::Json);
        assert_eq!(
            usage.input_tokens,
            Some(Where::Path(vec!["usage".into(), "input_tokens".into()]))
        );
        assert_eq!(
            usage.cached_tokens,
            Some(Where::Path(vec![
                "usage".into(),
                "cache_read_input_tokens".into()
            ])),
            "la cache ha un puntatore suo, separato dall'ingresso"
        );
        assert_eq!(usage.answer, Some(Where::Path(vec!["result".into()])));
        assert_eq!(usage.output_tokens, None, "ciò che non è scritto non c'è");
    }

    /// La forma testuale: i puntatori sono espressioni regolari.
    #[test]
    fn a_text_usage_block_reads_its_pointers_as_patterns() {
        let catalog = loaded(
            "usage-text",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "usage": { "read": "text", "total_tokens": "tokens used\\s*\\n\\s*([\\d.,]+)" }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let usage = catalog.descriptors[0].descriptor.usage.as_ref().unwrap();
        assert_eq!(usage.read, ReadAs::Text);
        assert_eq!(
            usage.total_tokens,
            Some(Where::Pattern("tokens used\\s*\\n\\s*([\\d.,]+)".to_owned()))
        );
    }

    /// **UN CAMPO INVENTATO SI DICE, MA NON PORTA VIA LO STRUMENTO.**
    ///
    /// **QUESTA PROVA DICEVA IL CONTRARIO, E IL RIBALTAMENTO È DELIBERATO.**
    /// Prima pretendeva `descriptors.len() == 0`: un refuso dentro `usage`
    /// faceva cadere l'intero descrittore, e con lui spariva lo strumento — è il
    /// guasto 8. La ragione scritta allora era buona («non un silenzio che poi
    /// lascia il consumo sconosciuto senza dire perché») ed è **ancora
    /// rispettata**: il campo viene nominato. Quello che cambia è il prezzo —
    /// prima si perdeva il motore, adesso si perde solo il campo che nessuno
    /// sapeva leggere.
    #[test]
    fn an_invented_field_inside_usage_is_named_without_losing_the_tool() {
        let catalog = loaded(
            "usage-sbagliato",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "usage": { "read": "json", "token_di_ingresso": ["a"] }
            }]"#,
        );

        assert_eq!(catalog.descriptors.len(), 1, "lo strumento resta usabile");
        assert!(
            catalog.problems.is_empty(),
            "e non è una voce persa: {:?}",
            catalog.problems
        );
        assert_eq!(catalog.notes.len(), 1, "ma non è nemmeno un silenzio");
        assert!(
            catalog.notes[0].reason.contains("usage.token_di_ingresso"),
            "la nota dice quale campo e dove sta: {}",
            catalog.notes[0].reason
        );
    }

    /// **UN CAMPO INVENTATO AL PRIMO LIVELLO, STESSA REGOLA.** Il caso vero per
    /// cui il guasto 8 esiste: un descrittore scritto per una versione più nuova
    /// di Sailor, o copiato da un esempio più recente.
    ///
    /// **L'ESEMPIO ERA `capabilities`, E IL 31/08/2026 HA SMESSO DI ESSERLO.**
    /// Quel campo adesso questa versione lo conosce, quindi non è più ignoto: un
    /// esempio che invecchia così non fa diventare rossa la prova nel punto
    /// giusto, la fa diventare rossa e basta. Serve un nome che nessuna versione
    /// legga, e questo è il difetto di ogni prova che usa come «ignoto» un nome
    /// plausibile.
    #[test]
    fn a_descriptor_from_a_newer_version_still_loads() {
        let catalog = loaded(
            "dal-futuro",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "streams_partial_answers": true
            }]"#,
        );

        assert_eq!(catalog.descriptors.len(), 1);
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert_eq!(catalog.notes.len(), 1);
        assert!(
            catalog.notes[0].reason.contains("streams_partial_answers"),
            "{}",
            catalog.notes[0].reason
        );
    }

    /// **E CIÒ CHE NON SI CAPISCE NON SI PERDE RISCRIVENDOLO.** Un descrittore
    /// riletto e riscritto da questa versione conserva i campi del futuro:
    /// perderli sarebbe il difetto opposto, e altrettanto silenzioso.
    #[test]
    fn what_this_version_does_not_understand_survives_a_round_trip() {
        let catalog = loaded(
            "andata-e-ritorno",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "streams_partial_answers": true
            }]"#,
        );

        let written = serde_json::to_value(&catalog.descriptors[0].descriptor)
            .expect("un descrittore in memoria si riscrive sempre");

        assert_eq!(
            written["streams_partial_answers"],
            serde_json::json!(true),
            "il campo ignoto torna fuori com'era: {written}"
        );
    }

    /// **CHI NON DICHIARA NIENTE CONTINUA A FUNZIONARE.** Un descrittore
    /// scritto prima che `capabilities` esistesse si carica identico e risponde
    /// «nessuno ha guardato» a qualunque domanda: è il vincolo permanente
    /// «indipendenza dal modello»: una capacità assente non è un errore.
    #[test]
    fn a_descriptor_written_before_capabilities_existed_still_loads() {
        let catalog = loaded(
            "senza-capacita",
            r#"[{
              "id": "vecchio", "family": "ai_cli", "label": "Vecchio",
              "detect": { "command": "vecchio" },
              "ask": { "args": ["-p"], "prompt": "stdin" }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert!(catalog.notes.is_empty(), "{:?}", catalog.notes);
        let descriptor = &catalog.descriptors[0].descriptor;

        assert!(descriptor.capabilities.is_empty());
        assert_eq!(
            descriptor.capability("response_shape"),
            CapabilityState::NotLookedAt
        );
        assert!(descriptor.ask.is_some(), "e il resto arriva intatto");
    }

    /// **LE TRE RISPOSTE POSSIBILI SU UNA CAPACITÀ, IN UNA PROVA SOLA.** Se
    /// «dichiarata assente» e «mai guardata» dessero la stessa risposta, il
    /// blocco intero non servirebbe a niente: si potrebbe elencare solo ciò che
    /// c'è, e ogni silenzio passerebbe per una misura.
    #[test]
    fn a_capability_can_be_present_declared_absent_or_never_looked_at() {
        let catalog = loaded(
            "tre-stati",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "capabilities": {
                "choose_model": { "args": ["--model"], "takes_value": true },
                "fork_session": false
              }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let descriptor = &catalog.descriptors[0].descriptor;

        assert_eq!(
            descriptor.capability("choose_model"),
            CapabilityState::Available
        );
        assert_eq!(
            descriptor.capability("fork_session"),
            CapabilityState::Absent,
            "scritto `false` vuol dire che qualcuno ha guardato"
        );
        assert_eq!(
            descriptor.capability("resume_session"),
            CapabilityState::NotLookedAt,
            "non nominata non vuol dire assente"
        );
    }

    /// Una capacità con più modi si scrive come elenco; una con un modo solo
    /// senza le parentesi quadre. Le due forme convivono nello stesso blocco.
    #[test]
    fn one_way_needs_no_brackets_and_several_ways_are_a_list() {
        let catalog = loaded(
            "modi",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "capabilities": {
                "resume_session": [
                  { "args": ["--resume"] },
                  { "args": ["--session-id"], "takes_value": true }
                ],
                "fork_session": { "args": ["--fork-session"] }
              }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let descriptor = &catalog.descriptors[0].descriptor;

        let resume = &descriptor.capabilities["resume_session"];
        assert_eq!(resume.forms().len(), 2);
        assert_eq!(resume.forms()[0].args, vec!["--resume"]);
        assert!(
            !resume.forms()[0].takes_value,
            "una bandiera non vuole un valore attaccato"
        );
        assert!(
            resume.forms()[1].takes_value,
            "e `--session-id` sì: chi compone la riga lo deve leggere dal dato"
        );

        let fork = &descriptor.capabilities["fork_session"];
        assert_eq!(fork.forms().len(), 1, "un modo solo, senza parentesi quadre");
    }

    /// **UN CAMPO INVENTATO DENTRO UNA FORMA NON PORTA VIA LO STRUMENTO.** È il
    /// guasto 8 sul blocco nuovo: la regola vale per intero o non vale.
    #[test]
    fn an_invented_field_inside_a_capability_is_named_without_losing_the_tool() {
        let catalog = loaded(
            "capacita-sbagliata",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "capabilities": { "resume_session": { "opzioni": ["--resume"] } }
            }]"#,
        );

        assert_eq!(catalog.descriptors.len(), 1, "lo strumento resta usabile");
        assert!(
            catalog.problems.is_empty(),
            "e non è una voce persa: {:?}",
            catalog.problems
        );
        assert_eq!(catalog.notes.len(), 1, "ma non è nemmeno un silenzio");
        assert!(
            catalog.notes[0]
                .reason
                .contains("capabilities.resume_session.opzioni"),
            "la nota dice quale campo, dentro quale capacità: {}",
            catalog.notes[0].reason
        );
    }

    /// **I QUATTRO MOTORI SPEDITI DICHIARANO OGNI CAPACITÀ DEL VOCABOLARIO.**
    ///
    /// Non che ce l'abbiano: che l'abbiano **guardata**. Un motore che tace su
    /// una capacità è indistinguibile da uno che non ce l'ha, e il blocco esiste
    /// per non confonderli — quindi il primo posto dove la distinzione va
    /// rispettata sono i descrittori spediti col prodotto.
    #[test]
    fn every_shipped_engine_answers_about_every_capability() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let vocabulary = [
            "ask_without_interaction",
            "response_shape",
            "resume_session",
            "fork_session",
            "isolate_from_user_config",
            "receive_equipment",
            "native_spend_cap",
            "choose_model",
            "fallback_model",
        ];
        for id in ["claude-code", "codex", "agy", "gemini-cli"] {
            let engine = catalog
                .live()
                .into_iter()
                .find(|loaded| loaded.descriptor.id == id)
                .unwrap_or_else(|| panic!("{id} è spedito col prodotto"));
            for name in vocabulary {
                assert_ne!(
                    engine.descriptor.capability(name),
                    CapabilityState::NotLookedAt,
                    "{id} non dice niente su «{name}»: «non ce l'ha» e «nessuno ha \
                     guardato» sono due fatti diversi"
                );
            }
        }
    }

    /// E almeno un'assenza dichiarata c'è davvero, misurata su questa macchina:
    /// senza, la prova qui sopra passerebbe anche dichiarando tutto presente.
    #[test]
    fn a_shipped_engine_declares_a_capability_it_does_not_have() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let agy = catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == "agy")
            .expect("agy è spedito col prodotto");
        assert_eq!(
            agy.descriptor.capability("native_spend_cap"),
            CapabilityState::Absent,
            "misurato con --help il 31/08/2026: agy non ha nessun tetto di spesa suo"
        );
    }

    /// **CHI DICE COME SI CHIEDE DICE ANCHE COME RIFIUTA.**
    ///
    /// La gemella della prova qui sopra, sul blocco `ask` invece che su
    /// `capabilities`, e per la stessa ragione: un motore che dichiara come
    /// gli si fa una domanda ha una riga di comando composta, e una riga
    /// composta che nessuno ha mai eseguito è esattamente il guasto 1 e il
    /// guasto 27. Le opzioni erano state lette dalla documentazione, ognuna
    /// giusta per conto suo, e sbagliate insieme.
    ///
    /// **QUESTA PROVA NON PUÒ ESSERE VERDE PER OMISSIONE.** Il campo si riempie
    /// solo montando la riga vera e guardando cosa risponde il motore: non c'è
    /// un valore ragionevole da indovinare, e un elenco vuoto la lascia rossa.
    /// È nata rossa su tutti e tre i motori che hanno un blocco `ask`.
    #[test]
    fn every_shipped_engine_that_asks_declares_how_it_refuses_without_a_prompt() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let mut checked = 0;
        for loaded in catalog.live() {
            let Some(ask) = loaded.descriptor.ask.as_ref() else {
                continue;
            };
            checked += 1;
            assert!(
                !ask.refuses_without_prompt.is_empty(),
                "«{}» dichiara come gli si fa una domanda ma non come rifiuta la \
                 riga montata senza domanda: la sua riga di comando non è mai \
                 stata eseguita, e nessun controllo se ne accorgerebbe. Si \
                 misura montando la riga e non dando il testo — non costa niente",
                loaded.descriptor.id
            );
            for mark in &ask.refuses_without_prompt {
                assert!(
                    !mark.trim().is_empty(),
                    "«{}» dichiara un frammento vuoto, che combacia con qualunque \
                     uscita e farebbe passare per sana ogni riga rotta",
                    loaded.descriptor.id
                );
            }
        }
        // Senza questa riga la prova sarebbe verde su un catalogo svuotato, che
        // è il modo più silenzioso di smettere di controllare.
        assert!(
            checked >= 3,
            "solo {checked} motori spediti hanno un blocco `ask`: erano tre il \
             31/08/2026, e se sono meno qualcuno l'ha tolto"
        );
    }

    /// **CHI DICHIARA COME SI CHIEDE DICHIARA TUTTE E DUE LE RISPOSTE.**
    ///
    /// Mezza dichiarazione è peggio di nessuna, e il verso in cui sbaglia è
    /// sempre quello comodo: un descrittore che sapesse riconoscere solo il sì
    /// chiamerebbe «non capito» ogni no, e chi legge non distinguerebbe più una
    /// casa senza credenziali da un motore che ha risposto qualcosa di strano.
    ///
    /// **E LE PAROLE DEL SÌ NON DEVONO STARE DENTRO QUELLE DEL NO.** «logged in»
    /// è contenuto in «not logged in»: con quelle due un motore che dice di no
    /// verrebbe letto come un sì da chiunque cercasse il sì per primo. Il codice
    /// legge il no per primo apposta, ma un descrittore che si regge solo su
    /// quell'ordine è un descrittore che dice il falso a chi lo legge — e
    /// `sailor profiles list` mostra quelle parole a una persona.
    #[test]
    fn every_shipped_engine_that_asks_about_login_declares_both_answers() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let mut checked = 0;
        for loaded in catalog.live() {
            let Some(login) = loaded.descriptor.login_status.as_ref() else {
                continue;
            };
            let id = &loaded.descriptor.id;
            checked += 1;
            assert!(
                !login.args.is_empty(),
                "«{id}» dichiara come si riconosce la risposta e non come si fa la \
                 domanda: non c'è niente da eseguire"
            );
            for (which, marks) in [
                ("logged_in_when", &login.logged_in_when),
                ("logged_out_when", &login.logged_out_when),
            ] {
                assert!(
                    marks.iter().any(|mark| !mark.trim().is_empty()),
                    "«{id}» non dichiara `{which}`: mezza dichiarazione non \
                     distingue niente, e l'errore cadrebbe dalla parte che \
                     tranquillizza"
                );
            }
            for yes in &login.logged_in_when {
                for no in &login.logged_out_when {
                    assert!(
                        !no.to_lowercase().contains(&yes.to_lowercase()),
                        "«{id}»: le parole del sì («{yes}») stanno dentro quelle del \
                         no («{no}»), quindi una casa vuota somiglia a una piena. \
                         Si dichiarano parole più lunghe, misurate"
                    );
                }
            }
        }
        // Senza questa riga la prova resterebbe verde su un catalogo a cui
        // qualcuno ha tolto il blocco: il modo più silenzioso di smettere.
        assert!(
            checked >= 2,
            "solo {checked} motori spediti dichiarano `login_status`: erano due il \
             01/09/2026 (claude-code e codex), e se sono meno qualcuno l'ha tolto"
        );
    }

    /// **NESSUN DESCRITTORE SPEDITO SI CONTRADDICE. È LA GUARDIA DEL GUASTO
    /// 32, E NON HA ECCEZIONI REGISTRATE.**
    ///
    /// Fino al 01/09/2026 questa regola era spezzata in tre: una metà verde che
    /// girava, l'altra metà dietro un `#[ignore]` con dentro il nome di
    /// `gemini-cli`, e accanto una terza prova che sorvegliava l'elenco delle
    /// eccezioni perché un quarto motore non ci entrasse di soppiatto. Un
    /// elenco di eccezioni è la forma che prende una regola quando la si scrive
    /// prima di poterla rispettare: sorvegliarlo era la cosa giusta da fare, e
    /// non è la stessa cosa che non averne bisogno.
    ///
    /// Adesso la regola vive in `Descriptor::contradictions`, in un posto solo,
    /// e la interroga anche `sailor flow check` sui descrittori di chi lancia —
    /// che è dove una prova sui soli descrittori spediti non arriva.
    #[test]
    fn no_shipped_descriptor_contradicts_itself() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let found = catalog.contradictions();
        assert!(
            found.is_empty(),
            "descrittori che dicono due cose diverse sullo stesso fatto: {}",
            found
                .iter()
                .map(Contradiction::line)
                .collect::<Vec<_>>()
                .join("; ")
        );
        // Senza questa riga la prova sarebbe verde su un catalogo svuotato, che
        // è il modo più silenzioso di smettere di controllare.
        let engines = catalog
            .live()
            .into_iter()
            .filter(|loaded| loaded.descriptor.ask.is_some())
            .count();
        assert!(
            engines >= 4,
            "solo {engines} motori spediti hanno un blocco `ask`: erano quattro il \
             01/09/2026, e se sono meno qualcuno l'ha tolto invece di ripararlo"
        );
    }

    /// **E LA GUARDIA PRENDE TUTTE E QUATTRO LE FORME, SU DESCRITTORI SCRITTI
    /// APPOSTA.**
    ///
    /// Serve perché la prova qui sopra, da sola, resterebbe verde anche se
    /// `contradictions` restituisse sempre l'elenco vuoto: un controllo che non
    /// controlla niente si presenta esattamente come un mondo sano. Qui il
    /// mondo è malato per costruzione, e la guardia deve dirlo.
    #[test]
    fn the_guard_names_every_way_two_blocks_can_disagree() {
        let catalog = loaded(
            "contraddittori",
            r#"[
              {
                "id": "dice-di-si-e-non-ha-la-riga", "family": "ai_cli",
                "detect": { "command": "primo" },
                "capabilities": { "ask_without_interaction": { "args": ["-p"] } }
              },
              {
                "id": "ha-la-riga-e-tace", "family": "ai_cli",
                "detect": { "command": "secondo" },
                "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] }
              },
              {
                "id": "due-opzioni-diverse", "family": "ai_cli",
                "detect": { "command": "terzo" },
                "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] },
                "capabilities": { "ask_without_interaction": { "args": ["--print"] } }
              },
              {
                "id": "un-frammento-vuoto", "family": "ai_cli",
                "detect": { "command": "quarto" },
                "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["   "] },
                "capabilities": { "ask_without_interaction": { "args": ["-p"] } }
              }
            ]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);

        let said: BTreeMap<String, String> = catalog
            .contradictions()
            .into_iter()
            .map(|found| (found.tool, found.said))
            .collect();

        assert_eq!(said.len(), 4, "una per descrittore, e sono quattro: {said:?}");
        assert!(
            said["dice-di-si-e-non-ha-la-riga"].contains("nessun blocco `ask`"),
            "{said:?}"
        );
        assert!(
            said["ha-la-riga-e-tace"].contains("non dichiara di saperne ricevere"),
            "{said:?}"
        );
        assert!(
            said["due-opzioni-diverse"].contains("--print"),
            "l'opzione che non combacia si nomina, o non si sa cosa correggere: {said:?}"
        );
        assert!(
            said["un-frammento-vuoto"].contains("frammento vuoto"),
            "{said:?}"
        );
    }

    /// **UN MOTORE CHE NON DICE COME SI ESAURISCE NON PUÒ FARE DA RIPIEGO, E
    /// UNO CHE LO DICE SÌ.** È il guasto 31 letto sul descrittore; dove la
    /// posizione in una catena lo renda un difetto lo decide chi legge i flussi.
    ///
    /// Le due metà stanno in una prova sola apposta: con la sola prima, un
    /// `cannot_be_a_fallback` che rispondesse sempre «no» sarebbe verde; con la
    /// sola seconda, uno che rispondesse sempre «sì» lo sarebbe.
    ///
    /// **PERCHÉ IL MONDO QUI È SCRITTO APPOSTA, DAL 01/09/2026.** Fino a quel
    /// giorno la metà negativa era `agy`, preso dai descrittori spediti perché
    /// era il motore che nessuno aveva ancora misurato. È un appoggio che si
    /// rompe da sé: appena `agy` è stato misurato e ha dichiarato le proprie
    /// parole, questa prova è morta con `expect("agy non dichiara nessun
    /// unusable_when")` — cioè **il lavoro di qualcun altro l'ha fatta cadere
    /// facendo la cosa giusta**. Una prova sulla regola non deve dipendere da
    /// quale strumento capiti a essere incompleto oggi: quel fatto cambia, e
    /// cambia proprio quando qualcuno lavora bene. Il mondo malato lo si
    /// costruisce, come per la guardia sulle contraddizioni qui sopra.
    ///
    /// Che i motori **spediti** stiano a posto è un'altra domanda, e ha il suo
    /// posto: `every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted`
    /// la fa sui flussi veri, dove ha una conseguenza.
    #[test]
    fn only_an_engine_that_says_how_it_runs_out_can_be_a_fallback() {
        let catalog = loaded(
            "ripieghi",
            r#"[
              {
                "id": "dice-come-finisce", "family": "ai_cli",
                "detect": { "command": "primo" },
                "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["weekly limit"] }
              },
              {
                "id": "tace", "family": "ai_cli",
                "detect": { "command": "secondo" },
                "ask": { "args": ["-p"], "prompt": "stdin" }
              },
              {
                "id": "dice-solo-frammenti-vuoti", "family": "ai_cli",
                "detect": { "command": "terzo" },
                "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["   "] }
              }
            ]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let of = |id: &str| {
            catalog
                .live()
                .into_iter()
                .find(|loaded| loaded.descriptor.id == id)
                .unwrap_or_else(|| panic!("{id} sta nel catalogo scritto qui"))
                .descriptor
                .cannot_be_a_fallback()
        };

        assert!(
            of("dice-come-finisce").is_none(),
            "chi dichiara le proprie parole può stare in mezzo: il lavoro passa oltre"
        );

        let why = of("tace").expect("chi non dichiara niente non può fare da ripiego");
        assert!(why.contains("unusable_when"), "{why}");
        assert!(
            why.contains("non partono mai"),
            "il motivo dice cosa si perde, non solo cosa manca: {why}"
        );

        // **UN ELENCO DI FRAMMENTI VUOTI NON È UN ELENCO.** `mentions_any` li
        // scarta uno per uno, quindi `says_it_cannot_work` resta `false` e il
        // motore è un tappo esattamente come chi tace — ma a chi legge il
        // descrittore sembra che qualcuno abbia guardato.
        assert!(
            of("dice-solo-frammenti-vuoti").is_some(),
            "un `unusable_when` di soli frammenti vuoti si comporta come un elenco \
             vuoto, e va detto: altrimenti la forma di una dichiarazione passa per \
             una dichiarazione"
        );
    }

    /// Il descrittore di `codex` spedito col prodotto dichiara come si legge il
    /// suo consumo, e lo dichiara nella forma testuale: è l'unico formato che
    /// su questa macchina sia stato davvero misurato.
    #[test]
    fn the_shipped_codex_descriptor_declares_how_to_read_its_tokens() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let codex = catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == "codex")
            .expect("codex è spedito col prodotto");
        let usage = codex
            .descriptor
            .usage
            .as_ref()
            .expect("codex dichiara il proprio consumo");
        assert_eq!(usage.read, ReadAs::Text);
        assert!(usage.total_tokens.is_some());
        assert!(
            usage.args.is_empty(),
            "codex scrive già i token da sé: chiedergli qualcosa in più \
             cambierebbe la sua riga di comando per niente"
        );
        assert!(
            usage.answer.is_none(),
            "nessun involucro chiesto, quindi niente da spacchettare: \
             l'uscita del passo resta quella di sempre"
        );
    }
}
