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

/// È un documento di consegna? `head` sono i primi 400 byte del file.
///
/// Il nome da solo non basta e il frontmatter da solo nemmeno. La skill
/// `handoff` di questo progetto scrive un file dal nome libero dentro `memory/`,
/// dove finisce ogni sorta di memoria: solo quelle marcate `type: project` sono
/// consegne. Misurato il 13/08/2026 subito dopo aver acceso il gancio — su una
/// consegna vera rispondeva «non è una consegna», quindi era acceso e cieco.
///
/// Nel dubbio il gancio deve tacere: una consegna non raccolta costa
/// un'apertura a mano, una scheda aperta a sproposito costa fiducia — e a quel
/// punto lo si spegne del tutto.
pub fn is_handoff_doc(path: &str, head: Option<&str>) -> bool {
    if name_says_handoff(path) {
        return true;
    }
    if !is_dated_memory(path) {
        return false;
    }
    head.map(|t| t.contains("type: project")).unwrap_or(false)
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
            path == root && !title.is_empty()
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

/// Il testo che il successore riceve come primo prompt.
///
/// Le due clausole sono ricopiate apposta, e nessuna è oziosa: una sessione
/// aperta in automatico parte senza nessuno che la corregga al primo turno,
/// quindi ciò che non sta qui non lo fa. Il 13/08/2026 una sessione aperta così
/// ha risposto al bollettino del monitor pur avendo in contesto la regola che lo
/// vieta — regola ricevuta, regola non applicata.
pub fn mandate(path: &str) -> String {
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
         dipendenze, reinstallale."
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
        let m = mandate("/x/consegna.md");
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
            ..Default::default()
        }
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
