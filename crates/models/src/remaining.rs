//! Quanto resta della quota di **una persona**, letto invece che chiesto.
//!
//! **PERCHÉ ESISTE.** Un passo consegnato a un agente vivo dichiara il proprio
//! consumo con `sailor step close --turns`, e un agente non sa contare ciò che
//! il suo harness consuma per lui: nell'A/B del 31/08/2026 ne ha dichiarati 33
//! su 75 veri, il 44%. La cura non è chiedere meglio — è **leggere**. Questo
//! modulo è la prima metà di quella lettura: il canale che dice, senza spendere
//! niente, quanta quota una persona ha già consumato e quando la finestra si
//! azzera.
//!
//! ─────────────────────────────────────────────────────────────────────────
//! **NON È IL COSTO DI UNA CORSA, E CONFONDERLE SAREBBE PEGGIO CHE NON AVERLA.**
//!
//! Quello che si legge qui è la quota **della persona**, su **tutte** le sue
//! sessioni: la corsa di Sailor, il terminale aperto accanto, l'editor, un
//! lavoro di ieri che ricade nella stessa finestra di sette giorni. Fra due
//! istanti si può ricavare *quanta quota è passata*, mai *quanta ne ha
//! consumata una corsa*, perché non c'è modo di sapere chi altro stava
//! scrivendo in mezzo.
//!
//! Un numero preso da qui e scritto accanto a un passo diventerebbe una misura
//! con la faccia giusta e il significato sbagliato — cioè il modo in cui il
//! guasto 37 è nato, non la sua cura. Il posto giusto di questa lettura è
//! accanto alla domanda «posso lanciarne un'altra?», che è una domanda sulla
//! persona.
//! ─────────────────────────────────────────────────────────────────────────
//!
//! **DUE METÀ, E UNA SOLA HA PROVE.** La lettura di un corpo è pura e si prova
//! su un campione scritto a mano (`tests/fixtures/oauth-usage-sample.json`); il
//! gesto che va sulla rete non ha prove, per la stessa ragione di
//! [`crate::fetch`]: una prova che chiama la rete è rossa quando cade la linea,
//! non quando sbaglia il codice.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// L'identificativo del motore che questa lettura riguarda: lo stesso `id` con
/// cui il catalogo dei descrittori lo nomina, così chi legge un `Remaining` può
/// risalire a chi dichiara di saperlo dire.
pub const CLAUDE_CODE: &str = "claude-code";

/// Dove Claude Code tiene le credenziali della persona. Sotto la sua casa, non
/// sotto quella di Sailor: è roba sua, e questo modulo la legge e basta.
const CLAUDE_CREDENTIALS: &str = ".claude/.credentials.json";

/// L'indirizzo che risponde con le finestre di quota.
///
/// **È UN CANALE BETA E VERSIONATO** — l'intestazione `anthropic-beta` porta una
/// data — quindi può smettere di rispondere senza che niente qui cambi. Per
/// questo l'assenza di una lettura non è mai un errore di chi la chiede: è una
/// lettura che non c'è, e chi la voleva continua a funzionare senza.
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// La versione del canale, dichiarata come il fornitore la vuole.
const BETA_HEADER: &str = "anthropic-beta: oauth-2025-04-20";

/// Quanto di una finestra di quota è già andato, e quando quella finestra si
/// azzera.
///
/// **`used_fraction` È UNA FRAZIONE, NON UNA PERCENTUALE.** Il fornitore
/// risponde `50.0` per «metà»; qui diventa `0.5`. Il motivo è che questo numero
/// finirà accanto ad altri rapporti — quote di altri motori, frazioni di un
/// tetto di spesa — e due unità che si assomigliano nello stesso posto si
/// sommano per sbaglio una volta sola, ma quella volta nessuno se ne accorge.
#[derive(Debug, Clone, PartialEq)]
pub struct Remaining {
    /// Di chi è questa quota: l'`id` del descrittore del motore.
    pub engine: String,
    /// Quale finestra: `five_hour`, `seven_day`, o un nome che questa versione
    /// non conosce. **Non è un insieme chiuso**, e non deve diventarlo: il
    /// fornitore ne ha aggiunte mentre questo file veniva scritto.
    pub unit: String,
    /// Quanto è già consumato, da `0.0` a `1.0`.
    pub used_fraction: f64,
    /// Quando la finestra riparte, nella forma in cui il fornitore lo dice.
    ///
    /// **RESTA UN TESTO, E NON È PIGRIZIA.** È il guasto 14: nessuno legge
    /// «si azzera alle 7» per riprovare a quell'ora, e non si legge apposta —
    /// un istante ricavato da una forma vista poche volte è un dato inventato
    /// con la faccia di una misura. Qui la forma è ISO e sarebbe convertibile,
    /// ma finché nessuno aspetta quell'ora convertirla è lavoro che si può
    /// solo sbagliare.
    pub resets_at: Option<String>,
    /// Quando l'abbiamo guardata. Serve perché una quota invecchia: un valore
    /// senza l'istante in cui è stato letto non si distingue da uno di ieri.
    pub observed_at: i64,
}

