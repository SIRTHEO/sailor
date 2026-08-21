//! `SessionStart`: registra la sessione viva per la staffetta, e la fa ripartire.
//!
//! Porta di `skills/hooks/register-session.py`. I due compiti restano quelli:
//! 1. scrivere `state/sessioni-vive/<sess>.json` con la tupla che il demone
//!    della staffetta non può dedurre da solo — manico del terminale, tab,
//!    worktree, trascrizione, cartella. È il ponte sessione↔terminale: senza,
//!    si sa che una sessione ha consegnato ma non quale terminale chiudere;
//! 2. se la staffetta ha appena rigenerato questo worktree, consumare il
//!    segnale `state/riprendi-da/<chiave>.txt` e iniettarne il mandato su
//!    stdout, così la sessione nuova riprende invece di aprire a vuoto.
//!
//! IL MANICO È UNA FOTOGRAFIA, NON UN'IDENTITÀ. `ORCA_TERMINAL_HANDLE` vale per
//! l'incarnazione del terminale che c'era all'avvio; dopo un riattacco Orca ne
//! conia un altro. La chiave che sopravvive è `ORCA_TAB_ID`, e basta una delle
//! due per registrare: pretendere il manico lasciava fuori proprio le sessioni
//! più facili da ritrovare.
//!
//! `state_key` NON SI RISCRIVE QUI. Sta in `guards::handoff`, ed è la stessa
//! che usa chi il segnale lo scrive: due copie della stessa trasformazione
//! divergono alla prima correzione, e chi scrive con una chiave e legge con
//! l'altra lascia il successore orfano — che è il difetto già visto il 17/08.
//!
//! FAIL-OPEN OVUNQUE: qualunque errore → stdout vuoto, uscita 0. Un gancio che
//! rompe l'avvio di una sessione fa più danno del problema che risolve.

use guards::handoff::state_key;
use hook_io::journal::{self, Field};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Un segnale di ripresa più vecchio di dieci minuti è stantio.
const FRESH_SEC: u64 = 600;

/// Le famiglie di marcatori che il presidio della consegna scrive per una
/// sessione. L'elenco è CHIUSO e sta in un posto solo: quando ne esistevano due
/// copie, la famiglia nata dopo era presente in una sola.
pub(crate) const MARKER_FAMILIES: &[&str] = &[
    "consegna-fatta",
    "consegna-fatta-ripartenze",
    "consegna-blocchi",
    "consegna-stop",
    "consegna-avvisata",
    "consegna-misura",
    "consegna-ripartenze",
    // AGGIUNTA IL 18/08/2026, e il commento qui sopra la contava già fra i 176
    // marcatori rimasti sul disco: era stata guardata e non aggiunta.
    "consegna-volontaria",
];

/// `successore-di-` fa eccezione: porta l'identificativo **intero**, non i primi
/// otto come tutti gli altri. Chi cancella per prefisso corto lo manca sempre, e
/// lo manca in silenzio.
const FULL_ID_FAMILIES: &[&str] = &["successore-di"];

/// E questa non porta l'id affatto, ma la sua impronta:
/// `successore-armato-<sha1(session)[..16]>`. Non essendo il nome leggibile,
/// nessuno aveva pensato a toglierla — 20 file il 18/08/2026, e provato lo
/// stesso giorno che due corrispondono all'impronta di sessioni ancora nominate
/// sul disco. Derivabile significa cancellabile.
const FINGERPRINT_FAMILIES: &[&str] = &["successore-armato"];

fn state_dir() -> PathBuf {
    // La HOME si legge dall'ambiente come nell'originale (`Path.home()`), così
    // il confronto di equivalenza può spostarla senza toccare nessuno dei due.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".into());
    PathBuf::from(home).join(".claude").join("state")
}

fn live_dir() -> PathBuf {
    state_dir().join("sessioni-vive")
}

