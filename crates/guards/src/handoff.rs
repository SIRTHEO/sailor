//! Quanto contesto ha consumato una sessione, e da che modello si misura.
//!
//! Porta della parte pura di `skills/hooks/handoff_common.py`, il file su cui
//! poggiano la staffetta e il presidio che arma il successore. Qui sta ciò che
//! si può decidere leggendo un transcript e nient'altro; la parte che tocca
//! disco, stato e `orca` sta in `claude-hooks/src/handoff.rs`, perché la
//! separazione è quella che rende provabile il resto.
//!
//! IL BUDGET NON È LA FINESTRA. Le soglie sono frazioni del budget di
//! **qualità** — il punto oltre cui la degradazione morde — non della finestra
//! tecnica. Opus 5 dichiara 1M di finestra e qui vale 500k, perché RULER misura
//! il crollo intorno a metà. Sbagliare questo numero non rompe niente in modo
//! visibile: fa consegnare tardi, quando il documento lo scrive una sessione già
//! degradata.
//!
//! L'ORDINE DELL'ELENCO NON CONTA OGGI, E DOMANI SÌ. Le chiavi sono frammenti
//! cercati dentro il model-id, e i sette attuali sono disgiunti: nessun id
//! contiene due frammenti insieme, quindi scorrere l'elenco in un ordine o
//! nell'altro dà lo stesso risultato — verificato per mutazione il 17/08/2026,
//! invertendo `opus-5` con `opus-4-8` su 120 transcript veri senza una sola
//! divergenza. L'ordine del Python è conservato lo stesso perché basta
//! aggiungere un frammento più generico (`opus`, `claude`) perché diventi di
//! colpo significativo, e quel giorno nessuno ricorderà di controllarlo.

/// Budget di qualità in token per frammento di model-id, dal più specifico.
pub const MODEL_BUDGET: &[(&str, u64)] = &[
    ("opus-4-8", 200_000),
    ("opus-4.8", 200_000),
    ("opus-5", 500_000),
    ("sonnet-5", 400_000),
    ("haiku-4-5", 150_000),
    ("haiku-4.5", 150_000),
    ("fable-5", 300_000),
];

/// Modello sconosciuto: si taglia basso, mai oltre il più prudente conosciuto.
pub const DEFAULT_BUDGET: u64 = 180_000;

pub const WARN_FRACTION: f64 = 0.78;
pub const REQUIRE_FRACTION: f64 = 0.90;

/// Byte di crescita del transcript prima di rimisurare.
pub const MIN_GROWTH: u64 = 400_000;

/// Byte di coda letti da un transcript. Una sessione lunga arriva a centinaia di
/// MB e leggerla tutta a ogni chiamata costa più di quanto faccia risparmiare.
pub const TAIL_BYTES: u64 = 400_000;

/// Elenco **chiuso** di ciò che passa sopra la soglia: si dichiara cosa serve a
/// consegnare, non cosa è vietato. L'elenco dei divieti è sempre in ritardo
/// sullo strumento nuovo.
pub const HANDOFF_TOOLS: &[&str] = &[
    "Skill",
    "Read",
    "Write",
    "Edit",
    "TodoWrite",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
    "SendMessage",
    "Glob",
    "Grep",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Thresholds {
    pub model: String,
    pub budget: u64,
    pub warn: u64,
    pub require: u64,
}

/// Il budget di qualità per il modello dato, per frammento del model-id.
pub fn quality_budget(model: &str) -> u64 {
    let m = model.to_lowercase();
    for (fragment, budget) in MODEL_BUDGET {
        if m.contains(fragment) {
            return *budget;
        }
    }
    DEFAULT_BUDGET
}

/// Il model-id dell'ultimo turno dell'assistente presente nelle righe passate.
///
/// Si scorre **all'indietro**: interessa l'ultimo turno, e un transcript lungo ne
/// contiene migliaia. Il filtro su `"model"` prima del parse non è cosmesi:
/// evita di deserializzare ogni riga di un file da centinaia di MB.
pub fn model_from_lines(lines: &[&str]) -> String {
    for line in lines.iter().rev() {
        if !line.contains("\"model\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let m = d
            .get("message")
            .and_then(|x| x.get("model"))
            .or_else(|| d.get("model"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        // `<synthetic>` è il modello dei turni che il runtime inventa: prenderlo
        // per buono darebbe il budget di default a una sessione su Opus 5.
        if !m.is_empty() && m.to_lowercase() != "<synthetic>" {
            return m.to_string();
        }
    }
    String::new()
}

/// Modello, budget e soglie assolute per la sessione che ha scritto queste righe.
pub fn thresholds_from_lines(lines: &[&str]) -> Thresholds {
    let model = model_from_lines(lines);
    let budget = quality_budget(&model);
    Thresholds {
        model: if model.is_empty() {
            "sconosciuto".to_string()
        } else {
            model
        },
        budget,
        // `int()` in Python tronca verso zero e questi sono positivi, quindi
        // `as u64` fa lo stesso. Arrotondare darebbe soglie diverse di un token:
        // invisibile finché non è esattamente il caso di confine.
        warn: (budget as f64 * WARN_FRACTION) as u64,
        require: (budget as f64 * REQUIRE_FRACTION) as u64,
    }
}

/// I token in contesto all'ultimo turno dell'assistente presente nelle righe.
///
/// Somma i tre campi che compongono l'ingresso reale: quello nuovo, quello letto
/// dalla cache e quello che la cache ha appena scritto. Contarne uno solo
/// sottostima di un ordine di grandezza con la cache calda, ed è la misura su
/// cui si decide se consegnare.
pub fn context_used_from_lines(lines: &[&str]) -> u64 {
    context_used_found(lines).unwrap_or(0)
}

/// Come sopra, ma distingue **«non ho trovato nessun `usage`»** (`None`) da
/// «l'ho trovato e somma zero» (`Some(0)`).
///
/// La differenza non è teorica ed è costata un commento falso: chi chiama scrive
/// il memo della misura quando ha trovato un `usage`, somma compresa lo zero —
/// un turno con `{"output_tokens":5}` e nient'altro conta come misura fatta.
/// Il porto scriveva il memo solo per una somma positiva, e su una trascrizione
/// vera lasciava il disco diverso dall'originale: `consegna-misura-* = <size> 0`
/// di là, niente di qua.
pub fn context_used_found(lines: &[&str]) -> Option<u64> {
    for line in lines.iter().rev() {
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(u) = d
            .get("message")
            .and_then(|x| x.get("usage"))
            .or_else(|| d.get("usage"))
        else {
            continue;
        };
        // Il Python salta gli `usage` vuoti e continua a scorrere: senza questo
        // un ultimo turno con `"usage":{}` azzererebbe la misura, e la sessione
        // risulterebbe di colpo sotto soglia.
        if u.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            continue;
        }
        let field = |name: &str| u.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
        return Some(
            field("input_tokens")
                + field("cache_read_input_tokens")
                + field("cache_creation_input_tokens"),
        );
    }
    None
}

/// Se l'ultimo record del transcript chiude un turno, o ne lascia uno aperto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    /// L'ultima cosa scritta è la risposta finale dell'assistente: nessun
    /// `tool_use` in attesa del suo risultato.
    Ended,
    /// Un `tool_use` senza il suo risultato, o un messaggio che aspetta ancora
    /// una risposta: il turno non è chiuso.
    InProgress,
    /// L'ultima riga non si legge, o non si capisce di che tipo sia.
    Unknown,
}

/// «FERMO» VUOL DIRE «ZITTO», NON «LIBERO». `tui-idle` dice solo che il
/// pannello non sta scrivendo in questo istante, ed è vero anche a metà di un
/// comando che dorme 300 secondi: misurato il 20/08/2026, `terminal wait --for
/// tui-idle` rispondeva `satisfied: true` con l'ultima riga del prompt vuota su
/// un turno ancora vivo. Qui si guarda l'ultimo record vero: un `tool_use`
/// senza il suo risultato prova che il turno non è finito, non la quiete del
/// terminale — che è solo silenzio, non libertà.
///
/// SI GUARDA L'ULTIMO RECORD **DI TURNO**, non l'ultima riga del file: la
/// domanda è «si può scrivere ORA», e un turno chiuso tre righe fa non conta se
/// nel frattempo ne è arrivato un altro senza risposta. In dubbio — riga
/// illeggibile — si risponde `Unknown`: «non lo so» pesa più di un'ipotesi.
///
/// UN'ANNOTAZIONE IN CODA NON È UN TURNO, E QUESTA DISTINZIONE VALEVA LA
/// STAFFETTA. Fino al 21/08/2026 qualunque tipo diverso da `assistant` e `user`
/// dava `Unknown`, e i ganci di questa casa appendono in coda record che turni
/// non sono — `system`, `attachment`, `atis-latch`, `pr-link`. L'esito non era
/// «non lo so per ora» ma «non lo so mai più»: la condizione non poteva cadere
/// da sola, perché la riga in coda non cambia da sé. Misurato lo stesso giorno:
/// **588 rinvii identici in dieci ore** su una sessione, e nessuna sostituzione
/// riuscita in ventun ore — quattro sessioni vive su quattro, tutte illeggibili
/// per questa stessa ragione. Ora quelle righe si scavalcano e si cerca il
/// turno vero sotto.
pub fn turn_status_from_lines(lines: &[&str]) -> TurnStatus {
    for line in lines.iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        // Una riga che non si legge ferma la scansione invece di lasciarla
        // proseguire: potrebbe essere un turno troncato a metà scrittura, e
        // scavalcarla vorrebbe dire rispondere su ciò che c'era prima.
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else {
            return TurnStatus::Unknown;
        };
        match d.get("type").and_then(|v| v.as_str()) {
            Some("assistant") => {
                let pending = d
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .map(|parts| {
                        parts
                            .iter()
                            .any(|p| p.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                    })
                    .unwrap_or(false);
                return if pending {
                    TurnStatus::InProgress
                } else {
                    TurnStatus::Ended
                };
            }
            // Un `user` in coda — vero o iniettato da un gancio — aspetta ancora
            // una risposta: l'assistente non ha ancora parlato dopo di lui.
            Some("user") => return TurnStatus::InProgress,
            // Tutto il resto è annotazione, e si scavalca. NON è un elenco
            // chiuso di tipi noti apposta: l'elenco chiuso è quello dei due tipi
            // che un turno lo sono davvero, e un tipo nuovo inventato domani
            // sarebbe di nuovo un blocco che non cade.
            _ => continue,
        }
    }
    TurnStatus::Unknown
}

/// Vero **solo** se questa chiamata è l'invocazione della skill `handoff`.
///
/// Niente rilevamento da Write/Edit su un file che si chiama `consegna-*.md`:
/// scrivere quel documento non è aver consegnato la propria sessione. Il
/// 13/08/2026 quel ramo aveva già prodotto cinque marcatori falsi, fra cui
/// quello della sessione che stava scrivendo questi stessi ganci — e un
/// `consegna-fatta` falso autorizza la staffetta a rigenerare una sessione che
/// non ha consegnato affatto.
pub fn is_handoff_call(tool: &str, tool_input: Option<&serde_json::Value>) -> bool {
    if tool != "Skill" {
        return false;
    }
    match tool_input {
        None => false,
        // La ricerca è sulla forma serializzata e non sui singoli campi, perché
        // il nome della skill può arrivare sotto chiavi diverse.
        Some(v) => serde_json::to_string(v)
            .unwrap_or_default()
            .to_lowercase()
            .contains("handoff"),
    }
}

/// Un pannello Orca, per quel poco che serve a riconoscerlo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Terminal {
    pub handle: String,
    pub tab_id: String,
    pub worktree_id: String,
    /// Il titolo del pannello. Serve alla staffetta, che lo ricopia sul
    /// successore: senza, ogni sessione rigenerata perde il suo nome e la barra
    /// si riempie di schede che si chiamano tutte «Claude Code».
    pub title: String,
}

impl Terminal {
    /// I pannelli dentro la risposta di `orca terminal list --json`.
    ///
    /// Costruiti a mano invece che con `derive(Deserialize)` perché `guards` non
    /// dipende da `serde` e il mandato non vuole dipendenze nuove: qui i campi
    /// sono tre e la mappatura sta in sei righe.
    ///
    /// La risposta arriva o come `{"result":{"terminals":[…]}}` o già come lista:
    /// si accettano entrambe, come fa il Python, perché la forma è cambiata una
    /// volta e chi la fissa la rincorre.
    pub fn from_response(value: &serde_json::Value) -> Option<Vec<Terminal>> {
        let inner = value.get("result").unwrap_or(value);
        // `items = d.get('result', d)`, poi `if isinstance(items, dict)`: solo
        // su un OGGETTO il Python scende dentro `terminals`. Su qualunque altra
        // forma resta dov'è, e se non è una lista risponde `None`.
        let vuoto = serde_json::Value::Array(Vec::new());
        let items = if inner.is_object() {
            match inner.get("terminals") {
                // Il Python scrive `items.get('terminals') or []`: un valore
                // falso — assente, `null`, `0`, `""`, `[]`, `{}` — diventa la
                // lista vuota, mentre uno vero passa così com'è e verrà
                // giudicato dal controllo sulla lista qui sotto.
                Some(v) if !is_falsy(v) => v,
                _ => &vuoto,
            }
        } else {
            inner
        };
        // NIENTE LISTA, NIENTE RISPOSTA. Prima una forma che non si riconosce
        // tornava `Vec::new()`, e a valle una lista vuota vuol dire «i pannelli
        // sono morti tutti»: la chiusura si dichiarava riuscita senza aver
        // chiuso niente, e la sessione vecchia restava viva sullo stesso albero.
        // È lo stesso difetto che `read_terminals` dice di aver chiuso — 276
        // giri «riusciti» mentre cancellava record di sessioni vive — rimasto
        // qui dentro perché il salto di forma avveniva un livello più giù.
        let items = items.as_array()?;
        Some(items
            .iter()
            // Ciò che non è un oggetto non è un terminale, e va SALTATO, non
            // trasformato in un pannello dai campi vuoti: nel ramo che sceglie
            // per worktree la risposta dipende da quanti candidati ci sono
            // (`len(hits) == 1`), quindi un fantasma in più fa tacere una
            // risoluzione che l'oracolo dà.
            .filter(|t| t.is_object())
            .map(|t| {
                let field = |name: &str| {
                    t.get(name)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                Terminal {
                    // `id` è il ripiego che il Python accetta accanto a `handle`.
                    handle: {
                        let h = field("handle");
                        if h.is_empty() {
                            field("id")
                        } else {
                            h
                        }
                    },
                    tab_id: field("tabId"),
                    worktree_id: field("worktreeId"),
                    title: field("title"),
                }
            })
            .collect())
    }
}

/// Falso come lo intende Python: assente, `null`, `false`, zero, stringa,
/// lista o oggetto vuoti. Serve a tradurre `x or []`, che non è un controllo
/// sul tipo ma sulla verità — e i due si comportano diverso proprio sui casi
/// storti, che sono gli unici per cui questa funzione esiste.
fn is_falsy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(b) => !b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f == 0.0).unwrap_or(false),
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Object(o) => o.is_empty(),
    }
}