/// Perché una lettura non c'è.
///
/// **NESSUNA DI QUESTE FORME PORTA IL GETTONE**, ed è il motivo per cui sono
/// scritte a mano invece di avvolgere l'errore di sotto: un `Display` generico
/// che riportasse la riga di comando o il corpo di una risposta è il modo in cui
/// un segreto finisce in un registro, e da un registro non si toglie più.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemainingError {
    /// Il file delle credenziali non c'è: quel motore non è autenticato qui.
    NoCredentials(PathBuf),
    /// Il file c'è e non si legge, o non è JSON.
    CredentialsUnreadable(String),
    /// Il file è JSON e non porta la chiave del gettone. Questa versione di
    /// quel motore tiene le credenziali da un'altra parte.
    NoToken,
    /// `curl` non è partito, o non ha risposto.
    Unreachable(String),
    /// Ha risposto, e ha detto di no. Porta la parola del fornitore, che dice
    /// **cosa fare** — «il gettone è stato revocato» si cura autenticandosi di
    /// nuovo, e nessuna frase scritta qui lo saprebbe dire meglio.
    ///
    /// **NON PORTA IL GETTONE**: il corpo di un rifiuto non l'ha mai visto, e
    /// qui si copia solo il campo `message`, mai la richiesta.
    Refused(String),
    /// Ha risposto qualcosa che non è il JSON atteso: il canale è beta, e
    /// questo è il modo in cui si romperà.
    NotUnderstood,
}

impl fmt::Display for RemainingError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemainingError::NoCredentials(path) => {
                write!(out, "nessuna credenziale in {}", path.display())
            }
            RemainingError::CredentialsUnreadable(why) => {
                write!(out, "le credenziali non si leggono: {why}")
            }
            RemainingError::NoToken => {
                write!(out, "le credenziali non portano la chiave del gettone")
            }
            RemainingError::Unreachable(why) => write!(out, "il canale non risponde: {why}"),
            RemainingError::Refused(said) => write!(out, "il motore ha rifiutato: {said}"),
            RemainingError::NotUnderstood => write!(
                out,
                "la risposta non ha la forma attesa: il canale è beta e versionato, \
                 e può cambiare senza avviso"
            ),
        }
    }
}

/// Il gettone di accesso, in una forma che **non si può stampare per sbaglio**.
///
/// **PERCHÉ UN TIPO E NON UNA `String`.** Una stringa finisce in un `{:?}` di
/// una struttura che la contiene, in un messaggio d'errore scritto di fretta,
/// in un `dbg!` lasciato indietro — e chi la scrive non se ne accorge, perché
/// nessuno di quei gesti ha l'aria di stampare un segreto. Qui `Debug` è scritto
/// a mano, non c'è `Display`, non c'è modo pubblico di tirar fuori il testo, e
/// l'unico posto che lo tocca è la configurazione che va sull'ingresso di
/// `curl`. Un difetto del genere non si previene con l'attenzione: si previene
/// togliendo il gesto.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("Token(nascosto)")
    }
}

