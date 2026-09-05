//! Two questions asked of an engine without spending: whether the line a
//! descriptor mounts is sound, tried without the prompt, and whether the home
//! it starts from is authenticated.

use crate::equipment::current_equipment_for;
use crate::process::{invoke_external_engine, EngineInvocation, EngineResult};
use crate::recipe::{command_line, mentions_any, says_it_cannot_work, AskRecipe, PromptVia};
use crate::{read_scalar, Pointer};
use std::collections::BTreeMap;
use std::time::Duration;

// ── la prova a secco di una riga di comando ─────────────────────────────

/// Come sta messa una riga di comando montata da un descrittore, provata
/// **senza dare la domanda**.
///
/// **PERCHÉ NON C'È UN «PASSATO/FALLITO».** Cinque esiti perché ci sono cinque
/// riparazioni diverse, e chi legge deve sapere quale gli tocca: una riga rotta
/// si corregge nel descrittore, un motore esaurito si aspetta, un descrittore
/// che tace si misura, un motore che non risponde si indaga. Metterne due sotto
/// la stessa parola manda a fare il lavoro sbagliato.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// Il motore ha detto «mancava solo la domanda»: la riga è montata bene.
    Sound,
    /// Il motore si è lamentato di **qualcos'altro**: la riga è malformata, e
    /// le sue parole sono la diagnosi. Sul guasto 27 la frase di `agy` diceva
    /// esattamente quale bandiera aveva mangiato quale argomento — nessuna
    /// classificazione nostra avrebbe potuto dire altrettanto.
    Broken { said: String },
    /// Il motore ha detto di non poter lavorare adesso — quota, credenziali —
    /// e questo non dice niente sulla riga: si riprova quando torna.
    CannotWork { said: String },
    /// Il descrittore non dichiara come questo motore rifiuta senza domanda.
    /// **Non è «la riga è sana»**: è che nessuno ha guardato.
    NotDeclared,
    /// Nessuna risposta dentro il tetto di tempo, o processo che non è partito.
    ///
    /// Il motivo viaggia col verdetto perché le due cose si riparano in modi
    /// diversi, e un rapporto che le confondesse manderebbe a cercare un motore
    /// lento dove c'è un eseguibile che non parte.
    TimedOut { why: String },
}

/// Il verdetto su una riga provata a secco, **senza eseguire niente**.
///
/// **PERCHÉ È UNA FUNZIONE PURA E SEPARATA DA CHI ESEGUE.** Perché il giudizio
/// è la parte che si sbaglia, e una prova che debba avviare un motore vero per
/// interrogarlo non si scrive: si prova con i testi che i motori hanno detto
/// davvero, copiati una volta e poi fermi lì.
///
/// **IL VERDETTO STA NEL TESTO, NON NEL CODICE D'USCITA**, e non è una
/// preferenza. Misurato il 31/08/2026 su questa macchina: `agy` esce **2** sia
/// quando rifiuta bene («flag needs an argument: -print») sia quando la riga è
/// quella malformata del guasto 27 («--print took "--output-format" as its
/// prompt…»). Una sonda che giudicasse dall'esito vedrebbe i due casi identici,
/// e passerebbe sopra al guasto 27 esattamente come ci è passato sopra chi
/// l'ha scritto. Per questo questa funzione non riceve nemmeno il codice
/// d'uscita: non c'è modo di usarlo per sbaglio.
///
/// **L'ORDINE DI LETTURA È VINCOLANTE: PRIMA `unusable_when`.** Un motore che
/// ha finito la quota si lamenta di quello, non della riga; letto nell'ordine
/// opposto, un `claude` esaurito verrebbe dichiarato **rotto** — e chi legge
/// andrebbe a correggere un descrittore sano mentre bastava aspettare. Un
/// motore esaurito non è un motore rotto.
pub fn judge_dry_run(recipe: &AskRecipe, stdout: &str, stderr: &str) -> ProbeVerdict {
    // Le due pipe si guardano insieme: chi scrive il rifiuto su stdout e chi lo
    // scrive su stderr sono lo stesso caso, e sceglierne una sola avrebbe reso
    // il verdetto dipendente da un dettaglio che nessun descrittore dichiara.
    let said = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if says_it_cannot_work(&recipe.unusable_when, &said) {
        return ProbeVerdict::CannotWork { said };
    }
    // An engine measured to answer nothing without a question is sound when
    // stdout is empty; stderr may carry a spinner and is not read here.
    if recipe.silent_without_prompt {
        return if stdout.trim().is_empty() {
            ProbeVerdict::Sound
        } else {
            ProbeVerdict::Broken { said }
        };
    }
    if recipe
        .refuses_without_prompt
        .iter()
        .all(|mark| mark.trim().is_empty())
    {
        return ProbeVerdict::NotDeclared;
    }
    if mentions_any(&recipe.refuses_without_prompt, &said) {
        return ProbeVerdict::Sound;
    }
    ProbeVerdict::Broken { said }
}

