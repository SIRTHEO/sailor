//! How a tool is resolved and asked: the resolver an action registry hands
//! in, and the recipes a descriptor declares for asking, measuring and
//! carrying a session.

use crate::probe::LoginRecipe;
use crate::{Declared, Pointer};

/// Come si passa da «voglio *questo* strumento» all'eseguibile che lo è qui.
///
/// **PERCHÉ UN TRATTO E NON UNA CHIAMATA.** Chi sa quali strumenti esistono su
/// una macchina è `toolbox`, e `toolbox` dipende da questo crate: chiamarlo da
/// qui chiuderebbe un anello. Ma la ragione vera viene prima dell'anello: un
/// flusso non deve sapere *come* si cerca uno strumento. Chi compone il registro
/// delle azioni sceglie — dove Sailor gira si leggono i descrittori, in una
/// prova si risponde senza toccare il disco — e il flusso resta lo stesso file.
pub trait ToolResolver: Send + Sync {
    /// Il percorso dell'eseguibile che vale `id` su questa macchina, oppure il
    /// motivo per cui non si può usare, scritto per una persona: quel testo
    /// finisce dentro il passo rosso, ed è tutto ciò che chi legge avrà.
    fn resolve(&self, id: &str) -> Result<String, String>;

    /// Come si fa una domanda secca a `id`, se il suo descrittore lo dichiara.
    ///
    /// **PERCHÉ IL PASSO NON DEVE SAPERLO.** Finché le opzioni di un motore
    /// stanno scritte dentro un passo — `-p` per uno, `--mode plan --print` per
    /// un altro — quel passo è legato a quel motore, e un flusso «indipendente
    /// dal modello» lo è solo nel nome. Il 29/08/2026 sei passi su sei di un
    /// flusso nominavano lo stesso motore: quando quello ha esaurito la quota,
    /// il flusso è morto mentre un altro motore, installato e vivo, non è stato
    /// nemmeno provato.
    ///
    /// Chi non la dichiara restituisce `None`, e il passo dovrà dire le opzioni
    /// da sé: si funziona peggio, non in silenzio.
    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        None
    }

    /// Whether what is sent to `id` trains its provider's next model. The
    /// default is what nobody measured, and a private step reads it as a no.
    fn data_pact(&self, _id: &str) -> models::pact::DataPact {
        models::pact::DataPact::Unknown
    }

    /// The subscription windows of `id` as fuel, read now; empty when it
    /// declares no channel or the reading failed.
    fn fuel(&self, _id: &str) -> Vec<models::fuel::Fuel> {
        Vec::new()
    }

    /// Come **questo** motore apre, riprende e ramifica una sessione, se lo sa
    /// fare.
    ///
    /// **IL PREDEFINITO È `None`, E QUEL `None` È IL VINCOLO PERMANENTE.** Un
    /// motore che non sa riprendere non diventa un errore e non diventa un ramo
    /// `if` scritto per lui: riceve la riga di comando di sempre, riparte da
    /// zero, e paga di più. È l'unica forma che «indipendenza dal modello»
    /// può prendere qui — la capacità è un dato di chi la dichiara, non una
    /// costante scritta accanto al codice che la userebbe.
    fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
        None
    }

    /// Come si chiede a `id` se la casa da cui parte è autenticata.
    ///
    /// **`None` VUOL DIRE «NESSUNO HA GUARDATO», MAI «È AUTENTICATO».** Chi non
    /// la dichiara non fa scattare nessun avviso e non ne fa scattare nemmeno
    /// uno tranquillizzante: il controllo tace su quel motore, e chi legge sa
    /// che tace. È la stessa regola di `refuses_without_prompt`, e il verso
    /// conta — un predefinito che dicesse di sì renderebbe silenziosa proprio la
    /// condizione che questo canale esiste per rendere visibile.
    fn login_recipe(&self, _id: &str) -> Option<LoginRecipe> {
        None
    }
}

/// Il segnaposto che, dentro le opzioni di una ricetta di sessione, prende il
/// posto dell'identificativo della sessione.
///
/// Sta qui e non in `toolbox` perché è **il contratto fra i due**: chi scrive
/// un file di capacità e chi monta la riga di comando devono nominare la stessa
/// cosa, e due costanti gemelle in due crate divergono al primo che la cambia.
pub const SESSION_PLACEHOLDER: &str = "{session}";