fn resume_dir() -> PathBuf {
    state_dir().join("riprendi-da")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Le chiavi d'ambiente che mancano per poter registrare. Elenco vuoto: si
/// registra.
///
/// Sta a parte perché è la condizione che faceva sparire sessioni vive senza
/// dirlo, e una condizione che si può provare solo dal vivo non si prova. Le
/// due chiavi del pannello si contano insieme: ne basta una, ed è già scritto
/// in testa al modulo perché.
pub(crate) fn missing_panel_keys(worktree: &str, handle: &str, tab: &str) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if worktree.trim().is_empty() {
        missing.push("ORCA_WORKTREE_ID");
    }
    if handle.trim().is_empty() && tab.trim().is_empty() {
        missing.push("ORCA_TAB_ID|ORCA_TERMINAL_HANDLE");
    }
    missing
}

/// Se questa sessione gira dentro Orca. La porta del gancio di Orca è l'unico
/// segno che c'è **prima** delle chiavi che si stanno cercando: usare una di
/// quelle per dire «siamo dentro Orca» renderebbe la condizione circolare, e il
/// caso da denunciare — dentro Orca, chiavi assenti — non uscirebbe mai.
fn inside_orca() -> bool {
    !env("ORCA_AGENT_HOOK_PORT").trim().is_empty()
}

fn record_session(data: &serde_json::Value) {
    let full = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sess: String = full.chars().take(8).collect();
    if sess.is_empty() {
        return;
    }
    let handle = env("ORCA_TERMINAL_HANDLE");
    let worktree = env("ORCA_WORKTREE_ID");
    let tab = env("ORCA_TAB_ID");
    // Basta UNA delle due chiavi; fuori da Orca non c'è niente da rigenerare.
    //
    // QUI SI USCIVA MUTI, e il registro delle sessioni vive ne portava il segno:
    // la notte del 21/08/2026 conteneva quattro record contro sei sessioni vere,
    // e chi conta da lì chiude un pannello vivo credendolo morto. Dentro Orca la
    // mancanza è un guasto e va detta; fuori da Orca è il caso normale e resta
    // muta, altrimenti ogni sessione aperta da un terminale qualunque scriverebbe
    // una riga inutile.
    let missing = missing_panel_keys(&worktree, &handle, &tab);
    if !missing.is_empty() {
        if inside_orca() {
            journal::record(
                "register-session",
                "salta",
                "chiavi-del-pannello-mancanti",
                &[
                    ("session", Field::Text(sess.clone())),
                    ("mancanti", Field::Text(missing.join(" "))),
                    (
                        "cwd",
                        Field::Text(
                            data.get("cwd")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ),
                    ),
                ],
            );
        }
        return;
    }
    let cwd = match data.get("cwd").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    // L'ordine delle chiavi è quello del dizionario Python, e conta: il file è
    // confrontato byte per byte dal test di equivalenza. `serde_json` lo tiene
    // grazie a `preserve_order`.
    let record = serde_json::json!({
        "session_id": full,
        "terminal_handle": handle,
        "worktree_id": worktree,
        "tab_id": tab,
        "transcript_path": data.get("transcript_path").and_then(|v| v.as_str()).unwrap_or(""),
        "cwd": cwd,
        "source": data.get("source").and_then(|v| v.as_str()).unwrap_or(""),
        "updated_at": now(),
    });
    let dir = live_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    // `json.dumps(..., ensure_ascii=False)`: separatori con lo spazio e unicode
    // vero, non `\uXXXX`.
    let _ = fs::write(
        dir.join(format!("{sess}.json")),
        hook_io::python_json::dumps_unicode(&record),
    );
}

