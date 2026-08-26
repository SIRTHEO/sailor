//! Sceglie il modello di un `Agent` dal mestiere, non da chi lo lancia.
//!
//! Un subagent eredita il modello della sessione madre. La tabella qui sotto
//! sta in una costante sola perché si corregge con la prova a campione, non
//! con la fiducia (la misura: `docs/2026-08-23-gradino-5-prova-a-campione.md`).
//! Puro: niente I/O né ambiente, la valvola arriva già decisa da chi chiama.
//! Non è `cost_ledger::MECHANICAL_AGENT_TYPES`: quella dice a consuntivo cosa
//! era delegabile, questa decide prima verso quale modello.

/// Una riga della tabella: mestiere, alias del modello che l'Agent tool
/// accetta nel campo `model`, e il perché (finisce nel registro).
pub type Row = (&'static str, &'static str, &'static str);

/// La tabella in servizio. **Vuota il 23/08/2026**, per misura e non per
/// prudenza: a cieco, `measurer` su haiku ha perso 3 coppie su 3 (numeri
/// falsi con il comando accanto), `code-reviewer` su sonnet 3 su 6 senza calo
/// contro una soglia di 5. Un mestiere entra qui solo quando regge la prova;
/// il meccanismo resta pronto e provato sulla tabella sotto.
pub const TRADE_MODEL: &[Row] = &[];

/// La tabella di partenza del mandato, tenuta per le batterie e per i
/// mutanti: è la forma che il router deve saper applicare, non ciò che
/// applica oggi.
pub const CANDIDATE_TABLE: &[Row] = &[
    (
        "measurer",
        "haiku",
        "legge e conta: un errore si vede nel numero",
    ),
    (
        "Explore",
        "haiku",
        "legge e conta: un errore si vede nel numero",
    ),
    ("code-reviewer", "sonnet", "verdetto su testo già scritto"),
    ("plan-reviewer", "sonnet", "verdetto su testo già scritto"),
];

/// Sopra questa lunghezza il mandato è già un lavoro esteso: la scelta del
/// modello piccolo era sulla forma del compito, non su quella del testo.
const PROMPT_CEILING: usize = 12_000;

/// I modelli in ordine di costo crescente. `fable` non c'è di proposito:
/// è un modello a sé e non si sa collocare fra questi tre, quindi chi lo
/// nomina non viene mai toccato.
const COST_ORDER: &[&str] = &["haiku", "sonnet", "opus"];

/// Il posto di un modello nella scala, riconosciuto sia dall'alias che
/// dall'identificativo pieno (`claude-opus-5`). `None` quando non si sa
/// collocare: allora non si confronta e non si tocca niente.
fn cost_rank(model: &str) -> Option<usize> {
    let lower = model.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    // Dal più caro al più economico: `claude-opus-5` contiene «opus» e basta,
    // ma un nome che li contenesse entrambi va letto sul più caro.
    COST_ORDER
        .iter()
        .enumerate()
        .rev()
        .find(|(_, name)| lower.contains(*name))
        .map(|(i, _)| i)
}

/// Il modello che un mestiere dichiara, letto dal solo frontmatter del suo
/// file di definizione — il blocco fra i primi due `---`.
///
/// Si ferma al frontmatter di proposito: il corpo di un agente parla dei
/// modelli in prosa («non delegare a un modello più piccolo»), e una riga di
/// prosa non è una dichiarazione. Puro: il testo lo legge chi chiama.
pub fn declared_model_in_frontmatter(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    // Il frontmatter esiste solo se il file ci si apre.
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return None; // frontmatter finito senza un `model:`
        }
        if let Some(rest) = trimmed.strip_prefix("model:") {
            let value = rest.trim().trim_matches(['"', '\'']).trim();
            return (!value.is_empty()).then_some(value);
        }
    }
    None // file senza chiusura del frontmatter: si legge quel che c'è
}

/// Il mestiere ha già scelto il proprio modello e chi lancia lo ha
/// scavalcato verso l'alto: cosa riportare, e perché.
#[derive(Debug, PartialEq, Eq)]
pub struct Downgrade {
    /// Il modello dichiarato dal mestiere, a cui si torna.
    pub to: String,
    pub reason: &'static str,
}