impl Token {
    /// Il gettone dentro il file delle credenziali di Claude Code.
    ///
    /// La forma è quella misurata su questa macchina il 01/09/2026:
    /// `{"claudeAiOauth": {"accessToken": "…"}}`. Una chiave che non c'è è
    /// [`RemainingError::NoToken`] e non un panico: un file di credenziali è di
    /// qualcun altro e cambia quando quel qualcun altro lo decide.
    pub fn from_credentials(text: &str) -> Result<Token, RemainingError> {
        let parsed: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| RemainingError::CredentialsUnreadable(error.to_string()))?;
        parsed
            .get("claudeAiOauth")
            .and_then(|oauth| oauth.get("accessToken"))
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.is_empty())
            .map(|token| Token(token.to_owned()))
            .ok_or(RemainingError::NoToken)
    }

    /// La configurazione che `curl` legge **dal proprio ingresso**.
    ///
    /// **IL GETTONE NON PASSA DAGLI ARGOMENTI, ED È IL PUNTO DI QUESTA
    /// FUNZIONE.** `curl -H "Authorization: Bearer …"` mette il segreto nella
    /// riga di comando del processo, e la riga di comando di un processo la
    /// legge chiunque sulla macchina con un `ps`. Con `-K -` la stessa cosa
    /// viaggia su una pipe che esiste solo fra questi due processi.
    fn curl_config(&self) -> String {
        format!(
            "url = \"{USAGE_URL}\"\n\
             header = \"Authorization: Bearer {}\"\n\
             header = \"{BETA_HEADER}\"\n\
             silent\n\
             show-error\n\
             max-time = 30\n",
            self.0
        )
    }
}

/// Le finestre di quota dentro una risposta di `/api/oauth/usage`.
///
/// **NON CONOSCE NESSUN NOME DI FINESTRA, E NON DEVE.** Prende ogni chiave di
/// primo livello il cui valore è un oggetto con dentro un `utilization`
/// numerico. La risposta vera del 01/09/2026 ne portava quattordici, di cui due
/// piene, una a zero con un nome che non compare in nessuna documentazione, e
/// undici nulle: un elenco scritto in un `match` avrebbe perso la terza il
/// giorno che è comparsa, e nessuno se ne sarebbe accorto perché una finestra
/// che manca non è rossa da nessuna parte.
///
/// **CIÒ CHE NON È UNA MISURA NON DIVENTA UNO ZERO.** Una finestra `null`, o un
/// oggetto con `utilization` a `null`, o senza quel campo, esce dall'elenco
/// invece di entrarci a zero. Uno zero in mezzo alle quote si legge «hai tutto
/// libero», che è la direzione rassicurante e sbagliata.
pub fn from_claude_oauth_usage(
    body: &str,
    observed_at: i64,
) -> Result<Vec<Remaining>, RemainingError> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| RemainingError::NotUnderstood)?;
    let windows = parsed.as_object().ok_or(RemainingError::NotUnderstood)?;

    // **UN RIFIUTO SI RICONOSCE PRIMA DI CONTARE LE FINESTRE.** Il rifiuto di
    // questo fornitore è un JSON valido con dentro un oggetto e nessun
    // `utilization`: scorrerlo cercando quote dà un elenco vuoto, cioè «non
    // risulta nessun consumo» — la stessa frase che direbbe una persona che non
    // ha ancora lavorato. Visto per davvero eseguendo `sailor remaining` il
    // 01/09/2026, col gettone su disco ruotato sotto i piedi del lettore.
    //
    // **SI GUARDA `error.message`, NON L'INVOLUCRO.** Le due forme misurate lo
    // stesso giorno, a venti minuti di distanza, differiscono proprio
    // sull'involucro: la revoca porta un `"type": "error"` di primo livello, il
    // limite di frequenza no. Riconoscere l'involucro avrebbe lasciato passare
    // come «zero consumo» esattamente la risposta che tocca a chi interroga
    // spesso, cioè a un controllo automatico.
    if let Some(said) = windows
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        return Err(RemainingError::Refused(said.to_owned()));
    }

    let mut found = Vec::new();
    for (unit, window) in windows {
        let Some(fields) = window.as_object() else {
            continue;
        };
        let Some(percent) = fields
            .get("utilization")
            .and_then(serde_json::Value::as_f64)
        else {
            continue;
        };
        found.push(Remaining {
            engine: CLAUDE_CODE.to_owned(),
            unit: unit.clone(),
            // Il fornitore dice «50.0» per metà. Vedi il commento su
            // `used_fraction`: qui l'unità cambia una volta sola, in un posto
            // solo.
            used_fraction: percent / 100.0,
            resets_at: fields
                .get("resets_at")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            observed_at,
        });
    }
    Ok(found)
}

