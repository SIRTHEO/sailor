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
    pub fn from_response(value: &serde_json::Value) -> Vec<Terminal> {
        let inner = value.get("result").unwrap_or(value);
        let items = inner
            .get("terminals")
            .and_then(|x| x.as_array())
            .or_else(|| inner.as_array());
        let Some(items) = items else {
            return Vec::new();
        };
        items
            .iter()
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
            .collect()
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
        return terminals
            .iter()
            .find(|t| t.tab_id == tab_id)
            .map(|t| t.handle.clone())
            .unwrap_or_default();
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
    /// `None` = l'elenco dei pannelli **non si è potuto leggere**. Non è «sono
    /// morti tutti»: la differenza vale un registro di sessioni cancellato ogni
    /// minuto, ed è già costata 276 giri tutti «riusciti».
    pub live_handles: Option<&'a [String]>,
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
pub fn gruppi(n: u64) -> String {
    let cifre = n.to_string();
    let mut out = String::new();
    for (i, c) in cifre.chars().enumerate() {
        if i > 0 && (cifre.len() - i) % 3 == 0 {
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
    if !live.iter().any(|h| h == f.handle) {
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
    if !f.armed_successor.is_empty() && live.iter().any(|h| h == f.armed_successor) {
        let short: String = f.armed_successor.chars().take(13).collect();
        return (Action::Retire, format!("{piena}; successore gia' vivo ({short})"));
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
/// sbagliare è lasciar passare. Contano due gesti soli — una scheda Orca che
/// avvia `claude`, e una copia di lavoro creata con un agente dentro. Un
/// `terminal create` che avvia una shell **non** è una sessione: sono le due
/// shell che `--setup run` lascia in ogni albero nuovo, e contarle spegnerebbe
/// il meccanismo senza che nessuna sessione sia stata aperta.
pub fn session_delta(command: &str) -> SessionDelta {
    let c = squeeze(command);
    if !word_at(&c, "orca") {
        return SessionDelta::None;
    }
    let with_agent = word_at(&c, "--agent");
    let opens = (c.contains("terminal create") && (with_agent || word_at(&c, "claude")))
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

    fn soglie_opus5() -> Thresholds {
        Thresholds {
            model: "claude-opus-5".into(),
            budget: 500_000,
            warn: 390_000,
            require: 450_000,
        }
    }

    fn sessione_piena<'a>(
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
    fn consegnata_e_piena_si_rigenera() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let (a, why) = evaluate(&sessione_piena(&live, &t));
        assert_eq!(a, Action::Regenerate);
        assert!(why.contains("96% del budget claude-opus-5"), "{why}");
    }

    #[test]
    fn sotto_soglia_e_non_deliberata_si_salta() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.used = 363_283; // il caso vero del 18/08/2026: 67% del budget
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("sotto soglia"), "{why}");
    }

    #[test]
    fn sotto_soglia_ma_deliberata_passa_il_testimone() {
        // Chi consegna con il contesto largo dichiara chiuso l'ambito. Senza
        // questo, la sua consegna resta sul disco e non la raccoglie nessuno.
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.used = 363_283;
        f.handoff_deliberate = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Regenerate);
        assert!(why.contains("di proposito"), "{why}");
        assert!(why.contains("ambito chiuso"), "{why}");
    }

    #[test]
    fn deliberata_non_scavalca_chi_ha_lavorato_dopo() {
        // La guardia che protegge una sessione ancora al lavoro non si allenta:
        // consegnare di proposito non autorizza a chiudere chi sta lavorando.
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.used = 363_283;
        f.handoff_deliberate = true;
        f.worked_after_handoff = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("lavoro dopo la consegna"), "{why}");
    }

    #[test]
    fn la_meta_esatta_arrotonda_al_pari() {
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
    fn la_percentuale_nel_motivo_usa_l_arrotondamento_al_pari() {
        let live = vec!["term_x".to_string()];
        let t = Thresholds {
            model: "claude-opus-4-8".into(),
            budget: 200_000,
            warn: 156_000,
            require: 180_000,
        };
        let mut f = sessione_piena(&live, &t);
        f.used = 181_000; // esattamente 90,5%
        let (_, why) = evaluate(&f);
        assert!(why.contains("90% del budget"), "{why}");
    }

    #[test]
    fn un_elenco_illeggibile_non_e_una_strage() {
        // `None` contro lista vuota: è la distinzione che valeva il registro
        // delle sessioni, cancellato ogni minuto per 276 giri «riusciti».
        let t = soglie_opus5();
        let live: Vec<String> = vec![];
        let mut f = sessione_piena(&live, &t);
        f.live_handles = None;
        assert_eq!(evaluate(&f).0, Action::Skip);
        f.live_handles = Some(&live);
        assert_eq!(evaluate(&f).0, Action::Clean);
    }

    #[test]
    fn il_successore_gia_armato_e_vivo_ferma_tutto() {
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.armed_successor = "term_succ";
        let (a, why) = evaluate(&f);
        // NON `Skip`: la vecchia va chiusa comunque, solo senza aprirne un'altra.
        // Con `Skip` restava viva accanto al successore e continuava a lavorare
        // su una consegna che invecchiava — nove ore e mezza, misurate.
        assert_eq!(a, Action::Retire);
        assert!(why.contains("successore gia' vivo"), "{why}");
    }

    #[test]
    fn un_lavoro_arrivato_dopo_la_consegna_ferma_la_rigenerazione() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.worked_after_handoff = true;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip, "ha chiuso una sessione che sta lavorando");
        assert!(why.contains("dopo la consegna"), "{why}");
    }

    #[test]
    fn e_lo_ferma_anche_quando_il_successore_e_gia_vivo() {
        // L'ORDINE È IL COMPORTAMENTO. Anche il congedo chiude: se questa
        // guardia stesse dopo quella del successore, una sessione con un
        // successore già armato verrebbe chiusa mentre lavora — che è
        // esattamente il caso del 17/08/2026, dove il successore c'era.
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.armed_successor = "term_succ";
        f.worked_after_handoff = true;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn un_successore_vivo_non_chiude_una_sessione_che_non_ha_consegnato() {
        // Il freno vale solo sulla creazione. Una sessione senza consegna non si
        // chiude comunque: il successore che c'è è di qualcun altro, o è vecchio.
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.armed_successor = "term_succ";
        f.handoff_done = false;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn un_successore_vivo_non_chiude_una_sessione_ancora_vuota() {
        let live = vec!["term_x".to_string(), "term_succ".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.armed_successor = "term_succ";
        f.used = 10_000;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn un_successore_morto_non_blocca_per_sempre() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.armed_successor = "term_sparito";
        assert_eq!(evaluate(&f).0, Action::Regenerate);
    }

    #[test]
    fn sotto_soglia_non_si_tocca() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.used = 100_000;
        let (a, why) = evaluate(&f);
        assert_eq!(a, Action::Skip);
        assert!(why.contains("sotto soglia (100000 < 450000"), "{why}");
    }

    #[test]
    fn senza_consegna_non_si_rigenera_mai() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.handoff_done = false;
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    #[test]
    fn il_raffreddamento_batte_la_soglia() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.in_cooldown = true;
        assert_eq!(evaluate(&f), (Action::Skip, "cooldown".to_string()));
    }

    #[test]
    fn un_record_incompleto_non_decide_niente() {
        let live = vec!["term_x".to_string()];
        let t = soglie_opus5();
        let mut f = sessione_piena(&live, &t);
        f.handle = "";
        assert_eq!(evaluate(&f).0, Action::Skip);
    }

    fn tre_pannelli() -> Vec<Terminal> {
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
    fn la_tab_ritrova_l_handle_rinato() {
        // Il caso vero: l'handle salvato è morto, la tab no.
        assert_eq!(
            resolve_terminal_handle("tab-1", "", "term_morto", &tre_pannelli()),
            "term_a"
        );
    }

    #[test]
    fn una_tab_sparita_non_risponde_per_un_altra() {
        assert_eq!(
            resolve_terminal_handle("tab-9", "", "term_a", &tre_pannelli()),
            ""
        );
    }

    #[test]
    fn senza_tab_l_handle_noto_vale_solo_se_vivo() {
        assert_eq!(
            resolve_terminal_handle("", "", "term_b", &tre_pannelli()),
            "term_b"
        );
        assert_eq!(
            resolve_terminal_handle("", "", "term_morto", &tre_pannelli()),
            ""
        );
    }

    #[test]
    fn il_worktree_risponde_solo_se_il_candidato_e_unico() {
        // `wt-2` ne ha due: chiudere quello sbagliato costa il lavoro altrui.
        assert_eq!(resolve_terminal_handle("", "wt-2", "", &tre_pannelli()), "");
        assert_eq!(
            resolve_terminal_handle("", "wt-1", "", &tre_pannelli()),
            "term_a"
        );
    }

    #[test]
    fn si_accettano_entrambe_le_forme_della_risposta() {
        // Annidata sotto `result`, e già come lista: la forma è cambiata una
        // volta, e leggerne una sola è il difetto che ha creato 24 sessioni.
        let annidata: serde_json::Value =
            serde_json::from_str(r#"{"result":{"terminals":[{"handle":"term_a"}]}}"#).unwrap();
        let piatta: serde_json::Value = serde_json::from_str(r#"[{"handle":"term_a"}]"#).unwrap();
        assert_eq!(Terminal::from_response(&annidata)[0].handle, "term_a");
        assert_eq!(Terminal::from_response(&piatta)[0].handle, "term_a");
        // Una risposta d'errore non produce pannelli inventati.
        let errore: serde_json::Value = serde_json::from_str(r#"{"ok":false}"#).unwrap();
        assert!(Terminal::from_response(&errore).is_empty());
    }

    #[test]
    fn un_elenco_vuoto_non_fa_rispondere_nessuno() {
        assert_eq!(resolve_terminal_handle("tab-1", "wt-1", "term_a", &[]), "");
    }

    #[test]
    fn i_campi_di_orca_si_leggono_col_nome_che_hanno() {
        // `tabId` e `worktreeId` arrivano in camelCase: sbagliare la rinomina
        // darebbe stringhe vuote senza errori, e la risoluzione tacerebbe sempre.
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"result":{"terminals":[{"handle":"term_x","tabId":"tab-x","worktreeId":"wt-x","title":"ignorato"}]}}"#,
        )
        .unwrap();
        let t = Terminal::from_response(&raw);
        assert_eq!(t[0].tab_id, "tab-x");
        assert_eq!(t[0].worktree_id, "wt-x");
        assert_eq!(resolve_terminal_handle("tab-x", "", "", &t), "term_x");
    }

    #[test]
    fn ogni_modello_prende_il_suo_budget() {
        assert_eq!(quality_budget("claude-opus-4-8"), 200_000);
        assert_eq!(quality_budget("claude-opus-5"), 500_000);
        assert_eq!(quality_budget("claude-sonnet-5"), 400_000);
        assert_eq!(quality_budget("claude-haiku-4-5-20251001"), 150_000);
    }

    #[test]
    fn i_frammenti_sono_disgiunti_e_per_questo_l_ordine_non_conta() {
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
    fn un_modello_sconosciuto_taglia_basso() {
        assert_eq!(quality_budget("gpt-9"), DEFAULT_BUDGET);
        assert_eq!(quality_budget(""), DEFAULT_BUDGET);
    }

    #[test]
    fn il_confronto_e_insensibile_alle_maiuscole() {
        assert_eq!(quality_budget("Claude-OPUS-5"), 500_000);
    }

    #[test]
    fn si_prende_l_ultimo_modello_non_sintetico() {
        let lines = vec![
            r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#,
            r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#,
        ];
        assert_eq!(model_from_lines(&lines), "claude-opus-5");
    }

    #[test]
    fn una_riga_illeggibile_non_ferma_la_ricerca() {
        let lines = vec![
            r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#,
            r#"{"model": rotta"#,
        ];
        assert_eq!(model_from_lines(&lines), "claude-opus-5");
    }

    #[test]
    fn senza_modello_le_soglie_dicono_sconosciuto() {
        let t = thresholds_from_lines(&[]);
        assert_eq!(t.model, "sconosciuto");
        assert_eq!(t.budget, DEFAULT_BUDGET);
        assert_eq!(t.warn, 140_400);
        assert_eq!(t.require, 162_000);
    }

    #[test]
    fn le_soglie_di_opus_5() {
        let lines = vec![r#"{"message":{"model":"claude-opus-5"}}"#];
        let t = thresholds_from_lines(&lines);
        assert_eq!(t.budget, 500_000);
        assert_eq!(t.warn, 390_000);
        assert_eq!(t.require, 450_000);
    }

    #[test]
    fn il_contesto_somma_i_tre_campi() {
        let lines = vec![
            r#"{"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":190000,"cache_creation_input_tokens":5}}}"#,
        ];
        assert_eq!(context_used_from_lines(&lines), 190_015);
    }

    #[test]
    fn un_usage_vuoto_non_conta_come_misura() {
        let lines = vec![
            r#"{"message":{"usage":{"input_tokens":100}}}"#,
            r#"{"message":{"usage":{}}}"#,
        ];
        assert_eq!(context_used_from_lines(&lines), 100);
    }

    #[test]
    fn senza_usage_il_contesto_e_zero() {
        assert_eq!(context_used_from_lines(&[r#"{"type":"user"}"#]), 0);
    }

    #[test]
    fn solo_la_skill_conta_come_consegna() {
        let v: serde_json::Value = serde_json::json!({"skill": "handoff"});
        assert!(is_handoff_call("Skill", Some(&v)));
        // Scrivere un documento di consegna NON è aver consegnato.
        let w: serde_json::Value = serde_json::json!({"file_path": "/x/consegna-y.md"});
        assert!(!is_handoff_call("Write", Some(&w)));
        assert!(!is_handoff_call("Edit", Some(&w)));
    }

    #[test]
    fn una_skill_diversa_non_conta() {
        let v: serde_json::Value = serde_json::json!({"skill": "grilling"});
        assert!(!is_handoff_call("Skill", Some(&v)));
        assert!(!is_handoff_call("Skill", None));
    }

    // ── il tetto alle sessioni ──────────────────────────────────────────────

    fn aggiunta(live: Option<usize>) -> CapFacts {
        CapFacts {
            delta: SessionDelta::Adds,
            live,
            cap: SESSION_CAP_DEFAULT,
        }
    }

    #[test]
    fn sotto_soglia_si_apre() {
        assert_eq!(session_cap_verdict(&aggiunta(Some(19))), None);
        assert_eq!(session_cap_verdict(&aggiunta(Some(0))), None);
    }

    #[test]
    fn alla_soglia_e_sopra_si_blocca() {
        // Il confine è chiuso a sinistra: venti è già troppo, come `>=` della
        // misura che ha scelto la soglia.
        let messaggio = session_cap_verdict(&aggiunta(Some(20))).expect("doveva bloccare");
        assert!(messaggio.contains("20 Claude sessions"), "{messaggio}");
        assert!(messaggio.contains("cap 20"), "{messaggio}");
        assert!(session_cap_verdict(&aggiunta(Some(64))).is_some());
    }

    #[test]
    fn una_sostituzione_passa_sempre() {
        // IL CASO CHE VALE PIÙ DI TUTTI. Con 64 sessioni vive, chi ne chiude una
        // per aprirne una deve passare: bloccarlo lascia sul posto una sessione
        // piena senza far scendere il conto di uno.
        let f = CapFacts {
            delta: SessionDelta::Replaces,
            live: Some(64),
            ..aggiunta(None)
        };
        assert_eq!(session_cap_verdict(&f), None);
        let f = CapFacts {
            delta: SessionDelta::None,
            live: Some(64),
            ..aggiunta(None)
        };
        assert_eq!(session_cap_verdict(&f), None);
    }

    #[test]
    fn un_conteggio_fallito_lascia_passare() {
        // Fail-open: un tetto che blocca perché non sa contare smette di frenare
        // le sessioni e comincia a frenare il lavoro.
        assert_eq!(session_cap_verdict(&aggiunta(None)), None);
    }

    #[test]
    fn il_tetto_si_puo_stringere_e_allargare() {
        let f = CapFacts {
            cap: 0,
            ..aggiunta(Some(0))
        };
        assert!(
            session_cap_verdict(&f).is_some(),
            "col tetto a zero non si apre niente"
        );
        let f = CapFacts {
            cap: 1000,
            ..aggiunta(Some(64))
        };
        assert_eq!(session_cap_verdict(&f), None);
    }

    #[test]
    fn il_messaggio_dice_come_uscirne() {
        // Un freno che non dice come procedere si aggira invece che rispettarlo.
        let m = session_cap_verdict(&aggiunta(Some(30))).unwrap();
        assert!(m.contains(REPLACEMENT_MARK), "{m}");
        assert!(m.contains("SESSION_CAP_GUARD=off"), "{m}");
    }

    #[test]
    fn le_due_aperture_note_contano_come_aggiunte() {
        assert_eq!(
            session_delta("orca terminal create --command 'claude' --title x"),
            SessionDelta::Adds
        );
        assert_eq!(
            session_delta("orca worktree create --repo id:r --name n --agent claude"),
            SessionDelta::Adds
        );
        // Spazi ripetuti e flag globali in mezzo non nascondono il gesto.
        assert_eq!(
            session_delta("orca   terminal   create --command \"claude 'x'\""),
            SessionDelta::Adds
        );
    }

    #[test]
    fn una_shell_non_e_una_sessione() {
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
    fn la_cartella_della_configurazione_non_apre_niente() {
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
    fn fuori_da_orca_non_si_giudica() {
        // Un `claude -p` lanciato a mano non passa da qui: questo freno guarda i
        // gesti che aprono una scheda, e dirlo è meglio che fingere di coprirlo.
        assert_eq!(session_delta("claude -p 'ciao'"), SessionDelta::None);
        assert_eq!(
            session_delta("git commit -m 'terminal create'"),
            SessionDelta::None
        );
    }

    #[test]
    fn la_dichiarazione_di_sostituzione_declassa_l_aggiunta() {
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