/// Cosa ha detto un motore alla riga montata senza domanda, o perché non ha
/// detto niente.
#[derive(Clone, Debug)]
pub enum DryRun {
    Answered { stdout: String, stderr: String },
    NoAnswer { why: String },
}

/// Chi esegue la prova a secco.
///
/// **PERCHÉ UN TRATTO E NON UNA CHIAMATA DIRETTA.** Perché altrimenti ogni
/// prova su questo codice dovrebbe avviare `claude`, `codex` e `agy` veri: la
/// batteria dipenderebbe da cosa è installato su chi la esegue e da come sta
/// messa la quota di quel giorno — cioè non potrebbe venire diversa per la
/// ragione che dichiara. Con un tratto le prove iniettano quattro finti
/// eseguibili e ottengono quattro verdetti, sempre gli stessi.
pub trait DryProbe: Send + Sync {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun;
}

/// Il tetto di tempo di una prova a secco.
///
/// **SERVE UN TETTO ESPLICITO PERCHÉ SU QUESTA MACCHINA `timeout` E `gtimeout`
/// NON ESISTONO**: verificato il 31/08/2026 con `command -v`. Chi si aspettasse
/// di poterli mettere davanti alla riga scoprirebbe il contrario solo quando un
/// motore si mette ad aspettare qualcosa e blocca il controllo di tutti gli
/// altri. Il tetto lo mette `invoke_external_engine`, che ce l'ha già.
pub const DRY_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// La sonda vera: monta la riga e la esegue senza dare la domanda.
pub struct RealDryProbe;