/// Fa valere il modello che il mestiere dichiara nel proprio frontmatter
/// quando la chiamata lo scavalca **verso l'alto**.
///
/// Non è una scelta nuova: quel modello è già stato deciso e versionato in
/// `~/.claude/agents/<mestiere>.md`. Chi passa `model` esplicito nella
/// chiamata `Agent` lo scavalca in silenzio, perché il campo della chiamata
/// vince sul frontmatter. Misurato il 26/08/2026 sui transcript dal 23/08:
/// **67 chiamate su circa 400** forzano un modello più caro di quello
/// dichiarato — 39 su `builder`, 21 su `measurer`, 6 su `code-reviewer`,
/// 1 su `security-reviewer`.
///
/// Verso il basso non si tocca: chi chiede un modello più economico di
/// quello dichiarato sta risparmiando, e non c'è una decisione da difendere.
/// Puro: il frontmatter lo legge chi chiama.
pub fn declared_model_wins(declared: Option<&str>, asked: Option<&str>) -> Option<Downgrade> {
    let asked = asked?.trim();
    let declared = declared?.trim();
    // Una stringa vuota vale come assente, di qua e di là: è la stessa
    // convenzione di `route_with`.
    let (Some(asked_rank), Some(declared_rank)) = (cost_rank(asked), cost_rank(declared)) else {
        return None;
    };
    if asked_rank <= declared_rank {
        return None;
    }
    Some(Downgrade {
        to: declared.to_string(),
        reason: "il mestiere aveva già scelto il proprio modello",
    })
}

/// Cosa il router ha deciso, e perché — per la riga di registro e per chi
/// prova a mano.
#[derive(Debug, PartialEq, Eq)]
pub struct Routing {
    /// `Some` solo quando l'input va riscritto.
    pub model: Option<&'static str>,
    /// Il mestiere così com'è arrivato, vuoto se assente: la riga di
    /// registro lo vuole anche quando il router non tocca niente.
    pub trade: String,
    pub reason: &'static str,
}

/// Il mandato chiede di scrivere o modificare sorgente: un verbo intero, o
/// una delle frasi che lo dicono in più parole. Niente `commit` né `edit`:
/// misurato il 23/08/2026 su 107 mandati reali di `measurer`, 27 nominano un
/// commit solo per leggerlo («i commit di origin/develop», «non fare commit»)
/// e 6 nominano lo strumento `Edit` per vietarlo — nessuno chiedeva di scrivere.
fn asks_to_write_code(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    const PHRASES: &[&str] = &["scrivi il codice", "scrivi codice", "modifica il codice"];
    if PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    const VERBS: &[&str] = &["implementa", "correggi"];
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| VERBS.contains(&w))
}

/// Dai fatti dell'`Agent` alla decisione, con la tabella in servizio.
pub fn route(trade: Option<&str>, model: Option<&str>, prompt: &str, valve_off: bool) -> Routing {
    route_with(TRADE_MODEL, trade, model, prompt, valve_off)
}