/// Legge davvero la quota di Claude Code su questa macchina.
///
/// **SOLO LETTURA, E NESSUN COSTO.** Non invoca nessun motore e non consuma
/// niente: chiede a un indirizzo quanto è già stato consumato. È la ragione per
/// cui si può chiamare in un controllo che gira spesso.
///
/// `home` è la casa della persona — si passa invece di leggerla qui dentro così
/// chi prova questo modulo non deve avere le credenziali vere di nessuno.
pub fn read_from_claude(home: &Path, observed_at: i64) -> Result<Vec<Remaining>, RemainingError> {
    let path = home.join(CLAUDE_CREDENTIALS);
    if !path.exists() {
        return Err(RemainingError::NoCredentials(path));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| RemainingError::CredentialsUnreadable(error.to_string()))?;
    let token = Token::from_credentials(&text)?;
    let body = ask_curl(&token.curl_config())?;
    from_claude_oauth_usage(&body, observed_at)
}

/// `curl` come processo, con la configurazione sull'ingresso.
///
/// Stessa strada di [`crate::fetch`] — un processo invece di una libreria HTTP,
/// per non tirarsi dietro una crate che il resto del workspace non ha.
fn ask_curl(config: &str) -> Result<String, RemainingError> {
    use std::io::Write;

    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| RemainingError::Unreachable("curl non ha aperto l'ingresso".to_owned()))?
        .write_all(config.as_bytes())
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    let done = child
        .wait_with_output()
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    if !done.status.success() {
        // **LO STANDARD ERRORE DI `curl` SI RIPORTA, LA CONFIGURAZIONE NO.**
        // La prima non ha mai visto il gettone; la seconda lo contiene.
        return Err(RemainingError::Unreachable(
            String::from_utf8_lossy(&done.stderr).trim().to_owned(),
        ));
    }
    String::from_utf8(done.stdout).map_err(|_| RemainingError::NotUnderstood)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/oauth-usage-sample.json");

    fn window<'a>(found: &'a [Remaining], unit: &str) -> Option<&'a Remaining> {
        found.iter().find(|entry| entry.unit == unit)
    }

    /// **LE DUE FINESTRE PIENE SI LEGGONO, E LA PERCENTUALE DIVENTA FRAZIONE.**
    /// Il `50.0` del fornitore vale mezza finestra: chi lo riportasse com'è
    /// darebbe una quota consumata cinquanta volte, cioè un numero che non
    /// significa niente in nessuna unità.
    #[test]
    fn the_two_full_windows_are_read_as_fractions() {
        let found = from_claude_oauth_usage(SAMPLE, 1_000).expect("il campione si legge");

        let five_hour = window(&found, "five_hour").expect("la finestra di cinque ore c'è");
        assert_eq!(
            five_hour.used_fraction, 0.5,
            "50.0 per cento è mezza finestra"
        );
        assert_eq!(five_hour.engine, CLAUDE_CODE);
        assert_eq!(
            five_hour.resets_at.as_deref(),
            Some("2026-09-01T03:29:59.801054+00:00")
        );
        assert_eq!(
            five_hour.observed_at, 1_000,
            "una quota senza istante invecchia in silenzio"
        );

        assert_eq!(
            window(&found, "seven_day")
                .expect("e quella di sette giorni")
                .used_fraction,
            0.32
        );
    }

    /// **UNA FINESTRA CHE QUESTA VERSIONE NON CONOSCE ESCE LO STESSO.** La
    /// risposta vera del 01/09/2026 ne portava una che non compare in nessuna
    /// documentazione: un elenco di nomi scritto nel codice l'avrebbe persa, e
    /// una quota persa non è rossa da nessuna parte.
    #[test]
    fn a_window_this_version_never_heard_of_is_reported_anyway() {
        let found = from_claude_oauth_usage(SAMPLE, 0).expect("il campione si legge");
        let unknown = window(&found, "nimbus_quill").expect("la finestra ignota c'è lo stesso");
        assert_eq!(unknown.used_fraction, 0.075);
    }

    /// **CIÒ CHE NON È UNA MISURA NON ENTRA COME ZERO.** Sono quattro forme
    /// diverse di «qui non c'è un numero», e tutte e quattro devono sparire
    /// invece di dire «hai tutto libero».
    #[test]
    fn what_is_not_a_measure_never_becomes_a_zero() {
        let found = from_claude_oauth_usage(SAMPLE, 0).expect("il campione si legge");
        let units: Vec<&str> = found.iter().map(|entry| entry.unit.as_str()).collect();

        for absent in ["seven_day_opus", "extra_usage", "spend", "limits"] {
            assert!(
                !units.contains(&absent),
                "«{absent}» non dichiara un consumo: non deve comparire fra le quote. Trovate: {units:?}"
            );
        }
        assert_eq!(
            units.len(),
            4,
            "le sole quattro con un `utilization` numerico: {units:?}"
        );
    }

    /// Una finestra reale e senza istante di azzeramento resta nell'elenco: il
    /// consumo si conosce anche quando non si sa quando riparte.
    #[test]
    fn a_window_without_a_reset_keeps_its_measure() {
        let found = from_claude_oauth_usage(SAMPLE, 0).expect("il campione si legge");
        let no_reset = window(&found, "no_reset").expect("c'è");
        assert_eq!(no_reset.used_fraction, 0.0);
        assert_eq!(no_reset.resets_at, None, "mai un istante inventato");
    }

    /// **UN RIFIUTO NON È UNA RISPOSTA CON ZERO FINESTRE**, ed è il difetto che
    /// questo modulo ha avuto per un'ora il 01/09/2026. Il corpo è quello vero,
    /// arrivato eseguendo `sailor remaining` su questa macchina: il gettone su
    /// disco era stato ruotato sotto i piedi del lettore, l'indirizzo ha
    /// risposto 401 con un JSON valido, e il lettore ha riportato «nessuna
    /// finestra di quota dichiarata». Cioè: interrogato sul consumo, ha detto
    /// che non ne risulta. Chi legge quella frase prima di lanciare qualcosa
    /// legge un via libera.
    #[test]
    fn a_refusal_is_a_refusal_and_never_an_empty_measure() {
        let refused = r#"{"type":"error","error":{"type":"authentication_error",
            "message":"OAuth access token has been revoked."},"request_id":null}"#;

        let said = from_claude_oauth_usage(refused, 0).expect_err("è un rifiuto, non una misura");
        assert_eq!(
            said,
            RemainingError::Refused("OAuth access token has been revoked.".to_owned()),
            "la parola del fornitore si riporta: dice cosa fare, cioè autenticarsi di nuovo"
        );
    }

    /// **IL FORNITORE RIFIUTA IN PIÙ DI UNA FORMA, E LE HO VISTE TUTTE E DUE IN
    /// VENTI MINUTI.** La prima porta un `type` di primo livello, questa no —
    /// è solo `{"error": {…}}`. Un controllo scritto sulla prima forma lasciava
    /// passare la seconda **come elenco vuoto**, cioè come «non risulta nessun
    /// consumo», e la seconda è quella che arriva a chi interroga spesso: è la
    /// risposta a chi ha chiesto troppo. Il riconoscimento sta quindi sulla
    /// parte che le due hanno in comune — un `error` con dentro un `message` —
    /// e non sull'involucro.
    #[test]
    fn a_refusal_without_the_outer_type_is_still_a_refusal() {
        let limited = r#"{"error":{"type":"rate_limit_error",
            "message":"Rate limited. Please try again later."}}"#;

        assert_eq!(
            from_claude_oauth_usage(limited, 0),
            Err(RemainingError::Refused(
                "Rate limited. Please try again later.".to_owned()
            )),
            "chi legge deve sapere che è stato rifiutato, non che non ha consumato niente"
        );
    }

    /// **E UNA RISPOSTA VERA SENZA FINESTRE RESTA UNA RISPOSTA VERA.** Senza
    /// questa metà basterebbe dichiarare rifiuto ogni elenco vuoto, e i due casi
    /// tornerebbero indistinguibili dall'altra parte.
    #[test]
    fn a_usage_answer_with_every_window_null_is_not_a_refusal() {
        let empty = r#"{"five_hour":null,"seven_day":null,"member_dashboard_available":false}"#;
        assert_eq!(from_claude_oauth_usage(empty, 0), Ok(vec![]));
    }

    /// Il canale è beta: il modo in cui si romperà è rispondendo altro.
    #[test]
    fn a_body_that_is_not_the_expected_shape_is_a_declared_failure() {
        assert_eq!(
            from_claude_oauth_usage("<html>502</html>", 0),
            Err(RemainingError::NotUnderstood)
        );
        assert_eq!(
            from_claude_oauth_usage("[1, 2, 3]", 0),
            Err(RemainingError::NotUnderstood)
        );
    }

    // ── il gettone ───────────────────────────────────────────────────────

    /// Un testo che ha la forma del file vero, con dentro un finto segreto
    /// riconoscibile: se compare da qualche parte, lo si vede subito.
    const A_SECRET: &str = "questo-non-deve-comparire-da-nessuna-parte";

    fn credentials_with(token: &str) -> String {
        format!(r#"{{"mcpOAuth": {{}}, "claudeAiOauth": {{"accessToken": "{token}"}}}}"#)
    }

    #[test]
    fn the_token_is_taken_from_the_key_the_file_really_uses() {
        assert!(Token::from_credentials(&credentials_with(A_SECRET)).is_ok());
    }

    /// **IL GETTONE NON SI STAMPA, E QUESTA È LA PROVA CHE LO TIENE.** Un
    /// `#[derive(Debug)]` al posto di quello scritto a mano rimette il difetto,
    /// e questa riga diventa rossa. È l'unico modo di provare un'assenza: si
    /// prova il gesto che la violerebbe.
    #[test]
    fn no_way_of_printing_a_token_shows_it() {
        let token = Token::from_credentials(&credentials_with(A_SECRET)).expect("c'è");
        let printed = format!("{token:?}");
        assert!(
            !printed.contains(A_SECRET),
            "il gettone è finito in una stampa: {printed}"
        );
        assert_eq!(printed, "Token(nascosto)");
    }

    /// **E NEMMENO UN MESSAGGIO D'ERRORE LO PORTA.** Un errore si scrive di
    /// fretta e finisce in un registro, dove resta.
    #[test]
    fn no_failure_message_carries_the_token() {
        let broken = format!("{{\"claudeAiOauth\": {{\"accessToken\": \"{A_SECRET}\"}}");
        let refused = Token::from_credentials(&broken).expect_err("il JSON è troncato");
        let said = format!("{refused} / {refused:?}");
        assert!(
            !said.contains(A_SECRET),
            "il gettone è finito nell'errore: {said}"
        );

        let no_key = Token::from_credentials(r#"{"claudeAiOauth": {}}"#).expect_err("manca");
        assert_eq!(no_key, RemainingError::NoToken);
    }

    /// La configurazione che va sull'ingresso di `curl` porta il gettone —
    /// deve — e nessun **argomento** lo porta. È la differenza fra un segreto
    /// su una pipe e un segreto leggibile con `ps`.
    #[test]
    fn the_secret_travels_on_the_pipe_and_never_in_an_argument() {
        let token = Token::from_credentials(&credentials_with(A_SECRET)).expect("c'è");
        let config = token.curl_config();
        assert!(
            config.contains(A_SECRET),
            "senza il gettone la richiesta non è autenticata"
        );
        assert!(config.contains(USAGE_URL));
        assert!(
            config.contains(BETA_HEADER),
            "il canale è versionato: la versione si dichiara"
        );
    }

    /// Un motore non autenticato qui non è un guasto: è una lettura che non
    /// c'è, e chi la voleva continua senza.
    #[test]
    fn a_machine_without_those_credentials_says_so_instead_of_failing_loudly() {
        let nowhere = PathBuf::from("/questa/casa/non/esiste");
        let refused = read_from_claude(&nowhere, 0).expect_err("non c'è niente da leggere");
        assert!(matches!(refused, RemainingError::NoCredentials(_)));
    }
}
