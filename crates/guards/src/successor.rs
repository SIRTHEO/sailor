//! I freni che decidono se una consegna appena scritta apre la sessione dopo.
//!
//! Porta della parte pura di `skills/hooks/handoff-arms-successor.py`. Il gancio
//! gira su `PostToolUse Write|Edit`: quando riconosce un documento di consegna
//! apre una scheda col mandato dentro, che parte da sola dopo trenta secondi.
//!
//! I FRENI STANNO SUL DISCO, NON NEL PROMPT. Un freno scritto nel testo del
//! mandato è un freno che il modello può decidere di non applicare. Questi sono
//! condizioni che si calcolano prima di aprire qualsiasi cosa, e il 14/08/2026
//! sono stati provati sabotandoli uno per uno — prima ce n'era uno solo sotto
//! prova su quattro, e le prove dicevano comunque «8/8».
//!
//! PERCHÉ LA SCHEDA PARTE DA SOLA. Prima aspettava un Invio. Sulla carta costava
//! un gesto; nei fatti quel gesto non lo faceva nessuno — misurate 21 schede «in
//! attesa di INVIO» ancora vive su 191. Un freno che nessuno rilascia non è
//! prudenza, è spegnimento con l'aggravante di sembrare acceso.

use regex::Regex;
use std::sync::OnceLock;

/// Finestra oraria in cui è lecito aprire una sessione. Tre sessioni nate alle
/// tre di notte le scopri dal conto, non dal lavoro che hanno fatto.
pub const HOUR_MIN: u32 = 8;
pub const HOUR_MAX: u32 = 21;

/// Variabile ereditata dalla scheda figlia: una sessione nata da una consegna
/// non ne arma un'altra. È l'unico freno che sopravvive al fatto che il figlio
/// sia un processo diverso.
pub const GENERATION_ENV: &str = "CLAUDE_NATO_DA_CONSEGNA";

/// I marcatori che Orca antepone al titolo di un pannello con un agente dentro.
///
/// Elenco **chiuso** di cosa conta come sessione, non di cosa si esclude:
/// contando tutti i pannelli rientravano nel tetto le due shell che
/// `--setup run` lascia in ogni albero nuovo, e due shell inerti avrebbero
/// spezzato la catena delle consegne senza che nessuna sessione fosse aperta.
pub const AGENT_MARKS: &[char] = &['✳', '◑', '◐', '⏳'];

fn name_patterns() -> &'static [Regex; 2] {
    static RE: OnceLock<[Regex; 2]> = OnceLock::new();
    RE.get_or_init(|| {
        [
            Regex::new(r"/consegna-[^/]+\.md$").unwrap(),
            Regex::new(r"/handoff-[^/]+\.md$").unwrap(),
        ]
    })
}

fn in_memory() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Il Python usa `(?!MEMORY\.md$)`, che il crate `regex` non ha: il lookahead
    // negativo è sostituito dal controllo esplicito in `is_handoff_doc`, dove è
    // anche più leggibile di quanto fosse nella regex.
    RE.get_or_init(|| Regex::new(r"/memory/[^/]*\d{2}-\d{2}-\d{4}[^/]*\.md$").unwrap())
}

/// Sta in `memory/` e non è l'indice — senza pretendere niente dal nome.
fn in_memory_dir() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/memory/[^/]+\.md$").unwrap())
}

/// Le due intestazioni che la skill `handoff` prescrive nel corpo.
///
/// Sono il criterio che non passa dal nome del file, e servono perché il nome
/// non dice più niente: il comando `/handoff` ordina di AGGIORNARE la consegna
/// aperta sullo stesso filone (`commands/handoff.md:38`) e di scriverla in
/// `<memory-dir>/<slug>.md` (`:44`) — uno slug tematico, senza prefisso e senza
/// data. Le sezioni invece le prescrive una per una (`:57-65`), e sono ciò che
/// distingue una consegna da una nota di progetto.
fn body_marks() -> &'static [Regex; 2] {
    static RE: OnceLock<[Regex; 2]> = OnceLock::new();
    RE.get_or_init(|| {
        [
            Regex::new(r"(?mi)^#+\s*stato\b").unwrap(),
            Regex::new(r"(?mi)^#+\s*prossim[io]\s+pass").unwrap(),
        ]
    })
}

/// Il corpo ha la forma di una consegna: **entrambe** le sezioni, non una.
///
/// Entrambe perché «Stato» da solo compare in mezze note di progetto, mentre la
/// coppia no: misurata il 17/08/2026 su 415 documenti in `memory/`, seleziona 18
/// file — le consegne note, più quella che quel giorno era rimasta a terra — e
/// **zero** falsi positivi sui 187 senza `type: project`.
pub fn body_says_handoff(text: &str) -> bool {
    body_marks().iter().all(|re| re.is_match(text))
}