/// Ciò che arriva a una sessione senza che nessuno lo abbia scritto.
///
/// Elenco CHIUSO, e chiuso dal lato giusto: quello che non è qui dentro conta
/// come lavoro umano, quindi un marcatore che non conoscessimo ancora produce
/// «non chiudo» — l'errore che costa un giro, non una sessione. Va tenuto uguale
/// a `AUTOMATIC_PREFIXES` di `handoff_common.py`: se i due elenchi divergono, le
/// due implementazioni chiudono sessioni diverse, e `compare-relay-evaluate.py`
/// se ne accorge.
pub const AUTOMATIC_PREFIXES: [&str; 9] = [
    "<task-notification>",
    "<system-reminder>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<command-name>",
    "<command-message>",
    "<user-prompt-submit-hook>",
    "Caveat: The messages below were generated by the user while running local commands",
    "This session is being continued from a previous conversation",
];

/// Un `worktree_id` reso adatto a stare in un nome di file.
///
/// IL DIFETTO CHE CHIUDE, misurato il 17/08/2026. Un identificativo di copia
/// vero è `<uuid>::/Users/theo/orca/general`, con le barre del percorso dentro,
/// e i due file che la staffetta scrive lo prendono come nome:
/// `state/staffetta-cooldown-<worktree>` e `state/riprendi-da/<worktree>.txt`.
/// Quello non è un nome, è un percorso dentro cartelle che non esistono: la
/// scrittura fallisce, e i due chiamanti la ingoiano perché sono fail-open.
///
/// Sul disco di quel giorno: **zero** raffreddamenti e **zero** segnali di
/// ripresa, con 6 sessioni vive su 6 che avevano barre nell'identificativo. Le
/// due conseguenze si vedevano da fuori — nessuna tregua fra un tentativo e il
/// successivo, e un successore che nasce senza sapere da dove riprendere.
///
/// Si sostituisce invece di accorciare: tenendo il solo uuid, due copie diverse
/// dello stesso repo finirebbero sullo stesso nome e la tregua dell'una
/// fermerebbe l'altra.
pub fn state_key(worktree_id: &str) -> String {
    worktree_id
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect()
}

/// L'handle della propria scheda, chiesto adesso invece che ricordato.
///
/// IL DIFETTO CHE CHIUDE. `ORCA_TERMINAL_HANDLE` è catturato quando la sessione
/// parte e non si aggiorna mai più: ogni riavvio del terminale ne conia uno
/// nuovo, e il vecchio sparisce dall'elenco. Chi lo conserva parla a un oggetto
/// che non esiste. Misurato il 16/08/2026, e di nuovo il 17/08 su un marcatore
/// di successore che citava un handle assente mentre la sua tab lavorava.
///
/// IL PONTE STABILE È LA TAB, non il worktree. `ORCA_TAB_ID` sopravvive al
/// riavvio del terminale e identifica **una** scheda; il worktree no, perché due
/// sessioni sullo stesso albero sono un caso normale qui. Col solo worktree si
/// risponde **solo** se il candidato è unico: chiudere il terminale sbagliato
/// costa il lavoro di qualcun altro, e nel dubbio si tace.
///
/// L'ordine delle tre vie è comportamento, non stile: la tab batte l'handle noto,
/// che batte il worktree. Chi lo inverte fa rispondere l'handle scaduto per primo.
///
/// UNA TAB PUÒ AVERE PIÙ PANNELLI. `terminal split` ne affianca un secondo
/// nella stessa tab, e allora `tab_id` non basta più a scegliere: prima si
/// prendeva il primo trovato, e la tab reale `2ae72849` — due pannelli — non
/// dava modo di sapere quale fosse quello giusto. Qui si segue la stessa forma
/// del ramo sul worktree qui sotto: se il candidato non è unico, si tace.
/// Astenersi costa un giro saltato; scrivere sul pannello sbagliato costa i
/// tasti di una sessione estranea.
pub fn resolve_terminal_handle(
    tab_id: &str,
    worktree_id: &str,
    known_handle: &str,
    terminals: &[Terminal],
) -> String {
    if terminals.is_empty() {
        return String::new();
    }
    if !tab_id.is_empty() {
        let hits: Vec<&Terminal> = terminals.iter().filter(|t| t.tab_id == tab_id).collect();
        if hits.len() == 1 {
            return hits[0].handle.clone();
        }
        // SCHEDA AMBIGUA NON VUOL DIRE IDENTITÀ IGNOTA. Fino al 21/08/2026 qui
        // si usciva a mani vuote in ogni caso, e il ripiego sotto — l'handle
        // noto — non veniva mai raggiunto. Da quando le figure di guardia stanno
        // in pannelli affiancati, la scheda `general` ne ha tre: il presidio si
        // asteneva ogni minuto pur avendo in mano l'handle giusto, vivo e
        // scritto nella scheda della sessione.
        //
        // SI CHIEDE CHE L'HANDLE SIA UNO DEI CANDIDATI, non solo che sia vivo, e
        // la differenza non è formale: una scheda **sparita** (`hits` vuoto)
        // significa che la sessione sta altrove, e un handle vivo di un'altra
        // scheda è il pannello di qualcun altro — scriverci dentro costa i tasti
        // di una sessione estranea. Con `hits` a più di uno l'handle scioglie
        // l'ambiguità perché è uno di quei pannelli; con `hits` vuoto non
        // scioglie niente, e lì si tace come prima.
        if hits.len() > 1 && !known_handle.is_empty() {
            if let Some(t) = hits.iter().find(|t| t.handle == known_handle) {
                return t.handle.clone();
            }
        }
        return String::new();
    }
    // Il ripiego per i record scritti prima che la tab venisse salvata: quel
    // vecchio handle vale se è ancora fra i vivi, altrimenti no. Senza,
    // adottare questa funzione cancellerebbe le sessioni già in corso.
    if !known_handle.is_empty() {
        return if terminals.iter().any(|t| t.handle == known_handle) {
            known_handle.to_string()
        } else {
            String::new()
        };
    }
    if !worktree_id.is_empty() {
        let hits: Vec<&Terminal> = terminals
            .iter()
            .filter(|t| t.worktree_id == worktree_id)
            .collect();
        if hits.len() == 1 {
            return hits[0].handle.clone();
        }
    }
    String::new()
}