/// I file che questa sessione ha scritto, il proprio record compreso. Solo i suoi.
fn own_markers(sess: &str, full_id: &str) -> Vec<PathBuf> {
    if sess.is_empty() {
        return Vec::new();
    }
    let state = state_dir();
    let mut paths = vec![live_dir().join(format!("{sess}.json"))];
    for f in MARKER_FAMILIES {
        paths.push(state.join(format!("{f}-{sess}")));
    }
    if !full_id.is_empty() {
        let safe: String = full_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(64)
            .collect();
        for f in FULL_ID_FAMILIES {
            paths.push(state.join(format!("{f}-{safe}")));
        }
        // L'impronta si calcola sull'identificativo INTERO e non filtrato,
        // perché è esattamente ciò che il gancio passa a `hashlib.sha1`: con la
        // forma corta, o con quella ripulita, il nome uscirebbe diverso e non si
        // cancellerebbe niente. `guards::sha1` esiste per dare le stesse cifre.
        let digest = guards::duplication::sha1(full_id.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        for f in FINGERPRINT_FAMILIES {
            paths.push(state.join(format!("{f}-{}", &hex[..16])));
        }
    }
    paths
}

/// `SessionEnd`: la sessione cancella il proprio record, e nessuno indovina.
///
/// Chi ha scritto un marcatore sa quando scade, e lo butta lui. Un raccoglitore
/// che passa dopo dovrebbe indovinare un'età massima, cioè scegliere fra
/// cancellare troppo presto e non cancellare mai.
fn forget_session(data: &serde_json::Value) {
    let full = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sess: String = full.chars().take(8).collect();
    if sess.is_empty() {
        return;
    }
    for path in own_markers(&sess, &full) {
        let _ = fs::remove_file(path);
    }
}

/// Il mandato lasciato dalla staffetta, se c'è ed è fresco. Lo consuma.
fn resume_message() -> String {
    let worktree = env("ORCA_WORKTREE_ID");
    if worktree.is_empty() {
        return String::new();
    }
    let signal = resume_dir().join(format!("{}.txt", state_key(&worktree)));
    let Ok(meta) = fs::metadata(&signal) else {
        return String::new();
    };
    let age = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| now().saturating_sub(d.as_secs()))
        .unwrap_or(u64::MAX);
    if age > FRESH_SEC {
        let _ = fs::remove_file(&signal); // stantio: si butta senza agire
        return String::new();
    }
    let Ok(body) = fs::read_to_string(&signal) else {
        return String::new();
    };
    // IL SEGNALE È JSON, e il formato vecchio era il solo percorso su una riga.
    //
    // Si distinguono da soli: un percorso non è JSON valido. Il formato a righe
    // etichettate, provato per un'ora il 19/08/2026, non ha mai raggiunto la
    // produzione — spezzava i mandati su più righe e chi rileggeva riattaccava
    // le righe senza separatore.
    let (path, punto, mandato) = match serde_json::from_str::<serde_json::Value>(body.trim()) {
        Ok(d) => {
            let campo = |k: &str| {
                d.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string()
            };
            // IL SEGNALE È DI CHI È INTESTATO. La chiave del file è l'albero, e
            // su un albero possono vivere due sessioni: senza questo controllo
            // la seconda che riparte si prende il punto di ripresa e il `/loop`
            // della prima. Si confronta la tab, che `/clear` non cambia e che
            // non invecchia come l'handle.
            //
            // Se il segnale non dice a chi va, o se l'ambiente non dice chi
            // siamo, si consuma comunque: è la forma che aveva prima, e negarsi
            // il mandato per un dubbio lascerebbe la sessione senza incarico.
            let destinatario = campo("tab");
            let io = env("ORCA_TAB_ID");
            if !destinatario.is_empty() && !io.is_empty() && destinatario != io {
                return String::new(); // non è per noi: resta a chi aspetta
            }
            (campo("handoff"), campo("punto"), campo("mandato"))
        }
        Err(_) => (body.trim().to_string(), String::new(), String::new()),
    };
    let _ = fs::remove_file(&signal); // consumato: una staffetta, una ripresa
    if path.is_empty() {
        return String::new();
    }
    let base = format!(
        "RIPARTENZA AUTOMATICA (staffetta). La sessione precedente su questo \
worktree ha consegnato ed e' stata rigenerata per non trascinare un contesto \
gonfio. Riprendi da quell'handoff: leggi `{path}` e prosegui il piano gia' \
autorizzato. Non ricominciare da zero, e **non annunciare la ripartenza**: il \
primo messaggio del turno e' gia' il passo successivo del piano."
    );
    if punto.is_empty() && mandato.is_empty() {
        return base;
    }
    let mut coda = String::new();
    if !punto.is_empty() {
        // IL PUNTO PRIMA DEL MANDATO: è la riga che dice cosa fare adesso,
        // mentre il mandato dice a che lavoro appartiene.
        coda.push_str(&format!(
            "\n\nRIPRENDI DA QUI, e' il punto che la sessione uscente ha \
dichiarato chiudendo il suo ultimo turno:\n{punto}"
        ));
    }
    // IL MANDATO NON E' CONTESTO, E' L'INCARICO. Chi arriva qui sta sostituendo
    // una sessione che girava in `/loop`: senza queste righe legge la consegna,
    // dichiara da dove riparte e si ferma — e il loop muore alla prima
    // rigenerazione.
    if !mandato.is_empty() {
        coda.push_str(&format!(
            "\n\nLa sessione uscente stava girando su questo mandato, e tocca \
ora a te: riprendilo cosi' com'e'.\n\n{mandato}"
        ));
    }
    format!("{base}{coda}")
}