/// Il blocco di frontmatter: da `---` al `---` che lo chiude.
///
/// Il frontmatter si guarda solo in testa — `type: project` citato a metà corpo,
/// in una consegna che parla di consegne, non deve valere come dichiarazione —
/// ma «in testa» è il blocco, non una finestra di byte. Qui c'erano i primi 400,
/// ereditati dal Python, e il 17/08/2026 sono bastati a rendere il gancio cieco
/// una seconda volta sullo stesso documento: cresciuto a 32 KB, con una
/// `description` più lunga, `type: project` è finito al byte **419** e la
/// consegna ha smesso di essere riconosciuta un'ora dopo che l'avevamo fatta
/// riconoscere. Una soglia più alta avrebbe solo spostato il giorno del guasto.
///
/// Il tetto resta contro un file che comincia con `---` e non lo chiude più:
/// senza, un documento senza frontmatter farebbe scorrere tutto il testo.
fn front(text: &str) -> &str {
    const CAP: usize = 4096;
    let Some(rest) = text.strip_prefix("---") else {
        return "";
    };
    let mut fine = CAP.min(rest.len());
    while fine > 0 && !rest.is_char_boundary(fine) {
        fine -= 1;
    }
    let head = &rest[..fine];
    match head.find("\n---") {
        Some(chiusura) => &head[..chiusura],
        None => head,
    }
}

/// Il nome dice già che è una consegna?
pub fn name_says_handoff(path: &str) -> bool {
    name_patterns().iter().any(|re| re.is_match(path))
}

/// Sta in `memory/`, ha una data nel nome, e non è l'indice?
///
/// La data serve: `orca-usciere-tab.md` è `type: project` ma descrive uno
/// strumento, non una sessione da riprendere, e col solo controllo sul
/// frontmatter armava una sessione per sbaglio.
/// L'esclusione esplicita di `MEMORY.md` è **ridondante** finché la regex
/// pretende una data nel nome, e `MEMORY.md` non ne ha: verificato per mutazione
/// il 17/08/2026, toglierla non cambia una sola risposta su 482 casi. Resta
/// perché il Python la porta (ha lo stesso doppione, come lookahead negativo) e
/// perché è la difesa che regge se un domani la data smette di essere richiesta —
/// l'indice che arma una scheda a ogni aggiornamento è metà del difetto delle
/// due schede di quel giorno.
pub fn is_dated_memory(path: &str) -> bool {
    if !in_memory().is_match(path) {
        return false;
    }
    !path.ends_with("/memory/MEMORY.md")
}

/// È un documento di consegna? `text` è il contenuto del file.
///
/// Il nome da solo non basta e il frontmatter da solo nemmeno. La skill
/// `handoff` di questo progetto scrive un file dal nome libero dentro `memory/`,
/// dove finisce ogni sorta di memoria: solo quelle marcate `type: project` sono
/// consegne. Misurato il 13/08/2026 subito dopo aver acceso il gancio — su una
/// consegna vera rispondeva «non è una consegna», quindi era acceso e cieco.
///
/// LA DATA NEL NOME NON È PIÙ L'UNICA PORTA, dal 17/08/2026. Chiedere una data a
/// un file che la skill battezza con uno slug tematico rendeva il gancio cieco
/// sul caso che la skill stessa rende normale — aggiornare la consegna aperta.
/// Quel giorno una sessione all'88% del budget ha consegnato in
/// `sezioni-cv-campi-separati-o-testo.md` e non è partito niente: il gancio è
/// uscito prima di ogni freno, senza lasciare una riga di registro. Sul disco di
/// quel giorno erano invisibili 202 dei 227 documenti `type: project`.
///
/// Nel dubbio il gancio deve tacere: una consegna non raccolta costa
/// un'apertura a mano, una scheda aperta a sproposito costa fiducia — e a quel
/// punto lo si spegne del tutto. Per questo la porta nuova non allarga a tutto
/// `type: project`: pretende anche la forma del corpo.
pub fn is_handoff_doc(path: &str, text: Option<&str>) -> bool {
    if name_says_handoff(path) {
        return true;
    }
    if !in_memory_dir().is_match(path) || path.ends_with("/memory/MEMORY.md") {
        return false;
    }
    let Some(text) = text else { return false };
    if !front(text).contains("type: project") {
        return false;
    }
    is_dated_memory(path) || body_says_handoff(text)
}

/// La sessione è abbastanza avanti da star chiudendo, e non a metà lavoro?
///
/// `warn` è la soglia d'avviso (78% del budget), non quella d'obbligo (90%): le
/// due si separano su casi veri, e il test accanto li porta entrambi. Un consumo
/// a zero è «non misurato», non «vuoto», e non autorizza niente.
pub fn is_full_enough(used: u64, warn: u64) -> bool {
    used > 0 && used >= warn
}

