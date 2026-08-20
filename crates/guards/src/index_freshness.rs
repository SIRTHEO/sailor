//! Il verdetto puro su un indice SocratiCode: il ramo che il checkout riflette
//! è quello d'integrazione, ed è abbastanza aggiornato da fidarsene?
//!
//! Qui dentro non c'è nessun processo e nessun file: numeri già misurati
//! entrano, un verdetto e una riga escono. Chi legge git e lo stato su disco
//! sta in `claude-hooks::index_freshness`, che passa qui solo dati.
//!
//! PERCHÉ SERVE UN GIUDIZIO A PARTE. L'indice semantico di un repo riflette
//! sempre e solo il ramo che il checkout canonico ha in HEAD — non un ramo
//! dichiarato da nessuna parte. Quando quel HEAD è un ramo di lavoro altrui
//! (misurato il 20/08/2026: tre canonici su quattro), una ricerca può citare
//! codice che su `develop` non esiste, o tacere di codice che c'è da
//! settimane. Il caso peggiore non dà nessun errore: l'indice resta «green».

/// Quanti commit un checkout sul ramo giusto può avere indietro prima che il
/// verdetto passi da affidabile a vecchio.
///
/// SOGLIA MISURATA SUI QUATTRO REPO VERI IL 20/08/2026, non scelta a occhio:
/// `a-service` era sul ramo giusto (`develop`) ma 14 commit indietro
/// dall'ultimo fetch; `packages`, appena aggiornato, era a 0. La soglia sta a
/// metà — abbastanza bassa da non fidarsi di una giornata di lavoro persa,
/// abbastanza alta da non gridare al primo commit arrivato mentre la sessione
/// era già aperta.
pub const BEHIND_STALE_THRESHOLD: u32 = 10;

/// Oltre questa età un fetch non garantisce più che i numeri siano veri.
///
/// Misurata sugli stessi quattro repo: nell'ultima settimana `suite` ha preso
/// ~22 commit al giorno su `develop`, `a-service` ~12, `a-client` ~5,
/// `packages` ~1,6 (`git log origin/develop --since="7 days ago"`,
/// 20/08/2026). Anche il più lento accumula qualcosa in un giorno, e il più
/// veloce può nascondere dozzine di commit: un giorno intero senza fetch è il
/// punto oltre cui «N indietro» smette di essere una misura e diventa una
/// supposizione.
pub const FETCH_STALE_SECS: u64 = 24 * 60 * 60;

/// Cosa si sa di un repo indicizzato, già misurato: nessun campo qui dentro
/// richiede di parlare con git per essere letto.
pub struct Observation {
    pub name: String,
    pub branch: String,
    pub integration: String,
    /// `origin/<integration>` esiste in questo repo? Se no, lo scarto non si
    /// può nemmeno chiedere.
    pub has_integration_ref: bool,
    pub behind: Option<u32>,
    pub ahead: Option<u32>,
    pub fetch_age_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Sul ramo d'integrazione, e non troppo indietro.
    Reliable,
    /// Sul ramo d'integrazione, ma abbastanza indietro da non fidarsene.
    Outdated { behind: u32 },
    /// Il checkout segue un altro ramo: l'indice mente su cosa esiste.
    Wrong,
    /// Non si può dire — manca `origin/<integration>` o lo scarto non si è
    /// potuto misurare.
    Unknown(String),
}

/// Il giudizio puro: dati dentro, verdetto fuori. Il ramo batte lo scarto —
/// un checkout sul ramo sbagliato è "sbagliato" anche se per puro caso è
/// avanti di zero commit rispetto a `develop`.
pub fn judge(obs: &Observation) -> Verdict {
    if !obs.has_integration_ref {
        return Verdict::Unknown(format!(
            "nessun ramo origin/{} su questo repo",
            obs.integration
        ));
    }
    if obs.branch != obs.integration {
        return Verdict::Wrong;
    }
    match obs.behind {
        None => Verdict::Unknown("scarto non misurabile".to_string()),
        Some(behind) if behind > BEHIND_STALE_THRESHOLD => Verdict::Outdated { behind },
        Some(_) => Verdict::Reliable,
    }
}

/// Un fetch mancante conta come vecchio: fingerlo fresco mentirebbe più di
/// quanto mentirebbe dirlo sconosciuto.
pub fn is_fetch_stale(fetch_age_secs: Option<u64>) -> bool {
    match fetch_age_secs {
        None => true,
        Some(secs) => secs > FETCH_STALE_SECS,
    }
}

fn fetch_caveat(fetch_age_secs: Option<u64>) -> String {
    if !is_fetch_stale(fetch_age_secs) {
        return String::new();
    }
    match fetch_age_secs {
        Some(secs) => format!(
            " [fetch di {}h fa: i numeri potrebbero essere vecchi]",
            secs / 3600
        ),
        None => " [età del fetch sconosciuta: i numeri potrebbero essere vecchi]".to_string(),
    }
}

fn count_or_placeholder(n: Option<u32>) -> String {
    n.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string())
}