/// L'handle di un successore già armato altrove, o vuoto se il marcatore che
/// lo descrive è scaduto.
///
/// NON SI ADOTTA IL PANNELLO DEL VICINO. `terminal split` mette il successore
/// nella STESSA tab di chi lo arma: due sessioni diverse condividono lo stesso
/// `tab_id`. Se quella armata muore e sulla tab resta solo la sorella — magari
/// la sessione originaria, rinata su un handle nuovo — `resolve_terminal_handle`
/// trova un solo pannello per quella tab e lo darebbe per buono: è la lettura
/// giusta quando si segue la STESSA sessione (`the_tab_finds_the_reborn_handle`
/// qui sotto), sbagliata quando si verifica un marcatore su un'ALTRA sessione.
/// Cinque dei nove fallimenti della staffetta del 19/08/2026 hanno mandato i
/// tasti alla sessione sbagliata dello stesso albero così, e una di quelle ha
/// letto la consegna altrui e ha risposto «Testimone raccolto».
///
/// Un manico registrato che non è più fra i vivi non prova che «l'handle è
/// cambiato» — prova che QUESTO marcatore è scaduto: nessun pannello vivo porta
/// più l'identità che aveva quando è stato scritto. Qui si risponde vuoto
/// invece di indovinare quale sia il vicino.
pub fn resolve_armed_successor(tab_id: &str, known_handle: &str, terminals: &[Terminal]) -> String {
    if !known_handle.is_empty() && !terminals.iter().any(|t| t.handle == known_handle) {
        return String::new();
    }
    resolve_terminal_handle(tab_id, "", known_handle, terminals)
}

/// Cosa la staffetta deve fare di una sessione registrata.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Chiudere la vecchia e aprire il successore.
    Regenerate,
    /// Non toccare niente. È il default in ogni dubbio.
    Skip,
    /// Il pannello non esiste più: si butta il record, non la sessione.
    Clean,
    /// Un successore è già stato aperto da un altro meccanismo: si chiude la
    /// vecchia **senza** crearne un altro.
    ///
    /// Nasce da un difetto misurato il 17/08/2026. Il freno aggiunto quella
    /// mattina rispondeva `Skip` quando trovava un successore già armato, per
    /// non aprirne un secondo. Ma il compito della staffetta è **chiudere la
    /// vecchia**, e saltare non chiude mai: la sessione originale restava viva
    /// accanto al successore, e continuava a lavorare su un documento di
    /// consegna che invecchiava. Misurato su `d6cefb4d`: successore armato alle
    /// 13:55 del 16/08, ultima attività della vecchia alle **23:34** — nove ore
    /// e mezza dopo, con 93 MB di transcript.
    Retire,
}

/// I fatti su una sessione, già raccolti da chi ha il permesso di leggerli.
///
/// La decisione sta separata dalla raccolta di proposito: in Python `evaluate`
/// legge quattro file mentre decide, e questo la rende provabile solo con una
/// `HOME` finta. Qui i fatti entrano come dati e la funzione resta pura, così i
/// casi limite — l'elenco illeggibile, il successore morto — si scrivono senza
/// preparare un filesystem.
#[derive(Debug, Default)]
pub struct SessionFacts<'a> {
    pub session: &'a str,
    pub handle: &'a str,
    pub worktree: &'a str,
    /// La scheda su cui vive la sessione, vuota per i record scritti prima che
    /// venisse salvata.
    ///
    /// È L'IDENTITÀ CHE SOPRAVVIVE, e il manico no: `ORCA_TERMINAL_HANDLE` è la
    /// fotografia dell'incarnazione che c'era all'avvio, e dopo un riattacco
    /// Orca ne conia un altro. Chi *scrive* il record lo sa e lo dichiara — è il
    /// motivo per cui il record porta anche la scheda — e fino al 21/08/2026 chi
    /// *cancellava* guardava solo il manico: la sessione `04985c6a` è sparita
    /// dal registro alle 08:57 mentre lavorava.
    ///
    /// **NON È SOLO DEI RECORD VECCHI, ed è la riga che manca a chi si fida di
    /// questa protezione**: chi scrive il record si accontenta di *una* delle due
    /// chiavi del pannello, quindi una sessione nata oggi a cui Orca passa solo
    /// il manico ha la scheda vuota e resta esposta al difetto originale. La
    /// protezione copre un sottoinsieme dei record, non tutti quelli nuovi.
    pub tab_id: &'a str,
    /// `None` = l'elenco dei pannelli **non si è potuto leggere**. Non è «sono
    /// morti tutti»: la differenza vale un registro di sessioni cancellato ogni
    /// minuto, ed è già costata 276 giri tutti «riusciti».
    pub live_handles: Option<&'a [String]>,
    /// Le schede vive adesso, con lo stesso contratto di `live_handles`: `None`
    /// = **non si è potuto leggere**, mai «sono morte tutte».
    pub live_tabs: Option<&'a [String]>,
    pub opted_out: bool,
    pub in_cooldown: bool,
    /// L'handle del successore che un altro meccanismo ha già aperto, se vivo.
    pub armed_successor: &'a str,
    pub handoff_done: bool,
    /// La consegna è stata una **scelta**, non un ordine del presidio: scritta
    /// mentre il contesto era ancora sotto la soglia. Vale come secondo motivo
    /// per passare il testimone, accanto all'occupazione.
    pub handoff_deliberate: bool,
    pub transcript_exists: bool,
    /// Dopo aver consegnato, la sessione ha ricevuto altro lavoro.
    ///
    /// Costa la lettura della coda del transcript, quindi si raccoglie insieme
    /// alle soglie e non prima: le guardie che costano un `exists()` hanno già
    /// fermato tutto ciò che si poteva fermare gratis.
    pub worked_after_handoff: bool,
    /// Secondi che mancano al risveglio che la sessione si è armata da sola.
    ///
    /// `None` = nessun appuntamento in agenda. Un `/loop` ne arma uno a ogni
    /// giro, e quel risveglio muore col processo: chiudere la sessione mentre è
    /// in attesa non passa il testimone, spegne il loop.
    pub wakeup_in: Option<u64>,
    pub used: u64,
    pub thresholds: Option<&'a Thresholds>,
}

/// Arrotonda come `round()` di Python: sulla metà esatta **al pari**.
///
/// `f64::round()` di Rust arrotonda invece lontano da zero, e la differenza non
/// è teorica: un vaglio indipendente su 1932 combinazioni ha trovato sei casi
/// veri in cui la riga di registro divergeva — 90,5% diventava `90` di là e `91`
/// di qua. Cambia solo il testo, mai l'azione, ma quel testo è la riga che si
/// confronta per dire che le due implementazioni non si distinguono.
///
/// Prima qui c'era `.round()` con un commento che affermava di replicare
/// Python. Il commento era falso, ed è il secondo commento falso trovato in
/// questa giornata di porting: entrambi asserivano una proprietà che nessun
/// caso verificava.
pub fn round_half_to_even(x: f64) -> u64 {
    let floor = x.floor();
    let diff = x - floor;
    let r = if diff > 0.5 {
        floor + 1.0
    } else if diff < 0.5 {
        floor
    } else if (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    };
    r.max(0.0) as u64
}

/// Il separatore delle migliaia di Python (`f'{n:,}'`): la virgola.
///
/// Non è cosmesi: questi numeri compaiono nel testo che la sessione legge e che
/// il confronto pretende identico byte a byte. `180000` e `180,000` sono due
/// stringhe diverse. Sta qui perché la usano due presìdi — `handoff_required` e
/// `handoff_on_stop` — e due copie divergono al primo ritocco.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// La percentuale del budget, arrotondata come `round()` di Python.
pub fn percent(used: u64, budget: u64) -> u64 {
    round_half_to_even(used as f64 / budget as f64 * 100.0)
}