/// L'ora sta nella finestra in cui è lecito aprire una sessione?
///
/// Sta in una funzione sua perché dentro un `if` che legge l'orologio non era
/// provabile, e infatti non era provata: le prove interne dicevano 8/8 coprendo
/// un freno su quattro. Un freno non provato conta come assente.
pub fn within_hours(hour: u32) -> bool {
    (HOUR_MIN..HOUR_MAX).contains(&hour)
}

/// Quanti fra questi pannelli hanno un agente dentro e stanno in `root`.
///
/// Prende la risposta grezza di `orca terminal list --json` per non dipendere da
/// una struttura in più: qui servono due campi, e la lista arriva o annidata
/// sotto `result.terminals` o già piatta.
pub fn count_agents(response: &serde_json::Value, root: &str) -> usize {
    let inner = response.get("result").unwrap_or(response);
    let items = inner
        .get("terminals")
        .and_then(|x| x.as_array())
        .or_else(|| inner.as_array());
    let Some(items) = items else { return 0 };
    items
        .iter()
        .filter(|t| {
            let path = t.get("worktreePath").and_then(|v| v.as_str()).unwrap_or("");
            let title = t.get("title").and_then(|v| v.as_str()).unwrap_or("");
            path == root && title.chars().any(|c| AGENT_MARKS.contains(&c))
        })
        .count()
}

/// L'impronta del marcatore «per questa sessione un successore c'è già».
///
/// Il percorso è uscito dalla chiave il 17/08/2026: il titolo del freno diceva
/// già «per sessione», ma l'impronta univa sessione e percorso, quindi due
/// documenti diversi nello stesso turno armavano due successori. Chi consegna
/// scrive la consegna e poi la riga d'indice — misurate due schede aperte a un
/// minuto di distanza.
pub fn armed_fingerprint(path: &str, session: &str) -> String {
    let key = if session.is_empty() { path } else { session };
    let digest = crate::duplication::sha1(key.as_bytes());
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(16)
        .collect()
}

/// Un processo in ascolto su una porta, come lo riporta `lsof`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
    pub port: u16,
}

/// I processi in ascolto, dall'uscita `-Fpcn` di `lsof`.
///
/// Il formato è una riga per campo, con la lettera davanti: `p` apre un
/// processo, `c` lo nomina, `n` è l'indirizzo di un suo file aperto. Le porte
/// si ripetono — IPv4 e IPv6 arrivano come due righe uguali — e qui si contano
/// una volta sola, altrimenti il mandato elencherebbe due volte lo stesso
/// server.
pub fn parse_listeners(out: &str) -> Vec<Listener> {
    let mut found: Vec<Listener> = Vec::new();
    let (mut pid, mut command) = (0u32, String::new());
    for line in out.lines() {
        let (tag, rest) = line.split_at(line.char_indices().nth(1).map_or(0, |(i, _)| i));
        match tag {
            "p" => {
                pid = rest.parse().unwrap_or(0);
                command.clear();
            }
            "c" => command = rest.to_string(),
            "n" => {
                // `*:3040` e `127.0.0.1:3040` finiscono nella stessa porta.
                let Some(port) = rest.rsplit(':').next().and_then(|p| p.parse::<u16>().ok())
                else {
                    continue;
                };
                if pid != 0 && !found.iter().any(|l| l.pid == pid && l.port == port) {
                    found.push(Listener { pid, command: command.clone(), port });
                }
            }
            _ => {}
        }
    }
    found
}

/// La cartella di lavoro di ogni processo, dall'uscita `-d cwd -Fn` di `lsof`.
pub fn parse_cwds(out: &str) -> Vec<(u32, String)> {
    let mut pairs = Vec::new();
    let mut pid = 0u32;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix('p') {
            pid = rest.parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix('n') {
            if pid != 0 {
                pairs.push((pid, rest.to_string()));
            }
        }
    }
    pairs
}

/// Chi di quei processi sta lavorando dentro `root`.
///
/// Il confronto è sul prefisso della cartella di lavoro, non sull'uguaglianza:
/// misurato il 17/08/2026, un `mastra dev` aveva la cwd in
/// `a-service/src/mastra/public`, tre livelli sotto la radice del repo.
pub fn inherited_listeners(
    listeners: &[Listener],
    cwds: &[(u32, String)],
    root: &str,
) -> Vec<Listener> {
    if root.is_empty() {
        return Vec::new();
    }
    let mut here: Vec<Listener> = listeners
        .iter()
        .filter(|l| {
            cwds.iter()
                .any(|(pid, dir)| *pid == l.pid && (dir == root || dir.starts_with(&format!("{root}/"))))
        })
        .cloned()
        .collect();
    here.sort_by_key(|l| l.port);
    here
}

