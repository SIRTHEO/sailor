//! Il giudizio puro della ronda delle novità: due inneschi, gradino 9 della
//! scala (`docs/plans/2026-08-23-la-scala-sailor-e-la-squadra.md`), decisi da
//! Theo il 23/08/2026 (libro di bordo, voce «La configurazione si mantiene da
//! sola»). Chi tocca disco, `git` e stdin sta in
//! `claude-hooks/src/ronda_trigger.rs`, per lo stesso motivo di
//! `guards::handoff` e `guards::long_session`: i casi limite — file assente,
//! prima esecuzione, cooldown, voce già aperta — si provano senza filesystem.
//!
//! FAIL-OPEN PER CONTRATTO: questo modulo non blocca mai una sessione, scrive
//! al più un mandato in coda. Ogni «non lo so» qui dentro vale «non scattare».

/// La prima versione dichiarata nel changelog (`## X.Y.Z`), cercata per
/// **pattern**, non per numero di riga: il changelog cresce in testa, e la
/// riga 3 di oggi è la riga 6 di domani.
pub fn latest_version(changelog: &str) -> Option<String> {
    changelog.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("## ")?;
        let v = rest.trim();
        v.chars().next().filter(char::is_ascii_digit)?;
        Some(v.to_string())
    })
}

/// L'innesco A: la versione vista ora contro l'ultima registrata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCheck {
    /// Nessun file di stato: prima esecuzione di questo gancio. Si registra
    /// la versione corrente SENZA innescare — altrimenti la prima sessione
    /// dopo il rilascio del gancio innesca a vuoto su una versione già in
    /// servizio da giorni.
    FirstRun,
    Unchanged,
    Changed {
        previous: String,
        current: String,
    },
}

pub fn check_version(current: &str, seen: Option<&str>) -> VersionCheck {
    match seen.map(str::trim) {
        None => VersionCheck::FirstRun,
        Some(s) if s == current => VersionCheck::Unchanged,
        Some(s) => VersionCheck::Changed {
            previous: s.to_string(),
            current: current.to_string(),
        },
    }
}

/// La metà «impronta di `settings.json`» dell'innesco B: l'ultima riga di
/// `settings-fingerprint-changes.jsonl` contro il watermark registrato.
///
/// SI CONFRONTA IL CONTENUTO, NON LA DATA. Le due fonti scrivono l'ora in
/// formati diversi — `+0200` nel registro dell'impronta, `Z` in quello che
/// questo gancio scrive da sé — e un confronto lessicografico fra formati
/// diversi mente prima ancora di sbagliare fuso orario. La riga stessa è già
/// il confronto che serve: se non è cambiata da quando l'abbiamo già vista,
/// non c'è niente di nuovo da dire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FingerprintCheck {
    /// Il registro non c'è o non si legge: non è «mai cambiata», è «non lo
    /// so», e un «non lo so» non innesca niente.
    Silent,
    /// Nessun watermark: prima esecuzione, si registra la riga di adesso
    /// senza innescare — stesso motivo di `VersionCheck::FirstRun`.
    FirstRun,
    Unchanged,
    Changed(String),
}

pub fn check_fingerprint(last_line: Option<&str>, watermark: Option<&str>) -> FingerprintCheck {
    let Some(line) = last_line else {
        return FingerprintCheck::Silent;
    };
    match watermark {
        None => FingerprintCheck::FirstRun,
        Some(w) if w == line => FingerprintCheck::Unchanged,
        Some(_) => FingerprintCheck::Changed(line.to_string()),
    }
}

/// La metà «binario disallineato» dell'innesco B: nessun watermark, nessuna
/// prima esecuzione da proteggere — è un confronto fra due valori presenti
/// **adesso**, non una cronologia. Una stringa vuota da un lato o dall'altro
/// vuol dire «non sono riuscito a leggerla», e non prova un disallineamento.
pub fn check_binary(binary_commit: &str, head_commit: &str) -> bool {
    !binary_commit.is_empty() && !head_commit.is_empty() && binary_commit != head_commit
}

/// L'innesco B nel suo complesso: scatta se una delle due metà scatta.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriftVerdict {
    pub fires: bool,
    pub via_fingerprint: bool,
    pub via_binary: bool,
}