/// Cosa un motore sa fare con le proprie sessioni, in opzioni già scritte.
///
/// **OGNI MODO PORTA LA RIGA INTERA, NON LE OPZIONI IN PIÙ.** Sembra una
/// duplicazione di `AskRecipe::args` e non lo è: su `codex` riprendere non è
/// un'opzione aggiunta, è **un sottocomando diverso** — `codex exec resume
/// <id>` contro `codex exec` — e su `codex` ramificare è un terzo sottocomando
/// ancora, `codex exec fork <id>`. Un modello «aggiungi queste opzioni» non
/// saprebbe esprimere nessuno dei due, e li escluderebbe entrambi per sempre.
/// Verificato il 31/08/2026 con `codex exec --help` su questa macchina.
///
/// Ciò che resta condiviso con la ricetta della domanda resta condiviso: le
/// opzioni del consumo e quelle che devono stare attaccate alla domanda si
/// accodano qui come si accodano là, perché **misurare non deve smettere di
/// funzionare quando si riprende** — sarebbe il modo più elegante di perdere
/// proprio i numeri che dicono se la ripresa conviene.
///
/// `None` su un modo vuol dire che quel motore non lo sa fare: si riparte da
/// zero, e si paga di più.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionRecipe {
    /// Apre una sessione. Se la riga contiene il segnaposto, l'identificativo
    /// lo scegliamo noi; se non lo contiene, lo conia il motore e lo si va a
    /// leggere con `id_from`.
    pub open: Option<Vec<String>>,
    /// Riprende una sessione esistente, che resta la stessa.
    pub resume: Option<Vec<String>>,
    /// Ramifica una sessione esistente: il tronco resta dov'è, e il lavoro di
    /// questo passo non lo tocca.
    pub fork: Option<Vec<String>>,
    /// Dove, in ciò che il motore ha detto, sta l'identificativo della sessione
    /// **che ha appena usato**.
    ///
    /// **SERVE PERCHÉ NON TUTTI LASCIANO SCEGLIERE IL NOME, ED È LA MAGGIORANZA.**
    /// Verificato il 31/08/2026: `codex` non ha nessuna opzione per imporre un
    /// identificativo, ma lo **stampa** — `session id: <uuid>` — nello stesso
    /// flusso di testo da cui il suo descrittore legge già i token. Senza
    /// questa via, i motori che coniano da sé sarebbero esclusi per sempre da
    /// una capacità che hanno.
    ///
    /// **E VALE ANCHE DOPO UNA RAMIFICAZIONE**, che è dove rende di più: un
    /// ramo nasce con un identificativo nuovo che nessuno ci ha chiesto, e
    /// leggerlo è l'unico modo perché un passo ancora più avanti possa
    /// continuare **quel ramo** invece del tronco.
    pub id_from: Option<Pointer>,
}

/// Dove va a finire il testo della domanda quando si interroga un motore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptVia {
    /// Sull'ingresso standard.
    Stdin,
    /// Come ultimo argomento della riga di comando.
    LastArg,
}

/// La riga di comando di una ricetta, **senza** il testo della domanda.
///
/// L'ordine è: le opzioni della domanda, quelle che servono a farsi dire il
/// consumo, e per ultime quelle che devono restare attaccate alla domanda.
///
/// **STA FUORI DAL PUNTO CHE LA USA PERCHÉ SI POSSA GUARDARE SENZA ESEGUIRE
/// NIENTE.** Un ordine sbagliato qui non rompe la compilazione e non rompe
/// nessuna prova sui singoli blocchi: si vede solo lanciando il motore giusto,
/// che è come il guasto 1 è arrivato in produzione e come ci è tornato il
/// 31/08/2026 da un'altra porta.
pub fn command_line(recipe: &AskRecipe) -> Vec<String> {
    command_line_with(recipe, &recipe.args)
}

/// La stessa riga, con le opzioni della domanda sostituite da altre.
///
/// Serve alle sessioni: `codex exec resume <id>` non è `codex exec` con
/// qualcosa in coda, è un'altra riga. Ciò che sta **dopo** le opzioni della
/// domanda — il consumo e ciò che deve restare attaccato al testo — non cambia,
/// ed è il motivo per cui questa funzione esiste invece di lasciar montare la
/// riga a chi chiama: un motore ripreso deve continuare a dire quanto consuma.
pub fn command_line_with(recipe: &AskRecipe, ask_args: &[String]) -> Vec<String> {
    let mut args = ask_args.to_vec();
    if let Some(usage) = &recipe.usage {
        args.extend(usage.args.iter().cloned());
    }
    args.extend(recipe.args_before_prompt.iter().cloned());
    args
}