/// La clausola che dice al successore cosa ha ereditato acceso, o niente.
///
/// PERCHÉ NON DICE «SPEGNILI». Un successore che spegne il server di qualcun
/// altro fa più danno di uno che ne accende un secondo; e il guasto vero non è
/// il processo di troppo, è il minuto perso a inseguirlo. Il 17/08/2026 una
/// sessione ha scambiato per un difetto del codice un pannello che «si chiudeva
/// da solo» ed era solo il server dev caduto — e quel giorno l'inventario delle
/// porte gliel'aveva dovuto ricopiare Theo a mano nel prompt.
pub fn inherited_clause(here: &[Listener]) -> String {
    if here.is_empty() {
        return String::new();
    }
    let listed = here
        .iter()
        .map(|l| format!("porta {} ({})", l.port, l.command))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "\n\nTERZA REGOLA: in questo albero e' rimasto acceso del lavoro della \
         sessione precedente — {listed}. Non riavviarli alla cieca e non spegnerli \
         per abitudine: prima verifica che rispondano davvero \
         (`lsof -nP -iTCP:<porta> -sTCP:LISTEN`). Una seconda copia sulla stessa \
         porta fallisce, e un server caduto imita un difetto del codice."
    )
}

/// Il testo che il successore riceve come primo prompt.
///
/// Le clausole sono ricopiate apposta, e nessuna è oziosa: una sessione
/// aperta in automatico parte senza nessuno che la corregga al primo turno,
/// quindi ciò che non sta qui non lo fa. Il 13/08/2026 una sessione aperta così
/// ha risposto al bollettino del monitor pur avendo in contesto la regola che lo
/// vieta — regola ricevuta, regola non applicata.
///
/// `inherited` è vuoto quando non c'è niente acceso, e allora la terza clausola
/// non compare: un inventario vuoto scritto ogni volta insegna a saltare il
/// paragrafo, e il giorno che elenca qualcosa nessuno lo legge.
pub fn mandate(path: &str, inherited: &str) -> String {
    format!(
        "Leggi {path} e riprendi da li'.\n\n\
         PRIMA REGOLA, prima di qualunque altra cosa: se il primo messaggio che \
         ricevi e' una notifica automatica di stato (bollettino del monitor, \
         avvio sessione, salute di un servizio), NON e' il tuo compito e non si \
         risponde. Il tuo compito e' la consegna qui sopra. \
         (Questa clausola e' ricopiata apposta: il 13/08/2026 una sessione \
         aperta cosi' ha risposto al bollettino pur avendo la regola in contesto.)\
         \n\n\
         SECONDA REGOLA: prima di leggere codice o scrivere qualunque cosa, \
         `git fetch --all --prune` e verifica di essere allineato col ramo \
         d'integrazione. Su questi repo lavorano anche altri, con richieste di \
         modifica e aggiornamenti che arrivano mentre tu non c'eri: la consegna \
         che stai leggendo descrive il codice di quando e' stata scritta, non \
         quello di adesso. Se sei indietro, allineati PRIMA — e se ci sono \
         dipendenze, reinstallale.{inherited}"
    )
}