/// Azione e motivo. In dubbio si risponde sempre `Skip`.
///
/// L'ORDINE DEI CONTROLLI È IL COMPORTAMENTO. `Clean` sta dopo la lettura
/// dell'elenco e prima di tutto il resto, perché un record che punta a un
/// pannello morto non ha altro da dire; il raffreddamento sta prima della
/// consegna perché una sessione appena rigenerata non va riesaminata; e il
/// successore già armato sta **dopo** la soglia, perché la domanda che pone non
/// è «vale la pena guardare questa sessione» ma «resta da creare o solo da
/// chiudere»: metterlo prima faceva rispondere `Skip` a sessioni che non avevano
/// nemmeno consegnato, e quel `Skip` non chiudeva mai niente.
pub fn evaluate(f: &SessionFacts) -> (Action, String) {
    if f.session.is_empty() || f.handle.is_empty() || f.worktree.is_empty() {
        return (Action::Skip, "record incompleto".into());
    }
    if f.opted_out {
        return (Action::Skip, "opt-out".into());
    }
    let Some(live) = f.live_handles else {
        return (Action::Skip, "elenco dei terminali illeggibile".into());
    };
    // SI GUARDANO TUTTE E DUE LE IDENTITÀ, e si cancella solo se nessuna delle
    // due risulta viva. Il manico da solo è una fotografia che scade a ogni
    // riattacco (vedi `tab_id` qui sopra): il 21/08/2026 una sessione di guardia
    // è diventata invisibile mentre lavorava, e la sua scheda era lì nel record.
    // Il caso che il chiamante non ripara ririsolvendo il manico è la scheda con
    // più pannelli — `terminal split` — dove il candidato non è unico e
    // `resolve_terminal_handle` tace apposta.
    //
    // ILLEGGIBILE NON È «MORTA»: se l'elenco delle schede non si è potuto
    // leggere, la scheda del record vale come viva e il record resta. È la
    // stessa prudenza di `live_handles` due righe sopra, e lo stesso verso in
    // cui sbagliare — un record di troppo si riesamina al giro dopo, uno
    // cancellato per errore rende la sessione invisibile per sempre, perché
    // `sessioni-vive/<sess>.json` si riscrive solo a `SessionStart`.
    let tab_alive = !f.tab_id.is_empty()
        && match f.live_tabs {
            None => true,
            Some(tabs) => tabs.iter().any(|t| t == f.tab_id),
        };
    if !live.iter().any(|h| h == f.handle) && !tab_alive {
        return (Action::Clean, "terminale non piu' vivo".into());
    }
    if f.in_cooldown {
        return (Action::Skip, "cooldown".into());
    }
    if !f.handoff_done {
        return (Action::Skip, "handoff non ancora fatto".into());
    }
    if !f.transcript_exists {
        return (Action::Skip, "transcript assente".into());
    }
    let Some(t) = f.thresholds else {
        return (Action::Skip, "soglie non calcolabili".into());
    };
    // DUE MOTIVI PER PASSARE IL TESTIMONE, non uno. L'occupazione è quello per
    // cui la staffetta è nata, ma il CLAUDE.md ne prescrive un altro — «una
    // sessione, un ambito»: quando il lavoro cambia mestiere si consegna e si
    // riparte, con qualunque contesto residuo. Chi consegna sotto soglia non
    // obbedisce a un limite, dichiara finito il lavoro.
    //
    // Guardare la sola occupazione lasciava quel caso senza successore: la
    // consegna restava sul disco e non la raccoglieva nessuno, cioè proprio il
    // lavoro che doveva far proseguire. Misurato il 18/08/2026: consegnata al
    // 67% con la soglia al 90%, la sessione dopo non è mai partita e aprirla è
    // toccato a Theo. Le altre guardie restano: chi ha lavorato dopo la consegna
    // è protetto dal controllo qui sotto.
    if !f.handoff_deliberate && (f.used == 0 || f.used < t.require) {
        return (
            Action::Skip,
            format!("sotto soglia ({} < {}, {})", f.used, t.require, t.model),
        );
    }
    let pct = round_half_to_even(f.used as f64 / t.budget as f64 * 100.0);
    let piena = if f.handoff_deliberate {
        format!(
            "consegnato di proposito ({} token, {pct}% del budget {} ~{}): \
             ambito chiuso, non contesto pieno",
            f.used, t.model, t.budget
        )
    } else {
        format!(
            "consegnato e pieno ({} token, {pct}% del budget {} ~{})",
            f.used, t.model, t.budget
        )
    };
    // HA CONSEGNATO, MA POI HA RICEVUTO ALTRO LAVORO. Il 17/08/2026 questa
    // guardia non c'era e la staffetta ha chiuso due volte la stessa sessione
    // mentre lavorava: `cdca7b36` aveva un mandato arrivato ventun secondi dopo
    // la consegna, ed è stata rigenerata nove minuti dopo. «Consegnato» è un
    // marcatore che resta lì, e `tui-idle` dice soltanto «non sta scrivendo in
    // questo istante» — chi ha appena finito di rispondere è idle.
    //
    // Sta **prima** del congedo perché anche il congedo chiude: una sessione
    // che lavora non si tocca né rigenerandola né congedandola.
    if f.worked_after_handoff {
        return (
            Action::Skip,
            format!("{piena}; ma ha ricevuto lavoro dopo la consegna"),
        );
    }
    // Il successore c'è già: resta da chiudere la vecchia, non da aprirne un
    // altro. Questo controllo sta QUI e non più in cima di proposito — dove
    // stava, rispondeva `Skip` anche a una sessione che non aveva ancora
    // consegnato, e quel `Skip` non chiudeva niente mai.
    //
    // E STA PRIMA DEL RISVEGLIO ARMATO, non dopo. Chi ha già un successore vivo
    // ha già passato il testimone: il proprio appuntamento non serve più a
    // nessuno, e onorarlo significherebbe due sessioni sullo stesso lavoro.
    // Nell'ordine opposto — provato il 19/08/2026 e segnalato da un vaglio
    // indipendente — una sessione in `/loop` con un successore già aperto
    // rispondeva `Skip` e restava viva accanto a lui finché il risveglio non
    // scadeva.
    if !f.armed_successor.is_empty() && live.iter().any(|h| h == f.armed_successor) {
        let short: String = f.armed_successor.chars().take(13).collect();
        return (Action::Retire, format!("{piena}; successore gia' vivo ({short})"));
    }
    // HA UN APPUNTAMENTO IN AGENDA. Il 19/08/2026 la sessione `23d89176` ha
    // chiuso il giro di un `/loop` alle 01:30:23 armando un risveglio a 1500
    // secondi, ed è stata rigenerata alle 01:30:27 — quattro secondi dopo —
    // perché `tui-idle` diceva «non sta scrivendo adesso». Il risveglio vive nel
    // processo chiuso e il mandato al successore parla solo della consegna:
    // all'ora dell'appuntamento non è ripartito niente.
    //
    // La via d'uscita è la soglia, non il tempo: sopra `require` il contesto è
    // davvero al limite e continuare costa più che ripartire, quindi si
    // rigenera lo stesso — ed è `regenerate` che deve allora portarsi dietro il
    // mandato del loop, altrimenti si torna a spegnerlo.
    if let Some(left) = f.wakeup_in {
        if f.used < t.require {
            return (
                Action::Skip,
                format!("{piena}; ma ha un risveglio armato fra {left}s"),
            );
        }
    }
    (Action::Regenerate, piena)
}

// ─── Il tetto alle sessioni: morde chi AGGIUNGE, mai chi SOSTITUISCE ─────────

/// Cosa fa al numero di sessioni vive il gesto che sta per essere eseguito.
///
/// QUESTA DISTINZIONE È L'INTERA REGOLA, ed è la cosa che chi legge dopo
/// sbaglierà — nel verso pericoloso, cioè applicando il tetto anche a chi
/// sostituisce. Una sessione piena che si rigenera è a **saldo zero**: apre la
/// nuova e chiude la vecchia. Fermarla sopra soglia la ferma proprio quando
/// serve di più — con venti sessioni vive e piene, negare la sostituzione non
/// fa scendere il conto di uno, tiene solo venti sessioni degradate al posto di
/// venti fresche. Il totale lo fanno salire le AGGIUNTE, e solo quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDelta {
    /// Ne apre una in più: il conto sale.
    Adds,
    /// Ne apre una al posto di un'altra: il conto non cambia.
    Replaces,
    /// Non apre nessuna sessione.
    None,
}

/// Il tetto oltre il quale non si aggiunge una sessione.
///
/// VENTI, E NON OTTO. Il tetto storico di `successor::arm` vale 8, ma è il tetto
/// di **un** produttore automatico, che apre schede senza nessuno davanti. Un
/// tetto globale a 8 negherebbe il lavoro normale: la settimana 10→17/08/2026 ha
/// mediana **7** sessioni vive, quindi scatterebbe quasi sempre. La soglia qui è
/// quella misurata sulla saturazione vera: nelle due ore prima del riavvio del
/// 16/08/2026 alle 23:20 le sessioni erano **64** contro 7 del resto della
/// settimana, mentre carico (6,7 contro 4,6) e memoria libera non discriminavano.
/// `sessioni >= 20` scatta nel **6,8% del tempo** e un terzo dei suoi scatti cade
/// nella finestra critica: le misure stanno in `docs/2026-08-17-cron-e-soglie.md`.
pub const SESSION_CAP_DEFAULT: usize = 20;

/// Il prefisso con cui un comando dichiara di chiudere ciò che apre.
///
/// Si scrive sulla riga di comando e non nell'ambiente del processo di proposito:
/// una variabile esportata una volta esenterebbe in silenzio tutto ciò che viene
/// dopo, cioè sarebbe una valvola per sbaglio. Scritta davanti al comando, la
/// dichiarazione vale per quel comando e si vede nel registro.
pub const REPLACEMENT_MARK: &str = "SESSION_REPLACES=1";

/// `word` compare nel testo come parola, non come pezzo di un'altra.
///
/// Serve perché `.claude/rust` contiene «claude» e non avvia niente, e
/// `claude-hooks` nemmeno: un `contains` avrebbe scambiato per apertura di
/// sessione ogni comando eseguito dentro questa cartella.
fn word_at(text: &str, word: &str) -> bool {
    const BORDER: &[char] = &[
        ' ', '\t', '\n', '\'', '"', ';', '&', '|', '(', ')', '=', '`',
    ];
    let mut from = 0;
    while let Some(hit) = text[from..].find(word) {
        let start = from + hit;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if before.is_none_or(|c| BORDER.contains(&c)) && after.is_none_or(|c| BORDER.contains(&c)) {
            return true;
        }
        from = end;
    }
    false
}