impl DryProbe for RealDryProbe {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun {
        // **LA STESSA DOTAZIONE DELLA CORSA VERA, E QUI STA TUTTO IL VALORE DEL
        // VAGLIO.** Fino al 01/09/2026 questa riga era `BTreeMap::new()`: il
        // vaglio provava il motore nella casa di chi aveva aperto il terminale —
        // autenticata — e il passo lo faceva partire in quella del profilo
        // attivo, che può non avere nessuna credenziale. `flow check` chiudeva
        // in verde e la corsa falliva, e chi aveva letto il verde non aveva
        // sbagliato niente. Un controllo che prova un mondo diverso da quello in
        // cui si lavora è peggio di nessun controllo, perché rassicura.
        //
        // **NIENTE DALLO SPAZIO DI UN PASSO, ED È DELIBERATO.** Il vaglio non
        // sta provando un passo: sta provando la riga che il **descrittore**
        // monta, quella sola volta per motore. Le variabili che un passo
        // dichiara valgono per quella chiamata lì, e infilarle qui darebbe un
        // verdetto che non vale per gli altri passi che nominano lo stesso
        // motore.
        //
        // **QUESTO NON DICE SE LA CASA È AUTENTICATA, ED È UN LIMITE DELLA
        // TECNICA.** Il vaglio toglie la domanda apposta, quindi il motore si
        // ferma sulla domanda mancante e non arriva mai ai controlli che
        // verrebbero dopo — le credenziali stanno di là. Rimisurato il
        // 01/09/2026 nelle due case: `codex exec < /dev/null` risponde **la
        // stessa cosa** — «No prompt provided via stdin.» — e esce 1 tutte e due
        // le volte. (Fino a quella misura questa riga diceva «esce zero»: il
        // numero era falso, l'identità delle due risposte no, ed è quella che
        // porta la conclusione.)
        //
        // **LA DOMANDA CHE MANCA SI FA A PARTE, E ADESSO ESISTE**: è
        // `probe_login_status`, che chiede al motore con le parole che il
        // descrittore dichiara in `login_status`. Non va infilata qui: questa
        // sonda prova *la riga*, e mescolare i due verdetti renderebbe
        // impossibile dire quale dei due ha detto di no.
        let equipment = current_equipment_for(bin, &BTreeMap::new());
        let result = invoke_external_engine(&EngineInvocation {
            bin: bin.to_owned(),
            args: args.to_vec(),
            env: equipment.env,
            workdir: None,
            stdin,
            timeout: DRY_PROBE_TIMEOUT,
        });
        match result {
            // Un rifiuto è un'uscita non-zero, quindi il caso normale sta qui;
            // ma un motore che esce **zero** senza domanda è a maggior ragione
            // qualcosa da guardare, e buttarlo via lo nasconderebbe.
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. }
            | EngineResult::WaitingForAPerson { stdout, stderr } => {
                DryRun::Answered { stdout, stderr }
            }
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "nessuna risposta entro {} secondi",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Monta la riga di una ricetta senza la domanda, la fa provare, e giudica.
///
/// **COME SI TOGLIE LA DOMANDA DIPENDE DA DOVE ANDAVA**, ed è la sola parte del
/// montaggio che questa funzione decide: a chi la vuole sull'ingresso si dà un
/// ingresso **vuoto e chiuso** — che è ciò che fa `< /dev/null` — e a chi la
/// vuole come ultimo argomento si dà la riga senza quell'argomento. Sbagliare
/// qui non darebbe un errore: darebbe un motore che *aspetta*, e la prova a
/// secco diventerebbe un modo per appendere il controllo.
pub fn probe_dry_run(probe: &dyn DryProbe, bin: &str, recipe: &AskRecipe) -> ProbeVerdict {
    let args = command_line(recipe);
    let stdin = match recipe.prompt {
        PromptVia::Stdin => Some(Vec::new()),
        PromptVia::LastArg => None,
    };
    match probe.run(bin, &args, stdin) {
        DryRun::Answered { stdout, stderr } => judge_dry_run(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => ProbeVerdict::TimedOut { why },
    }
}

// ── la casa è autenticata? lo dice il motore ─────────────────────────────

/// Come si chiede a un motore **se la casa da cui parte è autenticata**, e con
/// quali parole risponde di sì e di no.
///
/// **PERCHÉ NON SI GUARDA IL DISCO.** Cercare `auth.json` sarebbe una seconda
/// copia della verità, da riscrivere per ogni motore e da tenere allineata a
/// mano mentre i motori cambiano dove mettono le cose. Chi sa rispondere è il
/// motore; il descrittore dichiara soltanto **come si chiede** e **come si
/// riconosce la risposta** — la stessa disciplina di `unusable_when` e
/// `refuses_without_prompt`, applicata a una terza domanda.
///
/// **PERCHÉ SERVE UN CANALE A SÉ, E IL VAGLIO A SECCO NON BASTA.** `flow check`
/// prova la riga **senza la domanda**: il motore si ferma su «non mi hai dato
/// niente da fare» e non arriva mai ai controlli che vengono dopo, dove stanno
/// le credenziali. Misurato il 01/09/2026 nelle due case: `codex exec <
/// /dev/null` risponde «No prompt provided via stdin.» ed esce 1 **in tutte e
/// due**, parola per parola la stessa cosa. È un limite della tecnica, non un
/// difetto da riparare in essa: la domanda sulle credenziali si fa a parte, e
/// costa zero perché è locale — nessun fornitore viene chiamato.
#[derive(Clone, Debug)]
pub struct LoginRecipe {
    /// Le opzioni, o il sottocomando, con cui si fa la domanda: `["login",
    /// "status"]`, `["auth", "status"]`.
    pub args: Vec<String>,
    /// Dove sta la risposta dentro ciò che il motore ha detto.
    ///
    /// **È IL PUNTATORE DI `usage`, NON UN SECONDO MECCANISMO**, e la ragione è
    /// che il problema è lo stesso: due motori dicono la stessa cosa in due
    /// forme diverse. `codex` risponde in prosa — «Logged in using ChatGPT» — e
    /// allora non c'è niente da puntare, il soggetto è tutto ciò che ha detto.
    /// `claude` risponde con un involucro JSON e mette la risposta in un campo
    /// booleano, `"loggedIn": true`, e allora il cammino di chiavi la raggiunge.
    ///
    /// `None` non è «non guardare»: è «il soggetto è l'uscita intera», che è la
    /// forma più comune e quella che non richiede di dichiarare niente.
    pub answer: Option<Pointer>,
    /// Le parole con cui questo motore dichiara di **essere** autenticato.
    pub logged_in_when: Vec<String>,
    /// Le parole con cui dichiara di **non** esserlo.
    ///
    /// **VANNO DICHIARATE TUTTE E DUE, E LA MANCANZA DI UNA SPEGNE IL
    /// CONTROLLO.** Un descrittore che sapesse riconoscere solo il sì
    /// chiamerebbe «non riconosciuto» ogni no, e chi legge non saprebbe
    /// distinguere un motore non autenticato da uno che ha risposto qualcosa di
    /// strano. Meglio tacere: vedi [`LoginVerdict::NotDeclared`].
    pub logged_out_when: Vec<String>,
}

/// Che cosa si è potuto sapere sulle credenziali di una casa.
///
/// **QUATTRO ESITI E NON DUE, PER LA RAGIONE DI SEMPRE.** «Nessuno ha guardato»,
/// «ha risposto e non l'ho capito» e «ha detto di no» sono tre fatti diversi, e
/// **nessuno dei tre è un sì**. Un tipo a due stati costringerebbe a scegliere
/// da che parte far cadere i primi due, e la direzione comoda è sempre quella
/// che tranquillizza — cioè quella che rimette il difetto.
#[derive(Clone, Debug)]
pub enum LoginVerdict {
    /// Il motore dichiara di essere autenticato in questa casa.
    LoggedIn { said: String },
    /// Il motore dichiara di **non** esserlo: le chiamate partiranno senza
    /// credenziali.
    LoggedOut { said: String },
    /// Il descrittore non dichiara il blocco, o lo dichiara a metà. **Nessuno
    /// ha guardato**, e non c'è niente da dire su questa casa.
    NotDeclared,
    /// Ha risposto, e la risposta non somiglia a nessuna delle due forme
    /// dichiarate. Le sue parole sono la diagnosi.
    Unrecognised { said: String },
    /// Non ha risposto affatto: non è partito, o ha superato il tetto di tempo.
    NoAnswer { why: String },
}

impl LoginVerdict {
    /// Vero **solo** quando il motore ha detto di sì. Ogni altro esito, dubbio
    /// compreso, risponde di no: è la forma in cui il verso dell'errore si
    /// scrive una volta sola invece che a ogni luogo di lettura.
    pub fn is_logged_in(&self) -> bool {
        matches!(self, LoginVerdict::LoggedIn { .. })
    }
}

/// Legge la risposta di un motore alla domanda «sei autenticato?».
///
/// **PURA, E SEPARATA DA CHI ESEGUE**, per la stessa ragione di
/// [`judge_dry_run`]: il giudizio è la parte che si sbaglia, e una prova che
/// dovesse lanciare `codex` direbbe com'è messa la macchina di chi la esegue
/// invece che se il riconoscimento funziona.
///
/// **IL CODICE D'USCITA NON ENTRA NEMMENO QUI.** Sui due motori misurati il
/// 01/09/2026 l'esito *distinguerebbe* — `codex login status` esce 1 non
/// autenticato e 0 autenticato, e `claude auth status` fa lo stesso — ma è un
/// fatto di quei due e non una regola che si possa scrivere nel codice: un
/// motore che rispondesse «Not logged in» uscendo zero verrebbe dichiarato
/// autenticato da chiunque leggesse l'esito, e nessuno se ne accorgerebbe. Il
/// testo lo dichiara il descrittore, l'esito no.
///
/// **L'ORDINE DI LETTURA È VINCOLANTE: PRIMA IL NO.** «Not logged in»
/// *contiene* «logged in», e in generale il modo di dire di no è il modo di dire
/// di sì con una negazione davanti. Letto nell'ordine opposto, una casa vuota
/// risulterebbe autenticata — che è precisamente il silenzio che questo blocco
/// esiste per rompere. Le parole dichiarate misurate lo eviterebbero già; questo
/// lo evita anche quando chi scrive il descrittore è stato distratto.
pub fn judge_login_status(recipe: &LoginRecipe, stdout: &str, stderr: &str) -> LoginVerdict {
    // **LE DUE PIPE INSIEME, E QUI NON È UN DETTAGLIO**: `codex login status`
    // non scrive niente su stdout — la risposta è tutta su stderr, misurato il
    // 01/09/2026. Chi ne leggesse una sola non troverebbe mai nessuna delle due
    // forme e direbbe sempre «nessuno ha guardato».
    let said = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }

    // Il puntatore sceglie il soggetto, e basta: le parole si cercano dentro
    // quello, con la stessa regola di `unusable_when`. Senza puntatore il
    // soggetto è ciò che il motore ha detto per intero.
    let subject = match recipe.answer.as_ref() {
        None => Some(said.clone()),
        Some(pointer) => read_scalar(&said, pointer),
    };
    // Un puntatore che non trova niente non è un sì: l'involucro non era quello
    // che il descrittore dichiarava, e la risposta resta sconosciuta.
    let Some(subject) = subject else {
        return LoginVerdict::Unrecognised { said };
    };

    // **SI MOSTRA IL SOGGETTO, NON L'INVOLUCRO CHE LO CONTENEVA.** La risposta
    // vera di `claude auth status` porta con sé l'indirizzo di posta del
    // proprietario, l'identificativo e il nome della sua organizzazione e il
    // tipo di abbonamento; questo testo finisce in `sailor profiles list` e nel
    // rapporto di `sailor flow check`, cioè in due uscite che si incollano in
    // una consegna e si versano in un registro. **Una diagnosi non deve
    // portarsi dietro chi la usa.**
    //
    // Non si perde niente: dove un puntatore c'è, il valore che ha isolato *è*
    // la risposta — «false» è più preciso dell'involucro, non meno — e dove non
    // c'è, il soggetto è già tutto ciò che il motore ha detto. È la stessa
    // regola delle righe rotte («le parole del motore per intero») applicata a
    // un motore che risponde con un campo invece che con una frase.
    let shown = if recipe.answer.is_some() {
        subject.clone()
    } else {
        said
    };

    if mentions_any(&recipe.logged_out_when, &subject) {
        return LoginVerdict::LoggedOut { said: shown };
    }
    if mentions_any(&recipe.logged_in_when, &subject) {
        return LoginVerdict::LoggedIn { said: shown };
    }
    LoginVerdict::Unrecognised { said: shown }
}

/// Chi fa la domanda locale «sei autenticato?», **dentro una casa precisa**.
///
/// **PERCHÉ UN TRATTO A SÉ E NON [`DryProbe`].** Sono due domande diverse su due
/// mondi diversi: il vaglio a secco prova la riga nella casa del profilo attivo,
/// e chi la compone non la sceglie; questa domanda va fatta **in una casa
/// nominata** — `sailor profiles list` la fa a ogni profilo, non solo a quello
/// in forza, e con `DryProbe` non avrebbe modo di dirlo. L'ambiente è quindi un
/// argomento, non una cosa che l'esecutore va a leggersi da solo.
pub trait LoginProbe: Send + Sync {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun;
}

/// Le due domande locali che si possono fare a un motore senza spendere.
///
/// Sta insieme perché chi controlla un flusso le fa tutte e due nello stesso
/// momento e sullo stesso mondo; separate, ogni luogo di chiamata dovrebbe
/// portarsi due argomenti che valgono sempre la stessa cosa.
pub trait EngineProbe: DryProbe + LoginProbe {}

impl<T: DryProbe + LoginProbe> EngineProbe for T {}

impl LoginProbe for RealDryProbe {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun {
        let result = invoke_external_engine(&EngineInvocation {
            bin: bin.to_owned(),
            args: args.to_vec(),
            env: env.clone(),
            workdir: None,
            // **L'INGRESSO VUOTO E CHIUSO, CIOÈ `< /dev/null`.** Un motore che
            // si mettesse ad aspettare qualcosa dall'ingresso appenderebbe il
            // controllo di tutti gli altri: è la trappola già pagata su `codex
            // exec`, e costa un carattere evitarla.
            stdin: Some(Vec::new()),
            timeout: DRY_PROBE_TIMEOUT,
        });
        match result {
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. }
            | EngineResult::WaitingForAPerson { stdout, stderr } => {
                DryRun::Answered { stdout, stderr }
            }
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "nessuna risposta entro {} secondi",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Chiede a `bin`, dentro la casa che `env` dichiara, se è autenticato.
///
/// **NON COSTA NIENTE E NON CHIAMA NESSUN FORNITORE.** Misurato il 01/09/2026:
/// `codex login status` e `claude auth status` leggono un file locale e
/// rispondono. Sono l'unico modo di sapere la cosa senza andare a guardare il
/// disco al posto del motore.
pub fn probe_login_status(
    probe: &dyn LoginProbe,
    bin: &str,
    env: &BTreeMap<String, String>,
    recipe: &LoginRecipe,
) -> LoginVerdict {
    // Un descrittore che non dichiara non fa partire nessun processo: chiedere
    // per poi non saper leggere la risposta sarebbe tempo speso per niente.
    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }
    match probe.ask(bin, &recipe.args, env) {
        DryRun::Answered { stdout, stderr } => judge_login_status(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => LoginVerdict::NoAnswer { why },
    }
}