/// Perché il gancio non ha aperto niente, o che ha aperto.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Si ferma e tace: il chiamante non deve stampare nulla.
    StopQuiet(&'static str),
    /// Si ferma e lo dice: un freno muto è indistinguibile da un guasto.
    StopLoud { reason: &'static str, message: String },
    /// Via libera: si può aprire la scheda.
    Open,
}

/// I fatti che i cinque freni guardano, già raccolti.
#[derive(Debug, Default)]
pub struct ArmFacts {
    /// Questa sessione è nata a sua volta da una consegna.
    pub second_generation: bool,
    pub hour: u32,
    /// Sessioni vive in tutta la macchina. `None` se non si è potuto sapere.
    pub live_sessions: Option<usize>,
    pub session_cap: usize,
    /// Pannelli con un agente in **questo** albero. `None` se non si sa.
    pub panes_here: Option<usize>,
    pub pane_cap: usize,
    /// Un successore per questa sessione è già stato armato.
    pub already_armed: bool,
    /// La sessione è davvero al capolinea, non a metà lavoro.
    pub enough_used: bool,
}

/// L'ordine dei freni è comportamento, non stile.
///
/// La generazione viene per prima perché è l'unica che deve tacere sempre: una
/// figlia che si lamenta di non poter armare riempirebbe il contesto di ogni
/// sessione nata così. L'orario prima del carico perché costa meno di una
/// chiamata esterna. Il consumo per ultimo perché **scrive** il marcatore, e
/// scriverlo per poi fermarsi su un altro freno brucerebbe l'unica arma di
/// quella sessione senza aprire niente.
pub fn decide(f: &ArmFacts) -> Outcome {
    if f.second_generation {
        return Outcome::StopQuiet("seconda-generazione");
    }
    if !within_hours(f.hour) {
        return Outcome::StopQuiet("fuori-orario");
    }
    // SCRIVERE UNA CONSEGNA NON SIGNIFICA AVER FINITO. `/handoff` si invoca
    // anche a metà lavoro, e fino al 17/08/2026 questo bastava ad aprire il
    // successore: quel giorno, un'ora dopo aver reso il gancio capace di
    // riconoscere la consegna, una sessione l'ha aggiornata, ha continuato a
    // lavorare, e in `tautog` si sono trovate due sessioni vive sullo stesso
    // albero. Il percorso Stop questo freno ce l'aveva già; qui mancava, ed era
    // l'unica differenza fra i due inneschi.
    if !f.enough_used {
        return Outcome::StopQuiet("non-piena");
    }
    // In dubbio non si frena: un conteggio che non si è potuto leggere deve
    // smettere di dare il suo parere, non spegnere il meccanismo.
    if let Some(live) = f.live_sessions {
        if live >= f.session_cap {
            return Outcome::StopLoud {
                reason: "troppe-sessioni",
                message: format!(
                    "Consegna scritta, ma ci sono {live} sessioni vive (tetto {}): \
                     non ne apro un'altra. Il documento resta su disco.",
                    f.session_cap
                ),
            };
        }
    }
    if let Some(here) = f.panes_here {
        if here >= f.pane_cap {
            return Outcome::StopLoud {
                reason: "albero-affollato",
                message: format!(
                    "Consegna scritta, ma in questo albero di lavoro ci sono gia' \
                     {here} pannelli (tetto {}): non ne apro un altro. \
                     Il documento resta su disco.",
                    f.pane_cap
                ),
            };
        }
    }
    if f.already_armed {
        return Outcome::StopQuiet("gia-armato");
    }
    Outcome::Open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_nome_riconosce_le_due_forme_classiche() {
        assert!(name_says_handoff("/x/consegna-ganci-2026.md"));
        assert!(name_says_handoff("/x/handoff-sandbox.md"));
        assert!(!name_says_handoff("/x/consegna.txt"));
        // Senza barra davanti non è un percorso: il Python ancora al `/`.
        assert!(!name_says_handoff("consegna-x.md"));
    }

    #[test]
    fn in_memory_vuole_la_data_e_non_e_l_indice() {
        assert!(is_dated_memory("/p/memory/consegna-ganci-17-08-2026.md"));
        // `orca-usciere-tab.md` è `type: project` ma non è una sessione da
        // riprendere: senza la data nel nome armava una scheda per sbaglio.
        assert!(!is_dated_memory("/p/memory/orca-usciere-tab.md"));
        assert!(!is_dated_memory("/p/memory/MEMORY.md"));
    }

    #[test]
    fn il_frontmatter_decide_solo_dentro_memory() {
        // Nome giusto: il contenuto non serve nemmeno.
        assert!(is_handoff_doc("/x/consegna-a.md", None));
        // In memory con la data: serve `type: project`.
        let p = "/p/memory/note-17-08-2026.md";
        assert!(is_handoff_doc(p, Some("---\ntype: project\n---")));
        assert!(!is_handoff_doc(p, Some("---\ntype: reference\n---")));
        // File illeggibile: non si arma niente.
        assert!(!is_handoff_doc(p, None));
    }

    /// Il corpo di una consegna come la scrive `/handoff`, ridotto all'osso.
    fn handoff_body(extra: &str) -> String {
        format!("---\nname: x\nmetadata:\n  type: project\n---\n\n## Stato\n\nfatto.\n\n## Prossimi passi\n\n1. riprendere.\n{extra}")
    }

    #[test]
    fn il_corpo_riconosce_la_consegna_che_il_nome_non_dichiara() {
        // Il caso vero del 17/08/2026: slug tematico, nessuna data, `type:
        // project`, e le sezioni che la skill prescrive. Prima usciva `False`.
        let p = "/p/memory/sezioni-cv-campi-separati-o-testo.md";
        assert!(is_handoff_doc(p, Some(&handoff_body(""))));
    }

    #[test]
    fn una_nota_di_progetto_non_arma_niente() {
        // La difesa che la data garantiva prima, ora la fa la forma del corpo:
        // `type: project` senza le due sezioni resta una nota, non una consegna.
        let p = "/p/memory/orca-usciere-tab.md";
        assert!(!is_handoff_doc(
            p,
            Some("---\ntype: project\n---\n\n## Come funziona\n\nchiude le tab.\n")
        ));
        // Una sezione sola non basta: «Stato» da solo sta in mezze note.
        assert!(!is_handoff_doc(
            p,
            Some("---\ntype: project\n---\n\n## Stato\n\nva bene.\n")
        ));
    }

    #[test]
    fn il_frontmatter_vale_solo_in_testa() {
        // Una consegna che parla di consegne cita `type: project` nel corpo: se
        // valesse ovunque, ogni documento sui ganci diventerebbe una consegna.
        let p = "/p/memory/note-sui-ganci.md";
        let corpo = format!("{}\n\n## Stato\n\nx\n\n## Prossimi passi\n\ny\n", "-".repeat(420));
        assert!(!is_handoff_doc(p, Some(&format!("---\ntype: reference\n---{corpo}\ntype: project\n"))));
    }

    #[test]
    fn un_frontmatter_lungo_resta_frontmatter() {
        // Il caso vero del 17/08/2026: la `description` cresce, `type: project`
        // scivola al byte 419, e con la finestra fissa da 400 la consegna
        // smetteva di essere riconosciuta — un'ora dopo averla fatta riconoscere.
        let p = "/p/memory/sezioni-cv-campi-separati-o-testo.md";
        let testo = format!(
            "---\nname: x\ndescription: \"{}\"\nmetadata:\n  node_type: memory\n  type: project\n---\n\n## Stato\n\nx\n\n## Prossimi passi\n\ny\n",
            "parole ".repeat(60)
        );
        assert!(testo.find("type: project").unwrap() > 400, "il caso non riproduce");
        assert!(is_handoff_doc(p, Some(&testo)));
    }

    #[test]
    fn senza_frontmatter_non_si_dichiara_niente() {
        let p = "/p/memory/nudo.md";
        // Nessun `---` in apertura: non c'è nessun blocco da leggere.
        assert!(!is_handoff_doc(p, Some("type: project\n\n## Stato\n\n## Prossimi passi\n")));
        // Aperto e mai chiuso: si legge fino al tetto e non si scorre il file.
        let mai_chiuso = format!("---\n{}\ntype: project\n", "x\n".repeat(4000));
        assert!(!is_handoff_doc(p, Some(&mai_chiuso)));
    }

    #[test]
    fn fuori_da_memory_il_corpo_non_conta() {
        // Le sezioni giuste in un file qualunque del repo non aprono niente.
        assert!(!is_handoff_doc("/repo/docs/piano.md", Some(&handoff_body(""))));
    }

    #[test]
    fn l_indice_non_e_mai_una_consegna() {
        // MEMORY.md porta `type: project` in ogni riga che cita, e senza questa
        // esclusione ogni aggiornamento dell'indice aprirebbe una scheda — che è
        // metà del difetto delle due schede del 17/08/2026.
        assert!(!is_handoff_doc(
            "/p/memory/MEMORY.md",
            Some("type: project")
        ));
    }

    /// L'uscita vera di `lsof -nP -iTCP -sTCP:LISTEN -Fpcn` su questa macchina,
    /// ridotta: un processo con la stessa porta due volte (IPv4 e IPv6), uno con
    /// due porte diverse, e uno che non ascolta niente di interessante.
    const LSOF_LISTEN: &str = concat!(
        "p648\n", "crapportd\n", "f10\n", "n*:50923\n", "f11\n", "n*:50923\n",
        "p50361\n", "cnode\n", "f24\n", "n*:3040\n",
        "p39389\n", "cnode\n", "f30\n", "n127.0.0.1:4111\n", "f31\n", "n*:4111\n",
    );

    #[test]
    fn le_porte_si_contano_una_volta_sola() {
        let l = parse_listeners(LSOF_LISTEN);
        assert_eq!(l.len(), 3, "IPv4 e IPv6 sono lo stesso server: {l:?}");
        assert_eq!(l[0].port, 50923);
        assert_eq!(l[1].port, 3040);
        assert_eq!(l[1].command, "node");
        assert_eq!(l[2].port, 4111);
    }

    #[test]
    fn la_cartella_di_lavoro_si_lega_al_processo_giusto() {
        let out = "p50361\nfcwd\nn/w/tautog\np39389\nfcwd\nn/w/altro\n";
        assert_eq!(
            parse_cwds(out),
            vec![(50361, "/w/tautog".to_string()), (39389, "/w/altro".to_string())]
        );
    }

    #[test]
    fn eredita_solo_chi_lavora_in_questo_albero() {
        let listeners = vec![
            Listener { pid: 1, command: "node".into(), port: 3040 },
            Listener { pid: 2, command: "node".into(), port: 4111 },
            Listener { pid: 3, command: "ollama".into(), port: 11434 },
        ];
        // Il caso vero del 17/08/2026: la cwd del secondo sta tre livelli sotto
        // la radice, quindi il confronto è sul prefisso e non sull'uguaglianza.
        let cwds = vec![
            (1, "/w/tautog".to_string()),
            (2, "/w/tautog/src/mastra/public".to_string()),
            (3, "/home/someone".to_string()),
        ];
        let here = inherited_listeners(&listeners, &cwds, "/w/tautog");
        assert_eq!(here.iter().map(|l| l.port).collect::<Vec<_>>(), vec![3040, 4111]);
        // Un albero fratello col nome che comincia uguale non è questo albero.
        assert!(inherited_listeners(&listeners, &cwds, "/w/tau").is_empty());
        // Radice ignota: non si eredita niente invece di ereditare tutto.
        assert!(inherited_listeners(&listeners, &cwds, "").is_empty());
    }

    #[test]
    fn senza_niente_acceso_la_terza_clausola_non_compare() {
        assert_eq!(inherited_clause(&[]), "");
        let m = mandate("/x/consegna.md", "");
        assert!(!m.contains("TERZA REGOLA"), "{m}");
    }

    #[test]
    fn la_terza_clausola_elenca_le_porte_e_non_ordina_di_spegnere() {
        let here = vec![Listener { pid: 1, command: "node".into(), port: 3040 }];
        let c = inherited_clause(&here);
        assert!(c.contains("porta 3040 (node)"), "{c}");
        // Il successore non deve spegnere il lavoro di qualcun altro.
        assert!(!c.contains("spegnili"), "{c}");
        assert!(c.contains("lsof"), "deve dire come verificare: {c}");
        // E la clausola arriva davvero nel mandato.
        assert!(mandate("/x/consegna.md", &c).contains("TERZA REGOLA"));
    }

    #[test]
    fn la_finestra_oraria_e_chiusa_a_destra() {
        assert!(!within_hours(7));
        assert!(within_hours(8));
        assert!(within_hours(12));
        assert!(within_hours(20));
        // Le 21 sono già fuori: `HOUR_MAX` è esclusivo, come il `<` del Python.
        assert!(!within_hours(21));
        assert!(!within_hours(3));
    }

    #[test]
    fn le_shell_non_contano_come_sessioni() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"result":{"terminals":[
                {"worktreePath":"/x","title":"Setup"},
                {"worktreePath":"/x","title":"Terminal 1"},
                {"worktreePath":"/x","title":"✳ Claude Code"},
                {"worktreePath":"/x","title":"◑ Consegna in corso"},
                {"worktreePath":"/altro","title":"✳ Claude Code"}]}}"#,
        )
        .unwrap();
        assert_eq!(count_agents(&raw, "/x"), 2);
        assert_eq!(count_agents(&raw, "/altro"), 1);
        assert_eq!(count_agents(&raw, "/nessuno"), 0);
    }

    #[test]
    fn l_impronta_dipende_dalla_sessione_e_non_dal_file() {
        // Il difetto del 17/08: due documenti nello stesso turno, due schede.
        let a = armed_fingerprint("/x/consegna.md", "sessione-A");
        let b = armed_fingerprint("/x/MEMORY.md", "sessione-A");
        assert_eq!(a, b);
        assert_ne!(a, armed_fingerprint("/x/consegna.md", "sessione-B"));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn senza_sessione_si_ricade_sul_percorso() {
        // Un payload senza identificativo torna al comportamento stretto, che è
        // il verso giusto in cui sbagliare.
        let a = armed_fingerprint("/x/consegna.md", "");
        assert_ne!(a, armed_fingerprint("/x/altra.md", ""));
    }

    #[test]
    fn il_mandato_porta_le_due_clausole() {
        let m = mandate("/x/consegna.md", "");
        assert!(m.contains("/x/consegna.md"));
        assert!(m.contains("notifica automatica"));
        assert!(m.contains("git fetch --all --prune"));
    }

    fn via_libera() -> ArmFacts {
        ArmFacts {
            hour: 12,
            live_sessions: Some(3),
            session_cap: 8,
            panes_here: Some(1),
            pane_cap: 2,
            // Il valore neutro è «piena»: questi casi provano gli ALTRI freni, e
            // col default `false` si sarebbero fermati tutti qui — verdi per il
            // motivo sbagliato, che è il modo in cui una batteria smette di
            // guardare.
            enough_used: true,
            ..Default::default()
        }
    }

    #[test]
    fn una_consegna_scritta_a_meta_lavoro_non_arma() {
        // Il difetto del 17/08/2026: `/handoff` si invoca anche a metà, e la
        // sola scrittura apriva il successore accanto a chi stava lavorando.
        let f = ArmFacts { enough_used: false, ..via_libera() };
        assert_eq!(decide(&f), Outcome::StopQuiet("non-piena"));
    }

    #[test]
    fn il_freno_della_pienezza_viene_prima_dei_tetti() {
        // Se il consumo si valutasse dopo, una sessione a metà lavoro in un
        // albero affollato riceverebbe il messaggio sbagliato — e soprattutto
        // il freno che scrive il marcatore brucerebbe l'arma di quella sessione.
        let f = ArmFacts {
            enough_used: false,
            panes_here: Some(9),
            live_sessions: Some(99),
            ..via_libera()
        };
        assert_eq!(decide(&f), Outcome::StopQuiet("non-piena"));
    }

    #[test]
    fn una_sessione_piena_passa_come_prima() {
        assert_eq!(decide(&via_libera()), Outcome::Open);
    }

    /// I due casi veri del 17/08/2026, che scelgono la soglia.
    ///
    /// Budget Opus 5: 500k, avviso 390k, obbligo 450k. Con la soglia d'obbligo
    /// il primo caso sarebbe muto — ed è quello per cui questo meccanismo
    /// esiste, la sessione che doveva passare il testimone e non l'ha passato.
    #[test]
    fn la_soglia_separa_chi_chiude_da_chi_e_a_meta() {
        let (avviso, obbligo) = (390_000, 450_000);
        // La sessione all'88% che quel giorno rimase a terra: deve armare.
        assert!(is_full_enough(440_000, avviso));
        assert!(!is_full_enough(440_000, obbligo), "con l'obbligo resterebbe ferma");
        // `tautog` alle 16:28, consegna scritta a metà lavoro: non deve armare.
        assert!(!is_full_enough(352_000, avviso));
        // Non misurato non è vuoto: senza misura non si apre niente.
        assert!(!is_full_enough(0, avviso));
        // E se anche la soglia mancasse, «zero su zero» non è una sessione
        // piena. Senza questo caso il `used > 0` sarebbe ridondante — con una
        // soglia positiva nessun consumo nullo la raggiunge — e un mutante che
        // lo toglie passerebbe la batteria: verificato il 17/08/2026.
        assert!(!is_full_enough(0, 0));
    }

    #[test]
    fn con_tutti_i_freni_liberi_si_apre() {
        assert_eq!(decide(&via_libera()), Outcome::Open);
    }

    #[test]
    fn una_figlia_non_arma_e_tace() {
        let mut f = via_libera();
        f.second_generation = true;
        assert_eq!(decide(&f), Outcome::StopQuiet("seconda-generazione"));
    }

    #[test]
    fn fuori_orario_si_tace() {
        let mut f = via_libera();
        f.hour = 3;
        assert_eq!(decide(&f), Outcome::StopQuiet("fuori-orario"));
    }

    #[test]
    fn i_due_tetti_si_fermano_e_lo_dicono() {
        // Un freno muto è indistinguibile da un meccanismo rotto: il 16/08/2026
        // il registro contava 97 «troppe sessioni» contro 3 aperture, e nessuno
        // dei 97 è mai arrivato a qualcuno.
        let mut f = via_libera();
        f.live_sessions = Some(8);
        match decide(&f) {
            Outcome::StopLoud { reason, message } => {
                assert_eq!(reason, "troppe-sessioni");
                assert!(message.contains("sessioni vive"), "{message}");
            }
            other => panic!("atteso un freno parlante, ottenuto {other:?}"),
        }
        let mut f = via_libera();
        f.panes_here = Some(2);
        match decide(&f) {
            Outcome::StopLoud { reason, message } => {
                assert_eq!(reason, "albero-affollato");
                assert!(message.contains("albero di lavoro"), "{message}");
            }
            other => panic!("atteso un freno parlante, ottenuto {other:?}"),
        }
    }

    #[test]
    fn un_conteggio_ignoto_non_frena() {
        // In dubbio si lascia passare: un elenco illeggibile deve smettere di
        // dare il suo parere, non spegnere il meccanismo.
        let mut f = via_libera();
        f.live_sessions = None;
        f.panes_here = None;
        assert_eq!(decide(&f), Outcome::Open);
    }

    #[test]
    fn il_consumo_e_l_ultimo_freno() {
        // Sta per ultimo perché è quello che SCRIVE il marcatore: consumarlo e
        // poi fermarsi su un altro freno brucerebbe l'unica arma della sessione
        // senza aprire niente.
        let mut f = via_libera();
        f.already_armed = true;
        f.hour = 3;
        assert_eq!(decide(&f), Outcome::StopQuiet("fuori-orario"));
    }
}