/// La riga che finisce nel rapporto di sessione: un repo, un verdetto, senza
/// bisogno di leggere il resto per capirlo.
pub fn format_line(obs: &Observation, verdict: &Verdict) -> String {
    let caveat = fetch_caveat(obs.fetch_age_secs);
    let behind = count_or_placeholder(obs.behind);
    let ahead = count_or_placeholder(obs.ahead);
    match verdict {
        Verdict::Reliable => format!(
            "{}: {} — {behind} indietro / {ahead} avanti da origin/{} — AFFIDABILE{caveat}",
            obs.name, obs.branch, obs.integration
        ),
        Verdict::Outdated { .. } => format!(
            "{}: {} — {behind} indietro / {ahead} avanti da origin/{} (soglia {BEHIND_STALE_THRESHOLD}) — VECCHIO{caveat}",
            obs.name, obs.branch, obs.integration
        ),
        Verdict::Wrong => format!(
            "{}: ramo «{}», non «{}» — SBAGLIATO: l'indice riflette un ramo di lavoro, non l'integrazione{caveat}",
            obs.name, obs.branch, obs.integration
        ),
        Verdict::Unknown(reason) => format!("{}: {reason} — NON VERIFICABILE{caveat}", obs.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str, branch: &str) -> Observation {
        Observation {
            name: name.to_string(),
            branch: branch.to_string(),
            integration: "develop".to_string(),
            has_integration_ref: true,
            behind: Some(0),
            ahead: Some(0),
            fetch_age_secs: Some(60),
        }
    }

    /// I quattro repo veri del 20/08/2026, coi numeri misurati quel giorno.
    #[test]
    fn suite_on_a_feature_branch_is_wrong_no_matter_the_gap() {
        let mut o = base("suite", "feat/design-residues-raw-colors");
        o.behind = Some(157);
        o.ahead = Some(1);
        assert_eq!(judge(&o), Verdict::Wrong);
    }

    #[test]
    fn a-client_on_a_fix_branch_is_wrong() {
        let mut o = base("a-client", "fix/matching-response-shapes");
        o.behind = Some(27);
        o.ahead = Some(2);
        assert_eq!(judge(&o), Verdict::Wrong);
    }

    #[test]
    fn matching_engine_on_develop_but_14_behind_is_outdated() {
        let mut o = base("a-service", "develop");
        o.behind = Some(14);
        assert_eq!(judge(&o), Verdict::Outdated { behind: 14 });
    }

    #[test]
    fn packages_on_develop_and_caught_up_is_reliable() {
        let o = base("packages", "develop");
        assert_eq!(judge(&o), Verdict::Reliable);
    }

    // --- il confine della soglia, a variabile unica -------------------------

    #[test]
    fn exactly_at_the_threshold_is_still_reliable() {
        let mut o = base("x", "develop");
        o.behind = Some(BEHIND_STALE_THRESHOLD);
        assert_eq!(judge(&o), Verdict::Reliable);
    }

    #[test]
    fn one_past_the_threshold_is_outdated() {
        let mut o = base("x", "develop");
        o.behind = Some(BEHIND_STALE_THRESHOLD + 1);
        assert_eq!(
            judge(&o),
            Verdict::Outdated {
                behind: BEHIND_STALE_THRESHOLD + 1
            }
        );
    }

    #[test]
    fn a_missing_integration_ref_is_unknown_even_on_the_right_branch_name() {
        let mut o = base("x", "develop");
        o.has_integration_ref = false;
        assert!(matches!(judge(&o), Verdict::Unknown(_)));
    }

    #[test]
    fn an_unmeasurable_gap_on_the_right_branch_is_unknown() {
        let mut o = base("x", "develop");
        o.behind = None;
        assert!(matches!(judge(&o), Verdict::Unknown(_)));
    }

    // --- la freschezza del fetch, separata dal verdetto ---------------------

    #[test]
    fn a_recent_fetch_is_not_stale() {
        assert!(!is_fetch_stale(Some(60)));
        assert!(!is_fetch_stale(Some(FETCH_STALE_SECS)));
    }

    #[test]
    fn a_fetch_older_than_a_day_is_stale() {
        assert!(is_fetch_stale(Some(FETCH_STALE_SECS + 1)));
    }

    #[test]
    fn an_unknown_fetch_age_is_treated_as_stale_not_as_fresh() {
        // Fingerla fresca mentirebbe di più: un numero senza data è un
        // numero di cui non si sa se fidarsi.
        assert!(is_fetch_stale(None));
    }

    #[test]
    fn the_line_names_the_stale_fetch_instead_of_pretending_the_numbers_are_current() {
        let mut o = base("suite", "develop");
        o.fetch_age_secs = Some(FETCH_STALE_SECS + 3600); // 25 ore
        let line = format_line(&o, &judge(&o));
        assert!(line.contains("fetch di 25h fa"), "line={line}");
    }

    #[test]
    fn a_fresh_fetch_leaves_no_caveat_in_the_line() {
        let o = base("packages", "develop");
        let line = format_line(&o, &judge(&o));
        assert!(!line.contains("fetch"), "line={line}");
    }

    #[test]
    fn the_wrong_verdict_names_both_branches() {
        let o = base("suite", "feat/x");
        let line = format_line(&o, &Verdict::Wrong);
        assert!(line.contains("feat/x") && line.contains("develop"), "line={line}");
    }
}