/// Come si interroga un motore in un colpo solo, e come quel motore dice di
/// **non poter lavorare**.
#[derive(Clone, Debug)]
pub struct AskRecipe {
    /// Le opzioni che vogliono una domanda secca, senza il testo della domanda.
    pub args: Vec<String>,
    /// Dove va il testo della domanda.
    pub prompt: PromptVia,
    /// Le opzioni che devono restare **attaccate alla domanda**, dopo quelle
    /// del consumo. Vuoto per quasi tutti; vedi `Ask::args_before_prompt`.
    pub args_before_prompt: Vec<String>,
    /// I frammenti che, comparendo nell'uscita di un fallimento, dicono che
    /// **questo motore non poteva lavorare** — quota esaurita, credenziali
    /// mancanti — e non che il lavoro fosse sbagliato.
    ///
    /// **PERCHÉ LA DISTINZIONE È TUTTO.** Passare al motore successivo a ogni
    /// fallimento sarebbe la cosa peggiore: un mandato scritto male
    /// scenderebbe la catena fino a un modello che risponde comunque, e la
    /// risposta sbagliata arriverebbe senza che nessuno sappia perché. Si passa
    /// oltre **solo** quando il motore ha dichiarato di non poter lavorare, e
    /// solo con le parole che il suo descrittore dichiara: chi non le dichiara
    /// non fa scattare nessun ripiego.
    pub unusable_when: Vec<String>,
    /// The words that mean the quota is spent, and how long to set the engine
    /// aside when they appear. Empty and `None` when the descriptor does not
    /// tell a spent quota from a missing credential.
    pub exhausted_when: Vec<String>,
    pub cooldown_secs: Option<u64>,
    /// The words after which the engine only waits for a person; on seeing
    /// one the step stops it instead of paying the wait. Empty: waited in full.
    pub waits_for_a_person_when: Vec<String>,
    /// Measured: without a question it exits quietly with an empty stdout
    /// instead of refusing in words.
    pub silent_without_prompt: bool,
    /// I frammenti con cui questo motore rifiuta la riga **montata senza la
    /// domanda**: «la riga andava bene, mancava solo il testo».
    ///
    /// Viaggia con la ricetta e non accanto, perché serve esattamente dove
    /// serve la riga: chi monta `command_line` per provarla a secco deve poter
    /// giudicare la risposta senza tornare a chiedere niente al catalogo.
    /// Vuoto vuol dire «nessuno ha guardato», mai «la riga è sana».
    pub refuses_without_prompt: Vec<String>,
    /// Come si legge **quanto ha consumato**, se il suo descrittore lo dichiara.
    ///
    /// Viaggia sulla stessa strada di tutto il resto della ricetta: chi scrive
    /// un descrittore lo dichiara una volta, e nessun flusso deve conoscerlo.
    /// `None` è la risposta di chi non lo dichiara, e non è un guasto: quel
    /// motore si invoca come prima e i suoi token restano sconosciuti.
    pub usage: Option<UsageRecipe>,
}

/// Le opzioni da aggiungere per farsi dire il consumo, e dove leggerlo.
#[derive(Clone, Debug)]
pub struct UsageRecipe {
    pub args: Vec<String>,
    pub declared: Declared,
}

/// Se questa uscita contiene una delle parole dichiarate. Il confronto ignora
/// maiuscole e minuscole: nessun fornitore promette di non cambiarle. Un
/// frammento vuoto non conta — combacerebbe con tutto, e trasformerebbe
/// qualunque uscita in una corrispondenza.
///
/// **STA QUI IN UNA COPIA SOLA** perché i due elenchi che un descrittore
/// dichiara — «non posso lavorare» e «mancava la domanda» — si leggono nello
/// stesso identico modo. Due funzioni gemelle divergerebbero sul primo
/// dettaglio che qualcuno cambia a una sola delle due, ed è il guasto 10.
pub(crate) fn mentions_any(marks: &[String], output: &str) -> bool {
    let output = output.to_lowercase();
    marks
        .iter()
        .any(|mark| !mark.trim().is_empty() && output.contains(&mark.to_lowercase()))
}

/// Se questa uscita è il modo in cui un motore dice di non poter lavorare.
pub(crate) fn says_it_cannot_work(marks: &[String], output: &str) -> bool {
    mentions_any(marks, output)
}