pub fn check_drift(
    fingerprint: &FingerprintCheck,
    binary_commit: &str,
    head_commit: &str,
) -> DriftVerdict {
    let via_fingerprint = matches!(fingerprint, FingerprintCheck::Changed(_));
    let via_binary = check_binary(binary_commit, head_commit);
    DriftVerdict {
        fires: via_fingerprint || via_binary,
        via_fingerprint,
        via_binary,
    }
}

/// Il gancio gira solo su un avvio vero. `resume` conta, `clear` e `compact`
/// no: la ronda parla al più una volta al giorno per innesco, e un `/clear`
/// dentro la stessa sessione non è una sessione nuova da avvisare.
pub fn wants_run(source: &str) -> bool {
    matches!(source, "startup" | "resume")
}

/// Cooldown: mai due mandati per lo stesso innesco nello stesso giorno.
/// `today` e `last_fired_day` nella forma `YYYY-MM-DD`: uguaglianza di
/// stringa, non aritmetica di calendario — un giorno è lo stesso giorno solo
/// se è scritto uguale.
pub fn in_cooldown(today: &str, last_fired_day: Option<&str>) -> bool {
    last_fired_day == Some(today)
}

/// Il mandato per questo innesco è ancora aperto? Legge `stato:` dal
/// frontmatter dello stesso file che questo gancio scrive: uno «aperta» ferma
/// un secondo mandato, qualunque altro valore — o la sua assenza — lascia
/// passare.
///
/// Riusa `queue_overlap::state_word`, che è il lettore di `stato:` della coda:
/// una seconda copia dello stesso taglio divergerebbe alla prima correzione
/// fatta a una sola delle due. Da lì arriva anche la regola del formato — vale
/// la **prima parola**, il resto della riga è commento — che questa versione
/// non aveva: uno `stato: aperta — RIAPERTA alle 09:50` era uno stato ignoto, e
/// il 21/08/2026 quel dettaglio ha reso muta una voce riaperta.
pub fn already_open(mandate_text: &str) -> bool {
    crate::queue_overlap::state_word(mandate_text).as_deref() == Some("aperta")
}

/// La riga breve verso la sessione, sul canale `additionalContext`: solo
/// quando un innesco scatta, silenzio altrimenti.
pub fn additional_context(innesco: &str, mandate_path: &str) -> String {
    format!("Ronda: innesco {innesco} scattato, mandato in coda: {mandate_path}")
}

/// Il mandato dell'innesco A, nello stesso formato frontmatter delle voci
/// già in coda (`sessione:`/`albero:`/`quando:`/`stato:`/`per:`).
pub fn mandate_body_a(quando: &str, previous: &str, current: &str, settings_line: &str) -> String {
    format!(
        "---\n\
sessione: ronda-delle-novita (automazione, nessuna sessione)\n\
albero: -\n\
quando: {quando}\n\
stato: aperta\n\
per: la sessione generale — ronda delle novità\n\
---\n\
\n\
# Innesco A: versione nuova di Claude Code\n\
\n\
**Chi ha scritto questa voce**: nessuno. Il gancio `ronda-trigger` a\n\
`SessionStart` ha confrontato la prima riga `## X.Y.Z` di\n\
`~/.claude/cache/changelog.md` con `~/.claude/state/ronda-versione-vista` e le\n\
ha trovate diverse.\n\
\n\
**Prima**: `{previous}`\n\
**Dopo**: `{current}`\n\
\n\
Gradino 9 della scala (`docs/plans/2026-08-23-la-scala-sailor-e-la-squadra.md`),\n\
decisione di Theo del 23/08/2026 nel libro di bordo, voce «La configurazione si\n\
mantiene da sola»: la ronda legge il changelog, le skill e i plugin ufficiali\n\
Anthropic, i modelli nuovi — ognuno alla prova a cieco del gradino 5 — e la\n\
documentazione di ingegneria, sempre accanto alle misure vive (costi, istinti,\n\
`ganci.jsonl`). Una novità senza la misura di casa accanto non si giudica.\n\
\n\
**Riga in `settings.json`** (classe MAI: la aggiunge solo Theo), nel gruppo\n\
`SessionStart` con `\"matcher\": \"startup|resume\"`:\n\
\n\
    {settings_line}\n"
    )
}