/// Siamo a fine sessione? Lo dice l'evento, non un argomento.
///
/// `--fine` restava un argomento da ricopiare in `settings.json`, e nel ramo che
/// esegue davvero non era stato ricopiato: la riga di `SessionEnd` passa
/// l'opzione al ripiego Python e **non** al binario, che esiste sempre. Esito
/// misurato il 18/08/2026: `forget_session` non era mai girata, e sul disco
/// c'erano **176 marcatori** dal 14/08 — 84 `consegna-misura-*`, 72
/// `consegna-ripartenze-*`, 16 `consegna-fatta-*`, 4 `consegna-volontaria-*`.
///
/// Il difetto non e' l'opzione dimenticata, e' che il gancio si affidava a
/// qualcuno che la ricopiasse. L'evento invece arriva sempre e da solo: chi
/// scrive la riga di configurazione non puo' piu' sbagliarla. `--fine` resta
/// riconosciuto per non rompere chi lo passa ancora.
fn is_session_end(data: &serde_json::Value, args: &[String]) -> bool {
    if args.iter().any(|a| a == "--fine") {
        return true;
    }
    data.get("hook_event_name")
        .and_then(|v| v.as_str())
        .is_some_and(|e| e == "SessionEnd")
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0; // stdin non è JSON: si esce muti, come l'originale
    };
    // Prima di qualunque riga di registro, altrimenti esce marcata come prova e
    // chi conta quanto un gancio morde la scarta.
    hook_io::mark_live_from_payload(&data);
    let args: Vec<String> = std::env::args().collect();
    if is_session_end(&data, &args) {
        forget_session(&data);
        return 0;
    }
    record_session(&data);
    let msg = resume_message();
    if !msg.is_empty() {
        println!("{msg}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    fn event(nome: &str, sess: &str) -> serde_json::Value {
        serde_json::json!({ "hook_event_name": nome, "session_id": sess })
    }

    #[test]
    fn the_end_of_a_session_is_read_off_the_event() {
        assert!(is_session_end(&event("SessionEnd", "abc"), &[]));
    }

    #[test]
    fn any_other_event_is_not_the_end() {
        assert!(!is_session_end(&event("SessionStart", "abc"), &[]));
        assert!(!is_session_end(&event("", "abc"), &[]));
        assert!(!is_session_end(&serde_json::json!({ "session_id": "abc" }), &[]));
    }

    #[test]
    fn the_old_flag_still_works_for_whoever_still_passes_it() {
        // La compatibilita' e' una promessa: senza questo caso, toglierla non
        // farebbe cadere niente.
        let args = vec!["claude-hooks".to_string(), "--fine".to_string()];
        assert!(is_session_end(&event("SessionStart", "abc"), &args));
        assert!(is_session_end(&serde_json::json!({}), &args));
    }

    #[test]
    fn a_session_end_sweeps_its_own_markers_and_leaves_the_others() {
        let casa = HomeIsolata::nuova("fine-sessione");
        let state = casa.dir.join(".claude/state");
        std::fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        let miei = ["consegna-misura-11112222", "consegna-fatta-11112222"];
        let altrui = ["consegna-misura-99998888", "consegna-fatta-99998888"];
        for n in miei.iter().chain(altrui.iter()) {
            std::fs::write(state.join(n), "x").unwrap();
        }
        std::fs::write(state.join("sessioni-vive/11112222.json"), "{}").unwrap();

        forget_session(&event("SessionEnd", "11112222-3333-4444-5555-666677778888"));

        for n in miei {
            assert!(!state.join(n).exists(), "{n} doveva sparire");
        }
        for n in altrui {
            assert!(state.join(n).exists(), "{n} non e' suo e doveva restare");
        }
        assert!(!state.join("sessioni-vive/11112222.json").exists());
    }

    /// Un segnale di ripresa fresco, nella forma che scrive la staffetta.
    fn signal_json(casa: &HomeIsolata, handoff: &str, punto: &str, mandato: &str) {
        signal_for(casa, handoff, punto, mandato, "");
    }

    /// Come sopra, intestato a una tab.
    fn signal_for(
        casa: &HomeIsolata,
        handoff: &str,
        punto: &str,
        mandato: &str,
        tab: &str,
    ) {
        let corpo = serde_json::json!({
            "handoff": handoff, "punto": punto, "mandato": mandato, "tab": tab,
        })
        .to_string();
        signal(casa, &corpo);
    }

    /// Un segnale col corpo dato alla lettera, per provare le forme storte.
    fn signal(casa: &HomeIsolata, corpo: &str) {
        std::env::set_var("ORCA_WORKTREE_ID", "repo::_prova");
        let dir = casa.dir.join(".claude/state/riprendi-da");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.txt", state_key("repo::_prova"))), corpo)
            .unwrap();
    }

    #[test]
    fn a_signal_with_only_the_path_stays_valid() {
        // E' la forma che il file ha avuto per giorni: cambiarla senza tenerla
        // buona spegnerebbe la ripresa di ogni albero gia' in attesa.
        let casa = HomeIsolata::nuova("segnale-vecchio");
        signal(&casa, "/percorso/consegna.md");
        let msg = resume_message();
        assert!(msg.contains("/percorso/consegna.md"), "{msg}");
        assert!(!msg.contains("stava girando su questo mandato"), "{msg}");
    }

    #[test]
    fn the_resume_point_reaches_the_successor() {
        // È la differenza fra «leggi questo documento e prosegui» e «riprendi
        // da qui»: il primo chiede di dedurre, il secondo dice.
        let casa = HomeIsolata::nuova("segnale-punto");
        signal_json(&casa, "/percorso/consegna.md", "la staffetta via /clear", "");
        let msg = resume_message();
        assert!(msg.contains("RIPRENDI DA QUI"), "{msg}");
        assert!(msg.contains("la staffetta via /clear"), "{msg}");
    }

    #[test]
    fn the_resume_point_comes_before_the_mandate() {
        let casa = HomeIsolata::nuova("segnale-ordine");
        signal_json(
            &casa,
            "/percorso/consegna.md",
            "il gate della lingua",
            "/loop Sistema tutto",
        );
        let msg = resume_message();
        let p = msg.find("il gate della lingua").expect("punto assente");
        let m = msg.find("/loop Sistema tutto").expect("mandato assente");
        assert!(p < m, "il mandato precede il punto:\n{msg}");
    }

    #[test]
    fn the_loop_mandate_reaches_the_successor() {
        // Senza queste righe il successore legge la consegna, dichiara da dove
        // riparte e si ferma: e' come e' morto il loop del 19/08/2026.
        let casa = HomeIsolata::nuova("segnale-mandato");
        signal_json(&casa, "/percorso/consegna.md", "", "/loop Sistemare la configurazione");
        let msg = resume_message();
        assert!(msg.contains("/percorso/consegna.md"), "{msg}");
        assert!(msg.contains("/loop Sistemare la configurazione"), "{msg}");
        assert!(msg.contains("tocca"), "{msg}");
    }

    #[test]
    fn a_signal_addressed_to_someone_else_is_not_consumed() {
        // Due sessioni sullo stesso albero sono un caso normale, e da quando
        // ogni rigenerazione manda un `/clear` la seconda che riparte
        // troverebbe il segnale della prima: si prenderebbe il suo punto di
        // ripresa e il suo `/loop`.
        let casa = HomeIsolata::nuova("segnale-altrui");
        signal_for(&casa, "/percorso/consegna.md", "punto altrui", "", "tab-di-un-altro");
        std::env::set_var("ORCA_TAB_ID", "tab-mia");
        let msg = resume_message();
        std::env::remove_var("ORCA_TAB_ID");
        assert!(msg.is_empty(), "ha raccolto il mandato di un'altra sessione: {msg}");
        // E resta sul disco per chi lo aspetta.
        let signal = casa
            .dir
            .join(".claude/state/riprendi-da")
            .join(format!("{}.txt", state_key("repo::_prova")));
        assert!(signal.exists(), "ha consumato un segnale non suo");
    }

    #[test]
    fn a_signal_addressed_to_us_is_consumed() {
        let casa = HomeIsolata::nuova("segnale-mio");
        signal_for(&casa, "/percorso/consegna.md", "il mio punto", "", "tab-mia");
        std::env::set_var("ORCA_TAB_ID", "tab-mia");
        let msg = resume_message();
        std::env::remove_var("ORCA_TAB_ID");
        assert!(msg.contains("il mio punto"), "{msg}");
    }

    #[test]
    fn a_multiline_mandate_arrives_intact() {
        // IL CASO PER CUI QUESTO CANALE ESISTE. Un mandato di `/loop` è spesso
        // un elenco, e il formato a righe etichettate — provato per un'ora il
        // 19/08/2026 — riattaccava le righe senza separatore: «1. leggi X» e
        // «2. fai Y» arrivavano come «1. leggi X2. fai Y».
        let casa = HomeIsolata::nuova("segnale-multiriga");
        let mandato = "/loop Sistema la configurazione:\n1. leggi X\n2. fai Y";
        signal_json(&casa, "/percorso/consegna.md", "il primo punto\ne il secondo", mandato);
        let msg = resume_message();
        assert!(msg.contains(mandato), "mandato corrotto nel viaggio:\n{msg}");
        assert!(msg.contains("il primo punto\ne il secondo"), "punto corrotto:\n{msg}");
        assert!(!msg.contains("leggi X2."), "righe riattaccate senza separatore:\n{msg}");
    }

    #[test]
    fn a_session_with_no_id_sweeps_nothing() {
        let casa = HomeIsolata::nuova("fine-senza-id");
        let state = casa.dir.join(".claude/state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("consegna-misura-77776666"), "x").unwrap();
        forget_session(&event("SessionEnd", ""));
        assert!(state.join("consegna-misura-77776666").exists());
    }

    #[test]
    fn one_panel_key_is_enough_and_the_worktree_is_never_optional() {
        assert!(missing_panel_keys("wt", "term", "tab").is_empty());
        assert!(missing_panel_keys("wt", "", "tab").is_empty());
        assert!(missing_panel_keys("wt", "term", "").is_empty());
        assert_eq!(
            missing_panel_keys("wt", "", ""),
            vec!["ORCA_TAB_ID|ORCA_TERMINAL_HANDLE"]
        );
        assert_eq!(missing_panel_keys("", "term", "tab"), vec!["ORCA_WORKTREE_ID"]);
        // Una variabile esportata piena di spazi è assente quanto una che non
        // c'è: `env()` restituisce la stringa così com'è, e il confronto con la
        // stringa vuota da solo la lascerebbe passare per buona.
        assert_eq!(missing_panel_keys("  ", "term", "tab"), vec!["ORCA_WORKTREE_ID"]);
    }

    /// Il payload di un avvio, nella forma che il gancio riceve.
    fn start(session: &str) -> serde_json::Value {
        serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": session,
            "transcript_path": "",
            "cwd": "/prova/albero",
            "source": "startup",
        })
    }

    /// Le righe del registro dei ganci scritte finora nella casa isolata.
    fn journal_lines(home: &HomeIsolata) -> String {
        std::fs::read_to_string(home.stato().join("ganci.jsonl")).unwrap_or_default()
    }

    /// Il caso che conta: dentro Orca una chiave mancante non fa più sparire la
    /// sessione in silenzio.
    ///
    /// DIFFERENZIALE A VARIABILE UNICA — i due bracci cambiano solo per
    /// `ORCA_WORKTREE_ID`. La prova «il record c'è / non c'è» da sola non
    /// basterebbe: togliendo la riga di registro dal ramo, il record continua a
    /// mancare in entrambi i modi e la mutazione passerebbe. Quello che separa
    /// il guasto muto dal guasto detto è la riga, e qui si guarda quella.
    #[test]
    fn inside_orca_a_missing_key_leaves_a_line_instead_of_silence() {
        let home = HomeIsolata::nuova("registro-sessione-muta");
        std::env::set_var("ORCA_AGENT_HOOK_PORT", "49571");
        std::env::set_var("ORCA_TAB_ID", "tab-1");
        std::env::set_var("ORCA_TERMINAL_HANDLE", "term-1");

        std::env::set_var("ORCA_WORKTREE_ID", "repo::/prova/albero");
        record_session(&start("aaaaaaaa-1111-2222-3333-444444444444"));
        assert!(
            home.stato().join("sessioni-vive/aaaaaaaa.json").exists(),
            "con tutte le chiavi il record deve esserci"
        );
        assert!(
            !journal_lines(&home).contains("register-session"),
            "una registrazione riuscita non lascia righe: {}",
            journal_lines(&home)
        );

        std::env::remove_var("ORCA_WORKTREE_ID");
        record_session(&start("bbbbbbbb-1111-2222-3333-444444444444"));
        assert!(
            !home.stato().join("sessioni-vive/bbbbbbbb.json").exists(),
            "senza la chiave dell'albero non si registra, e resta così"
        );
        let lines = journal_lines(&home);
        assert!(lines.contains("\"gancio\":\"register-session\""), "{lines}");
        assert!(lines.contains("chiavi-del-pannello-mancanti"), "{lines}");
        assert!(lines.contains("ORCA_WORKTREE_ID"), "{lines}");
        assert!(lines.contains("\"session\":\"bbbbbbbb\""), "{lines}");
    }

    /// Fuori da Orca la stessa mancanza è il caso normale e resta muta:
    /// altrimenti ogni sessione aperta da un terminale qualunque scriverebbe una
    /// riga, e il registro che serve a contare i guasti conterebbe il normale.
    #[test]
    fn outside_orca_the_same_absence_stays_silent() {
        let home = HomeIsolata::nuova("registro-fuori-da-orca");
        std::env::remove_var("ORCA_AGENT_HOOK_PORT");
        std::env::remove_var("ORCA_WORKTREE_ID");
        std::env::remove_var("ORCA_TAB_ID");
        std::env::remove_var("ORCA_TERMINAL_HANDLE");

        record_session(&start("cccccccc-1111-2222-3333-444444444444"));

        assert!(!home.stato().join("sessioni-vive/cccccccc.json").exists());
        assert!(
            !journal_lines(&home).contains("register-session"),
            "fuori da Orca non c'è niente da denunciare: {}",
            journal_lines(&home)
        );
    }
}