/// Gli spazi ripetuti diventano uno solo, così `terminal   create` si riconosce.
fn squeeze(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cosa questo comando fa al numero di sessioni vive.
///
/// ELENCO CHIUSO di cosa conta come apertura, non di cosa si esclude: l'elenco
/// dei divieti è sempre in ritardo sul gesto nuovo, e qui il verso giusto in cui
/// sbagliare è lasciar passare. Contano tre gesti — una scheda Orca che avvia
/// `claude`, un pannello affiancato che avvia `claude`, e una copia di lavoro
/// creata con un agente dentro. Un terminale che avvia una shell **non** è una
/// sessione: sono le due shell che `--setup run` lascia in ogni albero nuovo, e
/// contarle spegnerebbe il meccanismo senza che nessuna sessione sia stata
/// aperta.
///
/// `terminal split` sta qui perché la prescrizione manda gli agenti lì invece
/// che su `terminal create` (`rules/orca-e-la-sessione.md`): se il freno
/// riconoscesse solo il gesto vecchio, chi segue la prescrizione aprirebbe
/// sessioni che nessuno conta.
pub fn session_delta(command: &str) -> SessionDelta {
    let c = squeeze(command);
    if !word_at(&c, "orca") {
        return SessionDelta::None;
    }
    let with_agent = word_at(&c, "--agent");
    let new_pane = c.contains("terminal create") || c.contains("terminal split");
    let opens = (new_pane && (with_agent || word_at(&c, "claude")))
        || (c.contains("worktree create") && with_agent);
    if !opens {
        return SessionDelta::None;
    }
    if c.contains(REPLACEMENT_MARK) {
        return SessionDelta::Replaces;
    }
    SessionDelta::Adds
}

/// I fatti che il tetto guarda, già raccolti da chi ha il permesso di leggerli.
#[derive(Debug)]
pub struct CapFacts {
    pub delta: SessionDelta,
    /// Sessioni Claude vive adesso. `None` = **non si è potuto contare**.
    pub live: Option<usize>,
    pub cap: usize,
}

/// Il messaggio del rifiuto, oppure `None` se si passa.
///
/// FAIL-OPEN SU DUE FRONTI, e sono i due modi in cui questo freno potrebbe fare
/// più danno del problema: chi sostituisce passa sempre, e un conteggio che non
/// si è potuto fare lascia passare. Un tetto che blocca tutto perché non sa
/// contare è peggio di nessun tetto — smette di frenare l'apertura di sessioni e
/// comincia a frenare il lavoro, che è la fine di ogni freno.
pub fn session_cap_verdict(f: &CapFacts) -> Option<String> {
    if f.delta != SessionDelta::Adds {
        return None;
    }
    let live = f.live?;
    if live < f.cap {
        return None;
    }
    Some(format!(
        "{live} Claude sessions are already running (cap {}). This machine \
         restarted on 2026-08-16 at 23:20 with 64 sessions alive, and the number \
         of sessions was the only signal that told saturation apart from a normal \
         day. Close a session before opening another one. If this command closes \
         one as it opens one, say so by prefixing it with {REPLACEMENT_MARK} — a \
         replacement is never blocked. Valve: SESSION_CAP_GUARD=off.",
        f.cap
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opus5_thresholds() -> Thresholds {
        Thresholds {
            model: "claude-opus-5".into(),
            budget: 500_000,
            warn: 390_000,
            require: 450_000,
        }
    }

    fn full_session<'a>(
        live: &'a [String],
        t: &'a Thresholds,
    ) -> SessionFacts<'a> {
        SessionFacts {
            session: "provastf",
            handle: "term_x",
            worktree: "wt-1",
            live_handles: Some(live),
            handoff_done: true,
            transcript_exists: true,
            used: 480_000,
            thresholds: Some(t),
            ..Default::default()
        }
    }

    #[test]
    fn handed_off_and_full_regenerates() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let (a, why) = evaluate(&full_session(&live, &t));
        assert_eq!(a, Action::Regenerate);
        assert!(why.contains("96% del budget claude-opus-5"), "{why}");
    }

    #[test]
    fn below_threshold_and_not_deliberate_is_skipped() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 363_283; // il caso vero del 18/08/2026: 67% del budget
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("sotto soglia"), "{why}");
    }

    #[test]
    fn below_threshold_but_deliberate_passes_the_baton() {
        // Chi consegna con il contesto largo dichiara chiuso l'ambito. Senza
        // questo, la sua consegna resta sul disco e non la raccoglie nessuno.
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 363_283;
        f.handoff_deliberate = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Regenerate);
        assert!(why.contains("di proposito"), "{why}");
        assert!(why.contains("ambito chiuso"), "{why}");
    }

    #[test]
    fn an_armed_wakeup_stops_the_replacement() {
        // Il caso vero del 19/08/2026: `23d89176` aveva chiuso il giro di un
        // `/loop` armando un risveglio a 1500s, e quattro secondi dopo la
        // staffetta l'ha rigenerata perché aveva consegnato ed era idle.
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 406_854;
        f.handoff_deliberate = true;
        f.wakeup_in = Some(1108);
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("risveglio armato fra 1108s"), "{why}");
    }

    #[test]
    fn an_already_live_successor_beats_the_armed_wakeup() {
        // Chi ha già passato il testimone non ha più un appuntamento da
        // onorare: onorarlo lascerebbe due sessioni sullo stesso lavoro.
        // Nell'ordine opposto questa rispondeva `Skip` e restava viva accanto
        // al successore finché il risveglio non scadeva.
        let live = vec!["term_x".to_string(), "term_nuovo".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 406_854;
        f.handoff_deliberate = true;
        f.wakeup_in = Some(1108);
        f.armed_successor = "term_nuovo";
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Retire);
        assert!(why.contains("successore gia' vivo"), "{why}");
    }

    #[test]
    fn an_armed_wakeup_does_not_protect_a_context_at_the_limit() {
        // La via d'uscita: sopra `require` continuare costa più che ripartire.
        // Qui la staffetta rigenera lo stesso, ed è il segnale che il mandato
        // del loop deve viaggiare col testimone.
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 480_000; // 96%: oltre require
        f.wakeup_in = Some(1108);
        let (a, _) = evaluate(&f);
        assert_eq!(a, Action::Regenerate);
    }

    #[test]
    fn deliberate_does_not_override_a_session_that_kept_working() {
        // La guardia che protegge una sessione ancora al lavoro non si allenta:
        // consegnare di proposito non autorizza a chiudere chi sta lavorando.
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 363_283;
        f.handoff_deliberate = true;
        f.worked_after_handoff = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("lavoro dopo la consegna"), "{why}");
    }

    #[test]
    fn an_exact_half_rounds_to_even() {
        // I sei casi veri trovati dal vaglio indipendente: con `f64::round()`
        // davano tutti un punto in più del Python.
        assert_eq!(round_half_to_even(90.5), 90);
        assert_eq!(round_half_to_even(92.5), 92);
        assert_eq!(round_half_to_even(91.5), 92);
        assert_eq!(round_half_to_even(93.5), 94);
        // E il resto si comporta come chiunque si aspetta.
        assert_eq!(round_half_to_even(90.4), 90);
        assert_eq!(round_half_to_even(90.6), 91);
        assert_eq!(round_half_to_even(0.0), 0);
    }

    #[test]
    fn the_percentage_in_the_reason_rounds_half_to_even() {
        let live = vec!["term_x".to_string()];
        let t = Thresholds {
            model: "claude-opus-4-8".into(),
            budget: 200_000,
            warn: 156_000,
            require: 180_000,
        };
        let mut f = full_session(&live, &t);
        f.used = 181_000; // esattamente 90,5%
        let (_, why) = evaluate(&f);
        assert!(why.contains("90% del budget"), "{why}");
    }

    #[test]
    fn an_unreadable_list_does_not_mean_all_are_dead() {
        // `None` contro lista vuota: è la distinzione che valeva il registro
        // delle sessioni, cancellato ogni minuto per 276 giri «riusciti».
        let t = opus5_thresholds();
        let live: Vec<String> = vec![];
        let mut f = full_session(&live, &t);
        f.live_handles = None;
        assert_eq!(evaluate(&f).0, Action::Skip);
        f.live_handles = Some(&live);
        assert_eq!(evaluate(&f).0, Action::Clean);
    }

    #[test]
    fn a_live_tab_saves_the_record_of_a_dead_handle() {
        // DUE RECORD UGUALI TRANNE LA SCHEDA, e il manico morto in tutti e due:
        // quello con la scheda viva resta, l'altro se ne va. È il caso vero del
        // 21/08/2026 — la sessione `04985c6a`, cancellata alle 08:57 mentre
        // lavorava perché il suo pannello era rinato con un manico nuovo.
        //
        // L'elenco delle schede ne porta più di una di proposito: con una sola,
        // scambiare `any` per `all` non farebbe rosso nessuno di questi casi, e
        // la condizione passerebbe da «una qualunque combacia» a «tutte devono»
        // senza che niente lo denunci. Con l'elenco a una voce sola le due cose
        // sono indistinguibili.
        let t = opus5_thresholds();
        let live = vec!["term_altro".to_string()];
        let tabs = vec!["tab-di-un-altro".to_string(), "tab-viva".to_string()];
        let mut f = full_session(&live, &t);
        f.handle = "term_morto";
        f.live_tabs = Some(&tabs);

        f.tab_id = "tab-viva";
        assert_ne!(evaluate(&f).0, Action::Clean, "la scheda viva salva il record");

        f.tab_id = "tab-morta";
        assert_eq!(evaluate(&f).0, Action::Clean, "ne' manico ne' scheda: si butta");
    }

    #[test]
    fn an_unreadable_tab_list_does_not_mean_all_tabs_are_dead() {
        // La stessa prudenza di `live_handles`: «non ho potuto leggere» non è
        // «sono morte tutte». Con l'elenco assente il record resta.
        let t = opus5_thresholds();
        let live = vec!["term_altro".to_string()];
        let mut f = full_session(&live, &t);
        f.handle = "term_morto";
        f.tab_id = "tab-qualunque";
        f.live_tabs = None;
        assert_ne!(evaluate(&f).0, Action::Clean);
    }

    #[test]
    fn a_record_without_a_tab_is_still_judged_on_the_handle() {
        // I record scritti prima che la scheda venisse salvata non hanno niente
        // che li protegga, e devono continuare a comportarsi come prima:
        // altrimenti il registro non si ripulisce più.
        let t = opus5_thresholds();
        let live = vec!["term_altro".to_string()];
        let tabs = vec!["tab-viva".to_string()];
        let mut f = full_session(&live, &t);
        f.handle = "term_morto";
        f.tab_id = "";
        f.live_tabs = Some(&tabs);
        assert_eq!(evaluate(&f).0, Action::Clean);
        f.live_tabs = None;
        assert_eq!(evaluate(&f).0, Action::Clean);
    }

    #[test]
    fn an_armed_and_live_successor_stops_everything() {
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.armed_successor = "term_succ";
        let (a, why) = evaluate(&f);
        // NON `Skip`: la vecchia va chiusa comunque, solo senza aprirne un'altra.
        // Con `Skip` restava viva accanto al successore e continuava a lavorare
        // su una consegna che invecchiava — nove ore e mezza, misurate.
        assert_eq!(a, Action::Retire);
        assert!(why.contains("successore gia' vivo"), "{why}");
    }

    #[test]
    fn work_arriving_after_the_handoff_stops_the_regeneration() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.worked_after_handoff = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip, "ha chiuso una sessione che sta lavorando");
        assert!(why.contains("dopo la consegna"), "{why}");
    }

    #[test]
    fn and_it_stops_it_even_when_the_successor_is_already_live() {
        // L'ORDINE È IL COMPORTAMENTO. Anche il congedo chiude: se questa
        // guardia stesse dopo quella del successore, una sessione con un
        // successore già armato verrebbe chiusa mentre lavora — che è
        // esattamente il caso del 17/08/2026, dove il successore c'era.
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.armed_successor = "term_succ";
        f.worked_after_handoff = true;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn a_live_successor_does_not_close_a_session_that_never_handed_off() {
        // Il freno vale solo sulla creazione. Una sessione senza consegna non si
        // chiude comunque: il successore che c'è è di qualcun altro, o è vecchio.
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.armed_successor = "term_succ";
        f.handoff_done = false;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn a_live_successor_does_not_close_a_still_empty_session() {
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.armed_successor = "term_succ";
        f.used = 10_000;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn a_dead_successor_does_not_block_forever() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.armed_successor = "term_sparito";
        assert_eq!(evaluate(&f).0, Action::Regenerate);
    }

    #[test]
    fn below_the_threshold_nothing_is_touched() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.used = 100_000;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("sotto soglia (100000 < 450000"), "{why}");
    }

    #[test]
    fn without_a_handoff_nothing_is_ever_regenerated() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.handoff_done = false;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn the_cooldown_beats_the_threshold() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.in_cooldown = true;
        assert_eq!(evaluate(&f), (Action::Skip, "cooldown".to_string()));
    }

    #[test]
    fn an_incomplete_record_decides_nothing() {
        let live = vec!["term_x".to_string()];
        let t = opus5_thresholds();
        let mut f = full_session(&live, &t);
        f.handle = "";
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    fn three_terminals() -> Vec<Terminal> {
        vec![
            Terminal {
                handle: "term_a".into(),
                tab_id: "tab-1".into(),
                worktree_id: "wt-1".into(),
                ..Default::default()
            },
            Terminal {
                handle: "term_b".into(),
                tab_id: "tab-2".into(),
                worktree_id: "wt-2".into(),
                ..Default::default()
            },
            Terminal {
                handle: "term_c".into(),
                tab_id: "tab-3".into(),
                worktree_id: "wt-2".into(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn the_tab_finds_the_reborn_handle() {
        // Il caso vero: l'handle salvato è morto, la tab no.
        assert_eq!(
            resolve_terminal_handle("tab-1", "", "term_morto", &three_terminals()),
            "term_a"
        );
    }

    #[test]
    fn a_vanished_tab_does_not_answer_for_another_one() {
        assert_eq!(
            resolve_terminal_handle("tab-9", "", "term_a", &three_terminals()),
            ""
        );
    }

    #[test]
    fn without_a_tab_the_known_handle_counts_only_if_live() {
        assert_eq!(
            resolve_terminal_handle("", "", "term_b", &three_terminals()),
            "term_b"
        );
        assert_eq!(
            resolve_terminal_handle("", "", "term_morto", &three_terminals()),
            ""
        );
    }

    /// Una scheda ambigua non è un'identità ignota, se l'handle noto è vivo.
    ///
    /// IL CASO CHE NESSUNO COPRIVA, e che è costato un'ora di sessioni perse di
    /// vista il 21/08/2026: le figure di guardia stanno in pannelli affiancati,
    /// quindi la scheda `general` ne aveva **tre**, e il presidio si asteneva
    /// ogni minuto pur avendo in mano l'handle giusto, vivo, scritto nella
    /// scheda della sessione. Il test che copriva l'astensione passava un handle
    /// noto **vuoto**: esercitava il ramo, non la domanda.
    ///
    /// MUTANTE: tolta la condizione «è vivo» dal ripiego, la seconda metà va in
    /// rosso; tolto il ripiego, la prima.
    #[test]
    fn an_ambiguous_tab_falls_back_to_the_known_handle_when_it_is_live() {
        let tre_pannelli = vec![
            Terminal { handle: "term_a".into(), tab_id: "tab-2".into(), ..Default::default() },
            Terminal { handle: "term_b".into(), tab_id: "tab-2".into(), ..Default::default() },
            Terminal { handle: "term_c".into(), tab_id: "tab-2".into(), ..Default::default() },
        ];
        // Vivo: l'ambiguità della scheda è sciolta, e si risponde.
        assert_eq!(
            resolve_terminal_handle("tab-2", "", "term_b", &tre_pannelli),
            "term_b"
        );
        // Non più vivo: l'identità torna incerta e si tace, come prima.
        assert_eq!(
            resolve_terminal_handle("tab-2", "", "term_morto", &tre_pannelli),
            ""
        );
        // E il caso che la prima stesura di questa riparazione sbagliava, preso
        // dalla batteria e non a mente: un handle vivo che sta in un'ALTRA
        // scheda non scioglie niente — è il pannello di qualcun altro.
        let altrove = {
            let mut v = tre_pannelli.clone();
            v.push(Terminal {
                handle: "term_estraneo".into(),
                tab_id: "tab-9".into(),
                ..Default::default()
            });
            v
        };
        assert_eq!(
            resolve_terminal_handle("tab-2", "", "term_estraneo", &altrove),
            "",
            "l'handle e' vivo ma appartiene a un'altra scheda: non e' nostro"
        );
    }

    #[test]
    fn a_tab_with_two_panes_answers_for_neither() {
        // La tab reale `2ae72849` aveva due pannelli: prima si prendeva il
        // primo trovato, e un `/clear` poteva finire su quello sbagliato.
        // Come il ramo del worktree qui sotto, l'ambiguità si astiene.
        let due_pannelli = vec![
            Terminal { handle: "term_a".into(), tab_id: "tab-2".into(), ..Default::default() },
            Terminal { handle: "term_b".into(), tab_id: "tab-2".into(), ..Default::default() },
        ];
        assert_eq!(resolve_terminal_handle("tab-2", "", "", &due_pannelli), "");
    }

    #[test]
    fn the_worktree_answers_only_if_the_candidate_is_unique() {
        // `wt-2` ne ha due: chiudere quello sbagliato costa il lavoro altrui.
        assert_eq!(resolve_terminal_handle("", "wt-2", "", &three_terminals()), "");
        assert_eq!(
            resolve_terminal_handle("", "wt-1", "", &three_terminals()),
            "term_a"
        );
    }

    // ── il marcatore del successore armato: niente pannello del vicino ───────

    #[test]
    fn a_dead_recorded_handle_does_not_adopt_the_sibling_pane() {
        // IL CASO VERO: `terminal split` mette il successore nella STESSA tab
        // di chi lo arma. Se il successore muore e resta un'altra sessione da
        // sola su quella tab, la ricerca per tab la troverebbe comunque — ed è
        // esattamente il pannello del vicino che questa funzione rifiuta.
        let vicino = vec![Terminal {
            handle: "term_vicino".into(),
            tab_id: "tab-1".into(),
            ..Default::default()
        }];
        assert_eq!(
            resolve_armed_successor("tab-1", "term_successore_morto", &vicino),
            ""
        );
    }

    #[test]
    fn a_live_recorded_handle_still_resolves_by_tab() {
        // Il gemello positivo: il manico registrato è ancora fra i vivi, quindi
        // il marcatore non è scaduto e la ricerca per tab può procedere.
        let vivo = vec![Terminal {
            handle: "term_vivo".into(),
            tab_id: "tab-1".into(),
            ..Default::default()
        }];
        assert_eq!(resolve_armed_successor("tab-1", "term_vivo", &vivo), "term_vivo");
    }

    #[test]
    fn an_empty_recorded_handle_still_trusts_the_tab() {
        // I marcatori scritti quando la regex non ha trovato il manico: senza
        // niente da verificare, si torna al comportamento di sempre.
        let vivo = vec![Terminal {
            handle: "term_vivo".into(),
            tab_id: "tab-1".into(),
            ..Default::default()
        }];
        assert_eq!(resolve_armed_successor("tab-1", "", &vivo), "term_vivo");
    }

    #[test]
    fn a_dead_recorded_handle_still_gets_the_two_pane_abstention() {
        // Il manico morto non deve far scavalcare l'astensione sulla tab con
        // due pannelli: `resolve_terminal_handle` deve restare l'unica strada.
        let due_pannelli = vec![
            Terminal { handle: "term_a".into(), tab_id: "tab-2".into(), ..Default::default() },
            Terminal { handle: "term_b".into(), tab_id: "tab-2".into(), ..Default::default() },
        ];
        assert_eq!(resolve_armed_successor("tab-2", "term_morto", &due_pannelli), "");
    }

    #[test]
    fn both_response_shapes_are_accepted() {
        // Annidata sotto `result`, e già come lista: la forma è cambiata una
        // volta, e leggerne una sola è il difetto che ha creato 24 sessioni.
        let annidata: serde_json::Value =
            serde_json::from_str(r#"{"result":{"terminals":[{"handle":"term_a"}]}}"#).unwrap();
        let piatta: serde_json::Value = serde_json::from_str(r#"[{"handle":"term_a"}]"#).unwrap();
        assert_eq!(Terminal::from_response(&annidata).unwrap()[0].handle, "term_a");
        assert_eq!(Terminal::from_response(&piatta).unwrap()[0].handle, "term_a");
        // Una risposta d'errore non produce pannelli inventati. Resta un
        // elenco VUOTO e non un «non lo so», perché è un oggetto senza
        // `terminals` — la stessa lettura del Python, che su un dizionario
        // scende dentro e non trova niente.
        let errore: serde_json::Value = serde_json::from_str(r#"{"ok":false}"#).unwrap();
        assert_eq!(Terminal::from_response(&errore), Some(Vec::new()));
    }

    #[test]
    fn an_unknown_shape_is_an_i_do_not_know() {
        // IL CASO CARO. `result` che non è né lista né oggetto: prima tornava
        // `Vec::new()`, e a valle un elenco vuoto vuol dire «i pannelli sono
        // morti tutti» — la chiusura si dichiarava riuscita senza chiudere
        // niente e la sessione vecchia restava viva sullo stesso albero.
        for odd in [
            r#"{"result":"oops"}"#,
            r#"{"result":42}"#,
            r#"{"result":true}"#,
            r#""solo una stringa""#,
            r#"42"#,
            r#"null"#,
        ] {
            let v: serde_json::Value = serde_json::from_str(odd).unwrap();
            assert_eq!(Terminal::from_response(&v), None, "forma: {odd}");
        }
        // E i gemelli che devono invece rispondere «zero pannelli»: la lista
        // vuota vera, e l'oggetto che dichiara zero terminali.
        for empty in [r#"[]"#, r#"{"result":{"terminals":[]}}"#, r#"{"result":{}}"#] {
            let v: serde_json::Value = serde_json::from_str(empty).unwrap();
            assert_eq!(Terminal::from_response(&v), Some(Vec::new()), "forma: {empty}");
        }
    }

    #[test]
    fn what_is_not_an_object_is_not_a_terminal() {
        // Un elemento storto si SALTA, non diventa un pannello dai campi vuoti:
        // il ramo che sceglie per worktree risponde solo se il candidato è
        // unico, quindi un fantasma in più fa tacere una risoluzione buona.
        let v: serde_json::Value =
            serde_json::from_str(r#"[1,{"handle":"term_a","worktreeId":"wt-1"},"x",null]"#).unwrap();
        let t = Terminal::from_response(&v).unwrap();
        assert_eq!(t.len(), 1, "gli elementi storti sono diventati pannelli");
        assert_eq!(resolve_terminal_handle("", "wt-1", "", &t), "term_a");
    }

    #[test]
    fn a_falsy_terminals_field_means_an_empty_list() {
        // Il Python scrive `items.get('terminals') or []`, che non è un
        // controllo sul tipo ma sulla verità: `null`, `0`, `""`, `[]` e `{}`
        // diventano la lista vuota, mentre un valore vero passa e viene
        // giudicato dal controllo sulla lista.
        for falsy in [r#"null"#, r#"0"#, r#""""#, r#"[]"#, r#"{}"#, r#"false"#] {
            let payload = format!(r#"{{"result":{{"terminals":{falsy}}}}}"#);
            let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(Terminal::from_response(&v), Some(Vec::new()), "falso: {falsy}");
        }
        // Vero ma non una lista: qui il Python non lo sostituisce, e il
        // controllo sulla lista risponde «non lo so».
        for truthy in [r#""x""#, r#"7"#, r#"{"a":1}"#, r#"true"#] {
            let payload = format!(r#"{{"result":{{"terminals":{truthy}}}}}"#);
            let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(Terminal::from_response(&v), None, "vero non-lista: {truthy}");
        }
    }

    #[test]
    fn an_empty_list_makes_nobody_answer() {
        assert_eq!(resolve_terminal_handle("tab-1", "wt-1", "term_a", &[]), "");
    }

    #[test]
    fn orca_fields_are_read_by_the_name_they_actually_have() {
        // `tabId` e `worktreeId` arrivano in camelCase: sbagliare la rinomina
        // darebbe stringhe vuote senza errori, e la risoluzione tacerebbe sempre.
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"result":{"terminals":[{"handle":"term_x","tabId":"tab-x","worktreeId":"wt-x","title":"ignorato"}]}}"#,
        )
        .unwrap();
        let t = Terminal::from_response(&raw).unwrap();
        assert_eq!(t[0].tab_id, "tab-x");
        assert_eq!(t[0].worktree_id, "wt-x");
        assert_eq!(resolve_terminal_handle("tab-x", "", "", &t), "term_x");
    }

    #[test]
    fn every_model_gets_its_own_budget() {
        assert_eq!(quality_budget("claude-opus-4-8"), 200_000);
        assert_eq!(quality_budget("claude-opus-5"), 500_000);
        assert_eq!(quality_budget("claude-sonnet-5"), 400_000);
        assert_eq!(quality_budget("claude-haiku-4-5-20251001"), 150_000);
    }

    #[test]
    fn the_fragments_are_disjoint_which_is_why_the_order_does_not_matter() {
        // Il vincolo vero non è «il più specifico prima», è che nessun model-id
        // contenga due frammenti: finché vale, l'ordine è indifferente. Provato
        // qui invece che nel commento, perché un frammento generico aggiunto
        // domani lo romperebbe in silenzio e nessun altro caso se ne accorge.
        for (frammento, _) in MODEL_BUDGET {
            let altri: Vec<_> = MODEL_BUDGET
                .iter()
                .filter(|(f, _)| f != frammento && frammento.contains(*f))
                .collect();
            assert!(
                altri.is_empty(),
                "{frammento} ne contiene un altro: ora l'ordine conta, {altri:?}"
            );
        }
    }

    #[test]
    fn an_unknown_model_falls_back_low() {
        assert_eq!(quality_budget("gpt-9"), DEFAULT_BUDGET);
        assert_eq!(quality_budget(""), DEFAULT_BUDGET);
    }

    #[test]
    fn the_match_is_case_insensitive() {
        assert_eq!(quality_budget("Claude-OPUS-5"), 500_000);
    }

    #[test]
    fn the_last_non_synthetic_model_is_taken() {
        let lines = vec![
            r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#,
            r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#,
        ];
        assert_eq!(model_from_lines(&lines), "claude-opus-5");
    }

    #[test]
    fn an_unreadable_line_does_not_stop_the_search() {
        let lines = vec![
            r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#,
            r#"{"model": rotta"#,
        ];
        assert_eq!(model_from_lines(&lines), "claude-opus-5");
    }

    #[test]
    fn without_a_model_the_thresholds_say_unknown() {
        let t = thresholds_from_lines(&[]);
        assert_eq!(t.model, "sconosciuto");
        assert_eq!(t.budget, DEFAULT_BUDGET);
        assert_eq!(t.warn, 140_400);
        assert_eq!(t.require, 162_000);
    }

    #[test]
    fn the_opus_5_thresholds() {
        let lines = vec![r#"{"message":{"model":"claude-opus-5"}}"#];
        let t = thresholds_from_lines(&lines);
        assert_eq!(t.budget, 500_000);
        assert_eq!(t.warn, 390_000);
        assert_eq!(t.require, 450_000);
    }

    #[test]
    fn the_context_sums_the_three_fields() {
        let lines = vec![
            r#"{"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":190000,"cache_creation_input_tokens":5}}}"#,
        ];
        assert_eq!(context_used_from_lines(&lines), 190_015);
    }

    #[test]
    fn an_empty_usage_does_not_count_as_a_measurement() {
        let lines = vec![
            r#"{"message":{"usage":{"input_tokens":100}}}"#,
            r#"{"message":{"usage":{}}}"#,
        ];
        assert_eq!(context_used_from_lines(&lines), 100);
    }

    #[test]
    fn without_any_usage_the_context_is_zero() {
        assert_eq!(context_used_from_lines(&[r#"{"type":"user"}"#]), 0);
    }

    // ── turn_status_from_lines: «fermo» non è «libero» ────────────────────────

    fn assistant_line(content: serde_json::Value) -> String {
        serde_json::json!({"type": "assistant", "message": {"content": content}}).to_string()
    }

    #[test]
    fn a_pending_tool_use_is_a_turn_in_progress() {
        // IL CASO VERO: un `Bash` che dorme 300 secondi lascia il transcript
        // fermo su un `tool_use` senza il suo `tool_result`. `tui-idle`
        // risponde «fermo» lo stesso — è zitto, non libero.
        let line = assistant_line(serde_json::json!([
            {"type": "tool_use", "name": "Bash", "id": "t1", "input": {"command": "sleep 300"}}
        ]));
        assert_eq!(turn_status_from_lines(&[&line]), TurnStatus::InProgress);
    }

    #[test]
    fn a_final_text_reply_ends_the_turn() {
        let line = assistant_line(serde_json::json!([{"type": "text", "text": "fatto"}]));
        assert_eq!(turn_status_from_lines(&[&line]), TurnStatus::Ended);
    }

    #[test]
    fn a_trailing_user_message_is_still_in_progress() {
        // Vero o iniettato da un gancio, un `user` in coda aspetta ancora una
        // risposta: l'assistente non ha ancora parlato dopo di lui.
        let line = serde_json::json!({"type": "user", "message": {"content": "ciao"}}).to_string();
        assert_eq!(turn_status_from_lines(&[&line]), TurnStatus::InProgress);
    }

    #[test]
    fn only_the_last_record_counts() {
        // Un turno chiuso tre righe fa non basta se dopo è arrivato altro.
        let closed = assistant_line(serde_json::json!([{"type": "text", "text": "primo"}]));
        let then = assistant_line(serde_json::json!([
            {"type": "tool_use", "name": "Bash", "id": "t1", "input": {}}
        ]));
        assert_eq!(turn_status_from_lines(&[&closed, &then]), TurnStatus::InProgress);
    }

    #[test]
    fn an_unreadable_or_empty_tail_is_unknown() {
        assert_eq!(turn_status_from_lines(&[]), TurnStatus::Unknown);
        assert_eq!(turn_status_from_lines(&["", "   "]), TurnStatus::Unknown);
        assert_eq!(turn_status_from_lines(&["ent\":\"x\"}]}}"]), TurnStatus::Unknown);
        let unknown_type = serde_json::json!({"type": "summary"}).to_string();
        assert_eq!(turn_status_from_lines(&[&unknown_type]), TurnStatus::Unknown);
    }

    #[test]
    fn a_note_appended_by_a_hook_does_not_hide_the_turn_underneath() {
        // IL CASO CHE HA FERMATO LA STAFFETTA PER VENTUN ORE. I ganci appendono
        // in coda record che turni non sono, e il giudizio si fermava lì
        // dicendo «non lo so» — per sempre, perche' quella riga non cambia da
        // se'. Il turno vero e' sotto, ed e' chiuso.
        let ended = assistant_line(serde_json::json!([{"type": "text", "text": "fatto"}]));
        let notes = [
            serde_json::json!({"type": "system", "content": "hook output"}).to_string(),
            serde_json::json!({"type": "attachment", "path": "x.png"}).to_string(),
            serde_json::json!({"type": "atis-latch"}).to_string(),
            serde_json::json!({"type": "pr-link", "url": "https://x"}).to_string(),
        ];
        for note in &notes {
            assert_eq!(
                turn_status_from_lines(&[&ended, note]),
                TurnStatus::Ended,
                "un record `{note}` in coda non e' un turno"
            );
        }
        // E in fila tutte insieme, che e' la forma vera sul disco.
        let tail: Vec<&str> = std::iter::once(ended.as_str())
            .chain(notes.iter().map(|s| s.as_str()))
            .collect();
        assert_eq!(turn_status_from_lines(&tail), TurnStatus::Ended);
    }

    #[test]
    fn skipping_the_notes_does_not_declare_free_a_turn_still_running() {
        // Il verso pericoloso: scavalcare le annotazioni non deve trasformare un
        // turno vivo in un turno chiuso, altrimenti la staffetta scriverebbe
        // addosso a chi sta lavorando — che e' peggio del blocco appena tolto.
        let running = assistant_line(serde_json::json!([
            {"type": "tool_use", "name": "Bash", "id": "t1", "input": {"command": "sleep 300"}}
        ]));
        let note = serde_json::json!({"type": "system", "content": "hook"}).to_string();
        assert_eq!(turn_status_from_lines(&[&running, &note]), TurnStatus::InProgress);

        let waiting = serde_json::json!({"type": "user", "message": {"content": "ciao"}}).to_string();
        assert_eq!(turn_status_from_lines(&[&waiting, &note]), TurnStatus::InProgress);
    }

    #[test]
    fn a_truncated_line_stops_the_scan_instead_of_being_stepped_over() {
        // Una riga a meta' scrittura potrebbe essere il turno vero: scavalcarla
        // vorrebbe dire rispondere su cio' che c'era prima, cioe' su un passato.
        let ended = assistant_line(serde_json::json!([{"type": "text", "text": "fatto"}]));
        assert_eq!(
            turn_status_from_lines(&[&ended, "{\"type\":\"assis"]),
            TurnStatus::Unknown
        );
    }

    #[test]
    fn nothing_but_notes_is_still_unknown() {
        // Senza nessun turno sotto non si sa niente, e resta «non lo so».
        let notes = [
            serde_json::json!({"type": "system"}).to_string(),
            serde_json::json!({"type": "attachment"}).to_string(),
        ];
        let tail: Vec<&str> = notes.iter().map(|s| s.as_str()).collect();
        assert_eq!(turn_status_from_lines(&tail), TurnStatus::Unknown);
    }

    #[test]
    fn only_the_skill_counts_as_a_handoff() {
        let v: serde_json::Value = serde_json::json!({"skill": "handoff"});
        assert!(is_handoff_call("Skill", Some(&v)));
        // Scrivere un documento di consegna NON è aver consegnato.
        let w: serde_json::Value = serde_json::json!({"file_path": "/x/consegna-y.md"});
        assert!(!is_handoff_call("Write", Some(&w)));
        assert!(!is_handoff_call("Edit", Some(&w)));
    }

    #[test]
    fn a_different_skill_does_not_count() {
        let v: serde_json::Value = serde_json::json!({"skill": "grilling"});
        assert!(!is_handoff_call("Skill", Some(&v)));
        assert!(!is_handoff_call("Skill", None));
    }

    // ── il tetto alle sessioni ──────────────────────────────────────────────

    fn adding(live: Option<usize>) -> CapFacts {
        CapFacts {
            delta: SessionDelta::Adds,
            live,
            cap: SESSION_CAP_DEFAULT,
        }
    }

    #[test]
    fn below_the_cap_a_session_opens() {
        assert_eq!(session_cap_verdict(&adding(Some(19))), None);
        assert_eq!(session_cap_verdict(&adding(Some(0))), None);
    }

    #[test]
    fn at_the_cap_and_above_it_blocks() {
        // Il confine è chiuso a sinistra: venti è già troppo, come `>=` della
        // misura che ha scelto la soglia.
        let messaggio = session_cap_verdict(&adding(Some(20))).expect("doveva bloccare");
        assert!(messaggio.contains("20 Claude sessions"), "{messaggio}");
        assert!(messaggio.contains("cap 20"), "{messaggio}");
        assert!(session_cap_verdict(&adding(Some(64))).is_some());
    }

    #[test]
    fn a_replacement_always_passes() {
        // IL CASO CHE VALE PIÙ DI TUTTI. Con 64 sessioni vive, chi ne chiude una
        // per aprirne una deve passare: bloccarlo lascia sul posto una sessione
        // piena senza far scendere il conto di uno.
        let f = CapFacts {
            delta: SessionDelta::Replaces,
            live: Some(64),
            ..adding(None)
        };
        assert_eq!(session_cap_verdict(&f), None);
        let f = CapFacts {
            delta: SessionDelta::None,
            live: Some(64),
            ..adding(None)
        };
        assert_eq!(session_cap_verdict(&f), None);
    }

    #[test]
    fn a_failed_count_lets_it_through() {
        // Fail-open: un tetto che blocca perché non sa contare smette di frenare
        // le sessioni e comincia a frenare il lavoro.
        assert_eq!(session_cap_verdict(&adding(None)), None);
    }

    #[test]
    fn the_cap_can_be_tightened_and_widened() {
        let f = CapFacts {
            cap: 0,
            ..adding(Some(0))
        };
        assert!(
            session_cap_verdict(&f).is_some(),
            "col tetto a zero non si apre niente"
        );
        let f = CapFacts {
            cap: 1000,
            ..adding(Some(64))
        };
        assert_eq!(session_cap_verdict(&f), None);
    }

    #[test]
    fn the_message_says_how_to_get_past_it() {
        // Un freno che non dice come procedere si aggira invece che rispettarlo.
        let m = session_cap_verdict(&adding(Some(30))).unwrap();
        assert!(m.contains(REPLACEMENT_MARK), "{m}");
        assert!(m.contains("SESSION_CAP_GUARD=off"), "{m}");
    }

    #[test]
    fn the_three_known_openings_count_as_additions() {
        assert_eq!(
            session_delta("orca terminal create --command 'claude' --title x"),
            SessionDelta::Adds
        );
        assert_eq!(
            session_delta("orca worktree create --repo id:r --name n --agent claude"),
            SessionDelta::Adds
        );
        // Il gesto che la prescrizione chiede: un pannello accanto, non una tab.
        assert_eq!(
            session_delta("orca terminal split --terminal term_abc --command claude"),
            SessionDelta::Adds
        );
        // Spazi ripetuti e flag globali in mezzo non nascondono il gesto.
        assert_eq!(
            session_delta("orca   terminal   create --command \"claude 'x'\""),
            SessionDelta::Adds
        );
    }

    #[test]
    fn a_shell_is_not_a_session() {
        // Le due shell che `--setup run` lascia in ogni albero nuovo: contarle
        // spegnerebbe il meccanismo senza che nessuna sessione sia stata aperta.
        assert_eq!(
            session_delta("orca worktree create --repo id:r --name n --setup run"),
            SessionDelta::None
        );
        assert_eq!(
            session_delta("orca terminal create --command 'npm run dev'"),
            SessionDelta::None
        );
        // Vale anche per il pannello: si divide un terminale anche per un server.
        assert_eq!(
            session_delta("orca terminal split --terminal term_abc --direction horizontal"),
            SessionDelta::None
        );
        assert_eq!(
            session_delta("orca terminal list --json"),
            SessionDelta::None
        );
        assert_eq!(
            session_delta("orca worktree rm --name n"),
            SessionDelta::None
        );
    }

    #[test]
    fn the_config_directory_opens_nothing() {
        // `.claude` contiene «claude», e ogni comando eseguito qui dentro lo
        // porta nel percorso: un `contains` avrebbe bloccato il lavoro normale.
        assert_eq!(
            session_delta("orca terminal create --command 'bash /Users/theo/.claude/scripts/x.sh'"),
            SessionDelta::None
        );
        assert_eq!(
            session_delta("orca terminal create --command 'claude-hooks --check'"),
            SessionDelta::None
        );
    }

    #[test]
    fn outside_orca_nothing_is_judged() {
        // Un `claude -p` lanciato a mano non passa da qui: questo freno guarda i
        // gesti che aprono una scheda, e dirlo è meglio che fingere di coprirlo.
        assert_eq!(session_delta("claude -p 'ciao'"), SessionDelta::None);
        assert_eq!(
            session_delta("git commit -m 'terminal create'"),
            SessionDelta::None
        );
    }

    #[test]
    fn the_replacement_declaration_downgrades_the_addition() {
        assert_eq!(
            session_delta("SESSION_REPLACES=1 orca terminal create --command claude"),
            SessionDelta::Replaces
        );
        // E su un comando che non apre niente resta «niente»: la dichiarazione
        // non inventa una sostituzione dove non c'è un'apertura.
        assert_eq!(
            session_delta("SESSION_REPLACES=1 orca terminal list"),
            SessionDelta::None
        );
    }
}