/// Il mandato dell'innesco B, stesso formato.
pub fn mandate_body_b(
    quando: &str,
    verdict: &DriftVerdict,
    binary_commit: &str,
    head_commit: &str,
    settings_line: &str,
) -> String {
    let reasons = match (verdict.via_fingerprint, verdict.via_binary) {
        (true, true) => "l'impronta di `settings.json` **e** il binario dei ganci",
        (true, false) => "l'impronta di `settings.json`",
        (false, true) => "il binario dei ganci",
        (false, false) => "(nessuno: voce scritta a innesco spento, da correggere)",
    };
    format!(
        "---\n\
sessione: ronda-delle-novita (automazione, nessuna sessione)\n\
albero: -\n\
quando: {quando}\n\
stato: aperta\n\
per: la sessione generale — ronda delle novità\n\
---\n\
\n\
# Innesco B: configurazione cambiata senza prova\n\
\n\
**Chi ha scritto questa voce**: nessuno. Il gancio `ronda-trigger` a\n\
`SessionStart` ha trovato {reasons} disallineati da quanto una ronda ha già\n\
guardato.\n\
\n\
- `state/hooks-binary-commit`: `{binary_commit}`\n\
- `git -C ~/.claude rev-parse HEAD`: `{head_commit}`\n\
\n\
Gradino 9 della scala (`docs/plans/2026-08-23-la-scala-sailor-e-la-squadra.md`),\n\
decisione di Theo del 23/08/2026 nel libro di bordo, voce «La configurazione si\n\
mantiene da sola»: una configurazione cambiata senza prova è esattamente il\n\
caso che l'innesco B esiste per prendere — sempre accanto alle misure vive\n\
(costi, istinti, `ganci.jsonl`). Una novità senza la misura di casa accanto\n\
non si giudica.\n\
\n\
**Riga in `settings.json`** (classe MAI: la aggiunge solo Theo), nel gruppo\n\
`SessionStart` con `\"matcher\": \"startup|resume\"`:\n\
\n\
    {settings_line}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_version_heading_wins_by_pattern_not_by_line_number() {
        let changelog = "# Changelog\n\n## 2.1.241\n\n- Bug fixes\n\n## 2.1.240\n\n- older\n";
        assert_eq!(latest_version(changelog).as_deref(), Some("2.1.241"));
    }

    #[test]
    fn a_changelog_that_grows_a_preamble_still_finds_the_top_version() {
        // Il changelog cresce in testa: la riga 3 di oggi diventa la riga 6 di
        // domani, e cercare per numero di riga fisso avrebbe letto «Bug fixes».
        let changelog =
            "# Changelog\n\nNota degli editori.\n\nAltra riga.\n\n## 3.0.0\n\n- nuova\n";
        assert_eq!(latest_version(changelog).as_deref(), Some("3.0.0"));
    }

    #[test]
    fn a_changelog_without_any_heading_says_nothing() {
        assert_eq!(latest_version("# Changelog\n\nsolo prosa\n"), None);
    }

    #[test]
    fn version_new_fires() {
        let v = check_version("2.1.241", Some("2.1.240"));
        assert_eq!(
            v,
            VersionCheck::Changed {
                previous: "2.1.240".into(),
                current: "2.1.241".into()
            }
        );
    }

    #[test]
    fn version_unchanged_is_silent() {
        assert_eq!(
            check_version("2.1.241", Some("2.1.241")),
            VersionCheck::Unchanged
        );
        // Uno spazio in coda non è una versione diversa.
        assert_eq!(
            check_version("2.1.241", Some("2.1.241\n")),
            VersionCheck::Unchanged
        );
    }

    #[test]
    fn version_first_run_without_a_state_file_does_not_fire() {
        assert_eq!(check_version("2.1.241", None), VersionCheck::FirstRun);
    }

    #[test]
    fn fingerprint_new_line_fires() {
        let fp = check_fingerprint(Some("riga-nuova"), Some("riga-vecchia"));
        assert_eq!(fp, FingerprintCheck::Changed("riga-nuova".into()));
    }

    #[test]
    fn fingerprint_unchanged_is_silent() {
        assert_eq!(
            check_fingerprint(Some("stessa"), Some("stessa")),
            FingerprintCheck::Unchanged
        );
    }

    #[test]
    fn fingerprint_first_run_primes_without_firing() {
        assert_eq!(
            check_fingerprint(Some("qualunque"), None),
            FingerprintCheck::FirstRun
        );
    }

    #[test]
    fn fingerprint_missing_file_is_silent_not_never_changed() {
        // «Non lo so» non è «non è mai cambiata»: il registro assente non deve
        // né innescare né avvelenare il watermark.
        assert_eq!(
            check_fingerprint(None, Some("qualunque")),
            FingerprintCheck::Silent
        );
        assert_eq!(check_fingerprint(None, None), FingerprintCheck::Silent);
    }

    #[test]
    fn a_misaligned_binary_fires() {
        assert!(check_binary("c29095b", "145324a"));
    }

    #[test]
    fn an_aligned_binary_is_silent() {
        assert!(!check_binary("145324a", "145324a"));
    }

    #[test]
    fn an_unreadable_side_is_not_a_mismatch() {
        // «Non sono riuscito a leggerla» non prova un disallineamento: fail-open.
        assert!(!check_binary("", "145324a"));
        assert!(!check_binary("145324a", ""));
        assert!(!check_binary("", ""));
    }

    #[test]
    fn drift_fires_on_either_half_and_says_which() {
        let fp = FingerprintCheck::Changed("x".into());
        let v = check_drift(&fp, "same", "same");
        assert!(v.fires && v.via_fingerprint && !v.via_binary);

        let v = check_drift(&FingerprintCheck::Unchanged, "a", "b");
        assert!(v.fires && !v.via_fingerprint && v.via_binary);

        let v = check_drift(&FingerprintCheck::Unchanged, "same", "same");
        assert!(!v.fires);
    }

    #[test]
    fn wants_run_accepts_startup_and_resume_only() {
        assert!(wants_run("startup"));
        assert!(wants_run("resume"));
        assert!(!wants_run("clear"));
        assert!(!wants_run("compact"));
        assert!(!wants_run(""));
    }

    #[test]
    fn cooldown_blocks_only_the_same_day() {
        assert!(in_cooldown("2026-08-23", Some("2026-08-23")));
        assert!(!in_cooldown("2026-08-23", Some("2026-08-22")));
        assert!(!in_cooldown("2026-08-23", None));
    }

    #[test]
    fn an_open_entry_blocks_a_second_one() {
        let open = "---\nsessione: x\nstato: aperta\nper: y\n---\n\ncorpo\n";
        assert!(already_open(open));
        let closed = "---\nsessione: x\nstato: chiusa\nper: y\n---\n\ncorpo\n";
        assert!(!already_open(closed));
        assert!(!already_open("niente frontmatter qui"));
    }

    #[test]
    fn the_additional_context_line_names_the_trigger_and_the_file() {
        let line = additional_context("A", "/x/AUTO-ronda-innesco-a.md");
        assert_eq!(
            line,
            "Ronda: innesco A scattato, mandato in coda: /x/AUTO-ronda-innesco-a.md"
        );
    }

    #[test]
    fn mandate_body_a_carries_the_before_and_after() {
        let body = mandate_body_a("2026-08-23 22:10", "2.1.240", "2.1.241", "LA-RIGA");
        assert!(body.starts_with("---\n"));
        assert!(body.contains("stato: aperta"));
        assert!(body.contains("per: la sessione generale — ronda delle novità"));
        assert!(body.contains("**Prima**: `2.1.240`"));
        assert!(body.contains("**Dopo**: `2.1.241`"));
        assert!(body.contains("LA-RIGA"));
    }

    #[test]
    fn mandate_body_b_names_which_half_fired() {
        let both = DriftVerdict {
            fires: true,
            via_fingerprint: true,
            via_binary: true,
        };
        let body = mandate_body_b("2026-08-23 22:10", &both, "aaa", "bbb", "LA-RIGA");
        assert!(body.contains("impronta"));
        assert!(body.contains("binario"));
        assert!(body.contains("`aaa`"));
        assert!(body.contains("`bbb`"));
        assert!(body.contains("LA-RIGA"));
    }
}