/// Lo stesso giudizio su una tabella passata da fuori: è ciò che le batterie
/// e i mutanti provano, perché la tabella in servizio può essere vuota.
/// `valve_off` è già risolta da chi chiama: qui non si legge l'ambiente.
pub fn route_with(
    table: &[Row],
    trade: Option<&str>,
    model: Option<&str>,
    prompt: &str,
    valve_off: bool,
) -> Routing {
    let trade = trade.unwrap_or("").to_string();
    if valve_off {
        return Routing {
            model: None,
            trade,
            reason: "valvola PHASE_ROUTER=off",
        };
    }
    // Una stringa vuota vale come assente: chi lancia non ha scritto niente.
    if model.is_some_and(|m| !m.trim().is_empty()) {
        return Routing {
            model: None,
            trade,
            reason: "chi lancia ha deciso",
        };
    }
    let Some(&(_, chosen, why)) = table.iter().find(|(t, _, _)| *t == trade) else {
        return Routing {
            model: None,
            trade,
            reason: "mestiere non tabellato",
        };
    };
    // La forma del mandato conta solo per chi legge: un builder che scrive lo
    // fa per mestiere, un measurer o un Explore che scrive è fuori forma.
    if matches!(trade.as_str(), "measurer" | "Explore") {
        if asks_to_write_code(prompt) {
            return Routing {
                model: None,
                trade,
                reason: "il mandato chiede di scrivere codice",
            };
        }
        if prompt.chars().count() > PROMPT_CEILING {
            return Routing {
                model: None,
                trade,
                reason: "mandato lunghissimo",
            };
        }
    }
    Routing {
        model: Some(chosen),
        trade,
        reason: why,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(trade: &str) -> Routing {
        route_with(CANDIDATE_TABLE, Some(trade), None, "censisci i file", false)
    }

    /// La tabella in servizio è vuota per misura: nessun mestiere viene
    /// riscritto finché non regge la prova a campione. Rompere questa prova
    /// vuol dire aver messo una riga in tabella: allora si aggiorna anche la
    /// misura, non solo il test.
    #[test]
    fn the_table_in_service_rewrites_nothing_until_a_trade_passes_the_trial() {
        for (trade, _, _) in CANDIDATE_TABLE {
            let d = route(Some(trade), None, "censisci i file", false);
            assert_eq!(
                d.model, None,
                "{trade}: la prova a campione non lo ha promosso"
            );
            assert_eq!(d.reason, "mestiere non tabellato");
        }
        assert!(TRADE_MODEL.is_empty());
    }

    /// Una riga per ogni riga della tabella candidata.
    #[test]
    fn measurer_and_explore_go_to_haiku() {
        assert_eq!(r("measurer").model, Some("haiku"));
        assert_eq!(r("Explore").model, Some("haiku"));
    }

    #[test]
    fn reviewers_go_to_sonnet() {
        assert_eq!(r("code-reviewer").model, Some("sonnet"));
        assert_eq!(r("plan-reviewer").model, Some("sonnet"));
    }

    /// Chi scrive o giudica rischio resta sul modello della sessione, e così
    /// un mestiere ignoto o assente: nessuno di questi è nella tabella.
    #[test]
    fn everything_not_in_the_table_is_left_alone() {
        for trade in [
            "builder",
            "investigator",
            "security-reviewer",
            "database-reviewer",
            "fork",
            "un-mestiere-mai-visto",
            "",
        ] {
            let d = r(trade);
            assert_eq!(d.model, None, "{trade} non deve riscrivere");
        }
        // L'assenza del campo è lo stesso caso del mestiere vuoto.
        assert_eq!(
            route_with(CANDIDATE_TABLE, None, None, "x", false).model,
            None
        );
    }

    /// La valvola spegne il router a prescindere dal mestiere.
    #[test]
    fn the_valve_passes_everything_through() {
        let d = route_with(CANDIDATE_TABLE, Some("measurer"), None, "x", true);
        assert_eq!(d.model, None);
        assert_eq!(d.reason, "valvola PHASE_ROUTER=off");
    }

    /// `model` già scritto vince sempre, anche su un mestiere tabellato.
    /// Una stringa vuota non conta come scritto.
    #[test]
    fn an_explicit_model_is_never_overwritten() {
        let d = route_with(CANDIDATE_TABLE, Some("measurer"), Some("opus"), "x", false);
        assert_eq!(d.model, None);
        assert_eq!(d.reason, "chi lancia ha deciso");

        let empty = route_with(CANDIDATE_TABLE, Some("measurer"), Some(""), "x", false);
        assert_eq!(
            empty.model,
            Some("haiku"),
            "una stringa vuota non è una scelta"
        );
    }

    /// La forma del mandato: un `measurer` a cui si chiede di scrivere resta
    /// sul modello della sessione, con un motivo diverso dal caso normale.
    #[test]
    fn a_measurer_asked_to_write_code_stays_on_the_session_model() {
        for prompt in [
            "implementa la correzione nel file x",
            "correggi il bug in guards/src",
            "scrivi il codice per il nuovo gancio",
        ] {
            let d = route_with(CANDIDATE_TABLE, Some("measurer"), None, prompt, false);
            assert_eq!(d.model, None, "{prompt:?} doveva restare sulla sessione");
            assert_eq!(d.reason, "il mandato chiede di scrivere codice");
        }
    }

    /// Leggere un commit, o vietare `Edit`, non è scrivere: sono i mandati
    /// reali di `measurer` che il primo filtro fermava a torto (27 + 6 su 107).
    #[test]
    fn reading_commits_or_forbidding_edit_is_not_writing() {
        for prompt in [
            "elenca i commit di origin/develop degli ultimi 30 giorni",
            "non correggere, non fondere, non fare commit",
            "sola lettura: non usare Write né Edit",
        ] {
            let d = route_with(CANDIDATE_TABLE, Some("measurer"), None, prompt, false);
            assert_eq!(d.model, Some("haiku"), "{prompt:?} doveva andare al router");
        }
    }

    /// Un mandato lunghissimo per un `Explore` è già un lavoro esteso.
    #[test]
    fn an_extremely_long_prompt_stays_on_the_session_model() {
        let long = "a".repeat(PROMPT_CEILING + 1);
        let d = route_with(CANDIDATE_TABLE, Some("Explore"), None, &long, false);
        assert_eq!(d.model, None);
        assert_eq!(d.reason, "mandato lunghissimo");
        // Esattamente al tetto si passa ancora.
        let at_ceiling = "a".repeat(PROMPT_CEILING);
        assert_eq!(
            route_with(CANDIDATE_TABLE, Some("Explore"), None, &at_ceiling, false).model,
            Some("haiku")
        );
    }

    /// La forma del mandato è una regola per chi legge: un `code-reviewer` a
    /// cui si chiede di scrivere non è fuori tabella, perché il suo mestiere
    /// non entra in questo controllo.
    #[test]
    fn the_shape_check_does_not_apply_outside_measurer_and_explore() {
        let d = route_with(
            CANDIDATE_TABLE,
            Some("code-reviewer"),
            None,
            "implementa la correzione",
            false,
        );
        assert_eq!(d.model, Some("sonnet"));
    }

    /// Il mestiere torna intatto nella decisione, anche quando non riscrive.
    #[test]
    fn the_trade_is_echoed_back_for_the_log_line() {
        assert_eq!(r("builder").trade, "builder");
        assert_eq!(route(None, None, "x", false).trade, "");
    }

    /// I quattro casi misurati il 26/08/2026 sui transcript: un mestiere che
    /// dichiara `sonnet` e una chiamata che chiede `opus`. Sono 67 chiamate
    /// su circa 400, e tutte tornano al modello dichiarato.
    #[test]
    fn asking_for_a_costlier_model_than_the_trade_declares_is_brought_back() {
        for trade in ["builder", "measurer", "code-reviewer", "security-reviewer"] {
            let d = declared_model_wins(Some("sonnet"), Some("opus"))
                .unwrap_or_else(|| panic!("{trade}: doveva tornare al dichiarato"));
            assert_eq!(d.to, "sonnet");
            assert_eq!(d.reason, "il mestiere aveva già scelto il proprio modello");
        }
    }

    /// Verso il basso non si tocca: chi chiede più economico sta
    /// risparmiando, e non scavalca nessuna decisione.
    #[test]
    fn asking_for_a_cheaper_model_is_left_alone() {
        assert_eq!(declared_model_wins(Some("opus"), Some("sonnet")), None);
        assert_eq!(declared_model_wins(Some("opus"), Some("haiku")), None);
        assert_eq!(declared_model_wins(Some("sonnet"), Some("haiku")), None);
    }

    /// Lo stesso modello da tutte e due le parti non è uno scavalcamento,
    /// nemmeno quando è scritto per esteso da un lato solo.
    #[test]
    fn the_same_model_on_both_sides_is_not_an_override() {
        for (declared, asked) in [
            ("sonnet", "sonnet"),
            ("opus", "opus"),
            ("sonnet", "claude-sonnet-5"),
            ("opus", "CLAUDE-OPUS-5"),
        ] {
            assert_eq!(
                declared_model_wins(Some(declared), Some(asked)),
                None,
                "{declared} contro {asked}"
            );
        }
    }

    /// L'identificativo pieno si colloca come il suo alias: è la forma in cui
    /// i modelli compaiono nel registro dei costi.
    #[test]
    fn a_full_model_id_is_ranked_like_its_alias() {
        let d = declared_model_wins(Some("claude-sonnet-5"), Some("claude-opus-5"))
            .expect("opus è più caro di sonnet, comunque sia scritto");
        assert_eq!(d.to, "claude-sonnet-5", "si torna al testo del frontmatter");
    }

    /// `fable` non sta nella scala: non si sa se sia più o meno caro, quindi
    /// non si tocca da nessuna delle due parti. È la prudenza che evita di
    /// riscrivere una scelta che non si sa giudicare.
    #[test]
    fn a_model_outside_the_cost_scale_is_never_touched() {
        assert_eq!(declared_model_wins(Some("sonnet"), Some("fable")), None);
        assert_eq!(declared_model_wins(Some("fable"), Some("opus")), None);
        assert_eq!(
            declared_model_wins(Some("sonnet"), Some("un-modello-mai-visto")),
            None
        );
    }

    /// Senza una delle due parti non c'è confronto: un mestiere che non
    /// dichiara niente (gli agenti di sistema, quelli dei plugin) e una
    /// chiamata che non chiede niente restano intatti. La stringa vuota vale
    /// come assente, come in `route_with`.
    #[test]
    fn a_missing_or_empty_side_means_no_comparison() {
        assert_eq!(declared_model_wins(None, Some("opus")), None);
        assert_eq!(declared_model_wins(Some("sonnet"), None), None);
        assert_eq!(declared_model_wins(None, None), None);
        assert_eq!(declared_model_wins(Some(""), Some("opus")), None);
        assert_eq!(declared_model_wins(Some("sonnet"), Some("")), None);
        assert_eq!(declared_model_wins(Some("sonnet"), Some("   ")), None);
    }

    /// La scala è ordinata dal più economico al più caro: se qualcuno la
    /// riordina, questa prova cade prima che il gancio riporti al modello
    /// sbagliato.
    #[test]
    fn the_cost_scale_runs_from_cheapest_to_costliest() {
        assert_eq!(COST_ORDER, &["haiku", "sonnet", "opus"]);
        assert!(cost_rank("haiku") < cost_rank("sonnet"));
        assert!(cost_rank("sonnet") < cost_rank("opus"));
        assert_eq!(cost_rank(""), None);
    }

    /// Il frontmatter vero di `builder`, con le righe che lo circondano nel
    /// file in servizio: il `model:` si legge in mezzo alle altre chiavi.
    #[test]
    fn the_model_is_read_from_the_frontmatter() {
        let file = "---\nname: builder\ndescription: costruisce\ntools: [\"Read\"]\nmodel: sonnet\neffort: high\n---\n\n# Costruttore\n";
        assert_eq!(declared_model_in_frontmatter(file), Some("sonnet"));
    }

    /// Fuori dal frontmatter non si legge niente: il corpo di un agente
    /// nomina i modelli in prosa, e una frase non è una dichiarazione.
    #[test]
    fn prose_after_the_frontmatter_is_not_a_declaration() {
        let file = "---\nname: x\n---\n\nmodel: opus è quello che useresti a mano\n";
        assert_eq!(declared_model_in_frontmatter(file), None);
    }

    /// Senza frontmatter, o senza `model:` dentro, non c'è dichiarazione:
    /// è il caso degli agenti di sistema e di quelli dei plugin.
    #[test]
    fn a_file_without_a_declared_model_yields_nothing() {
        assert_eq!(declared_model_in_frontmatter(""), None);
        assert_eq!(declared_model_in_frontmatter("# solo un titolo\n"), None);
        assert_eq!(declared_model_in_frontmatter("---\nname: x\n---\n"), None);
        assert_eq!(
            declared_model_in_frontmatter("---\nname: x\nmodel:\n---\n"),
            None,
            "una chiave senza valore non dichiara niente"
        );
    }

    /// Il valore si ripulisce dagli apici: un frontmatter può scriverlo
    /// quotato senza cambiare quale modello indica.
    #[test]
    fn the_value_is_stripped_of_quotes() {
        assert_eq!(
            declared_model_in_frontmatter("---\nmodel: \"sonnet\"\n---\n"),
            Some("sonnet")
        );
        assert_eq!(
            declared_model_in_frontmatter("---\nmodel: 'opus'\n---\n"),
            Some("opus")
        );
    }

    /// Un nome che contiene due alias si legge sul più caro, così un mestiere
    /// non finisce declassato per una parola nel nome del modello. Nessun
    /// modello si chiama oggi così: il caso è costruito, ed è il motivo per
    /// cui la scala si percorre dal fondo.
    #[test]
    fn a_name_carrying_two_aliases_is_read_on_the_costlier_one() {
        assert_eq!(cost_rank("sonnet-tuned-opus"), cost_rank("opus"));
        assert_eq!(
            declared_model_wins(Some("sonnet"), Some("sonnet-tuned-opus"))
                .expect("il nome contiene opus, che è più caro")
                .to,
            "sonnet"
        );
    }
}
