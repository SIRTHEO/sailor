//! La sessione che cambia mestiere in corsa: cosa si decide.
//!
//! Porto del giudizio di `skills/hooks/scope-drift.py`. Qui non si legge niente
//! e non si scrive niente: entra il payload dello strumento più ciò che la
//! sessione aveva già visto, esce l'elenco aggiornato delle aree e le due
//! risposte che l'involucro deve dare — se riscrivere lo stato, e se parlare.
//!
//! PERCHÉ ESISTE. L'impianto di presidio misura **quanto** una sessione ha
//! consumato: consegna obbligatoria al 78% e al 90% del budget. Nessuno guardava
//! **cosa** stesse facendo. Un gancio sul volume arriva sempre tardi — a
//! contesto pieno chiudere costa caro, perché il lavoro in corso è tanto. Questo
//! parla presto, quando cambiare sessione costa un minuto.
//!
//! IL NUMERO CHE LO MOTIVA, su 804 sessioni registrate: le corte (<500 righe)
//! toccano 1,49 aree, le enormi (>=10k) ne toccano 5,08 — e il 92% di queste
//! ultime sta sopra le tre. La dispersione non è un carattere della sessione,
//! **cresce con la sua lunghezza**: quindi non si corregge con l'attenzione, si
//! corregge chiudendo prima.
//!
//! TRE È LA SOGLIA perché due aree sono normali — un codice e la sua
//! configurazione, o due servizi che si parlano. Tre sono un altro mestiere.
//!
//! NON BLOCCA, E PARLA UNA VOLTA SOLA. Un blocco qui sarebbe sbagliato: a volte
//! tre aree sono davvero il lavoro (un contratto che attraversa tre servizi). Si
//! dice il fatto e si lascia decidere, e non lo si ripete.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// La terza area è quella che fa parlare il gancio.
pub const THRESHOLD: usize = 3;

/// I campi del payload in cui può comparire un percorso, nell'ordine
/// dell'originale: il testo si concatena, e l'ordine è parte di ciò che i
/// riconoscitori vedono.
pub const FIELDS: [&str; 4] = ["command", "file_path", "path", "notebook_path"];

/// Le stesse radici di `measure-drift.py`: se divergono, il numero della regola
/// smette di descrivere ciò che il gancio misura.
///
/// `configurazione` non è qui perché la sua è l'unica espressione con uno
/// sguardo in avanti negato, che il motore delle espressioni regolari di Rust
/// non ha: sta in [`is_configuration`], scritta a mano.
fn roots() -> &'static [(&'static str, Regex)] {
    static RE: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            (
                "suite",
                Regex::new(r"/gyver/work/suite\b|workspaces/suite/").unwrap(),
            ),
            (
                "matching-engine",
                Regex::new(r"/gyver/work/matching-engine\b|workspaces/matching-engine/").unwrap(),
            ),
            (
                "whatsapp",
                Regex::new(r"/gyver/work/whatsapp\b|workspaces/whatsapp/").unwrap(),
            ),
            ("packages", Regex::new(r"/gyver/work/packages\b").unwrap()),
            ("dev-stack", Regex::new(r"/gyver/work/dev-stack\b").unwrap()),
            (
                "personale",
                Regex::new(r"/personal/|/orca/general\b").unwrap(),
            ),
        ]
    })
}

/// `/\.claude/(?!projects|history)` scritta a mano.
///
/// PERCHÉ NON UNA REGEX. Il motore di Rust non ha lo sguardo in avanti negato, e
/// tirarsi dietro `fancy-regex` per un solo riconoscitore costerebbe tempo di
/// compilazione e di avvio su un gancio che gira a **ogni** chiamata di
/// strumento. Trenta caratteri di scansione fanno la stessa cosa.
///
/// L'ESCLUSIONE NON È UN DETTAGLIO: sotto `.claude/projects/` stanno le
/// trascrizioni di ogni sessione, e sotto `history/` la cronologia. Leggerle è
/// un lavoro che si fa **dentro** qualunque mestiere — misurare, indagare,
/// consegnare — quindi contarle come «configurazione» accenderebbe una terza
/// area a chiunque guardi i propri registri.
pub fn is_configuration(text: &str) -> bool {
    const NEEDLE: &str = "/.claude/";
    let mut from = 0usize;
    while let Some(hit) = text[from..].find(NEEDLE) {
        let after = from + hit + NEEDLE.len();
        let rest = &text[after..];
        if !rest.starts_with("projects") && !rest.starts_with("history") {
            return true;
        }
        // Si riparte dal carattere dopo l'inizio, non dopo la fine: è la
        // ripartenza che un motore di espressioni regolari fa da sé. Nessun caso
        // noto la distingue da quella dopo la fine — perché ogni occorrenza
        // sovrapposta a una esclusa comincia con `/`, e `rest` allora non
        // comincia per `projects` — quindi è prudenza, non una correzione: sta
        // scritto qui perché non la si scambi per un ramo provato.
        from = from + hit + 1;
    }
    false
}

/// Le aree che compaiono in un testo qualsiasi.
///
/// L'insieme è ordinato per costruzione: l'originale scrive e stampa sempre
/// `sorted(...)`, e su nomi ASCII l'ordine dei byte è quello di Python.
pub fn areas_in_text(text: &str) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = roots()
        .iter()
        .filter(|(_, rx)| rx.is_match(text))
        .map(|(name, _)| name.to_string())
        .collect();
    if is_configuration(text) {
        out.insert("configurazione".to_string());
    }
    out
}

/// Il testo su cui si cercano le aree: i campi noti, uno per riga.
///
/// Solo i valori che sono **stringhe**, come l'originale: un `file_path` che
/// arriva come numero o come oggetto non si legge, e fingere di leggerlo
/// significherebbe cercare aree dentro `{}`.
pub fn tool_text(args: &serde_json::Value) -> String {
    FIELDS
        .iter()
        .filter_map(|c| args.get(c).and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Le tre promesse del gancio in una funzione pura.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Le aree della sessione dopo questa chiamata.
    pub areas: BTreeSet<String>,
    /// Se lo stato va riscritto su disco.
    pub write: bool,
    /// Se l'avviso va detto **adesso**.
    pub speak: bool,
}

/// Da ciò che la sessione aveva visto, più ciò che vede ora, alla decisione.
///
/// Separata dall'involucro per la stessa ragione di `decide()` in
/// `handoff_on_stop`: finché la decisione vive dentro un lavoro che legge
/// stdin, scrive su disco e stampa, non la prova nessuno.
///
/// E infatti non la provava nessuno. Fino al 16/08/2026 le prove dell'originale
/// erano nove e tutte verdi: sette esercitavano il riconoscimento delle aree,
/// due controllavano che l'avviso contenesse due parole. Le tre cose che
/// l'intestazione **promette** — parla alla terza area, parla una volta sola,
/// non blocca niente — non comparivano in nessuna. Un conteggio che non tocca la
/// decisione non è una prova: è un numero.
pub fn decide(seen: &BTreeSet<String>, fresh: &BTreeSet<String>, already_said: bool) -> Outcome {
    let after: BTreeSet<String> = seen.union(fresh).cloned().collect();
    if after == *seen {
        // Niente di nuovo: non si riscrive il file a ogni chiamata. Senza questo
        // ramo il gancio scriverebbe su disco decine di migliaia di volte al
        // giorno per non cambiare un byte.
        return Outcome {
            areas: after,
            write: false,
            speak: false,
        };
    }
    let speak = after.len() >= THRESHOLD && !already_said;
    Outcome {
        areas: after,
        write: true,
        speak,
    }
}

/// L'avviso, parola per parola come nell'originale.
///
/// Dice **cosa fare**, non solo cosa succede: un avviso che descrive un fatto e
/// non nomina la mossa successiva viene letto e non cambia niente.
pub fn notice(areas: &BTreeSet<String>) -> String {
    let listed = areas.iter().cloned().collect::<Vec<_>>().join(", ");
    format!(
        "AMBITI: questa sessione ne ha toccati {} — {listed}.\n\
         Due sono normali (un codice e la sua configurazione, due servizi che si \
         parlano). Tre sono di solito un altro mestiere, e le sessioni che ne \
         toccano tre o più sono quelle che diventano enormi: misurato, il 92% \
         delle sessioni oltre le 10k righe sta qui.\n\
         Se quello che stai facendo adesso NON è il lavoro per cui questa \
         sessione è nata: chiudila con la competenza `handoff` e riparti da una \
         sessione nuova, che nascerà col nome del lavoro nuovo. Il titolo di \
         questa scheda non è correggibile — Orca lo rilegge dall'`ai-title`, \
         fissato al primo prompt — quindi l'unico modo di rimettere a posto il \
         nome è chiudere. Se resti, aggiorna almeno la copia di lavoro: \
         `orca worktree set --worktree path:<percorso> --display-name \"…\"`.\n\
         Se invece le tre aree SONO il lavoro (un contratto che attraversa tre \
         servizi), va bene così: questo avviso non torna.",
        areas.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn aree(args: serde_json::Value) -> BTreeSet<String> {
        areas_in_text(&tool_text(&args))
    }

    #[test]
    fn i_sette_riconoscitori_dell_originale() {
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/suite/src/x.ts"})),
            set(&["suite"])
        );
        assert_eq!(
            aree(json!({"command": "pnpm --dir /Users/theo/gyver/work/matching-engine run test"})),
            set(&["matching-engine"])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/whatsapp/src/a.ts"})),
            set(&["whatsapp"])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/packages/geo/x.ts"})),
            set(&["packages"])
        );
        assert_eq!(
            aree(json!({"command": "docker compose -f /Users/theo/gyver/work/dev-stack/x.yml up"})),
            set(&["dev-stack"])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/.claude/rules/x.md"})),
            set(&["configurazione"])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/personal/sailor/x.ts"})),
            set(&["personale"])
        );
        assert_eq!(
            aree(json!({"command": "ls /Users/theo/orca/general"})),
            set(&["personale"])
        );
    }

    /// L'esclusione che tiene spento il gancio mentre si leggono i registri.
    #[test]
    fn i_trascritti_e_la_cronologia_non_sono_configurazione() {
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/.claude/projects/x/y.jsonl"})),
            set(&[])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/.claude/history/x"})),
            set(&[])
        );
        // ATTENZIONE, verificato contro il Python il 17/08/2026 e NON come
        // sembra: lo sguardo in avanti guarda il **prefisso**, non il segmento,
        // quindi anche una cartella che comincia come quelle resta esclusa.
        // Avevo scritto la prova al contrario, ed è stato il Python a dire
        // com'era.
        assert!(!is_configuration("/Users/theo/.claude/projectsX/y"));
        // Due occorrenze, la prima esclusa: la seconda deve comunque contare —
        // è il caso che una scansione senza sovrapposizione perderebbe.
        assert!(is_configuration(
            "/Users/theo/.claude/projects/a\n/Users/theo/.claude/settings.json"
        ));
        // Senza la barra finale non c'è nessuna corrispondenza, come nell'originale.
        assert!(!is_configuration("/Users/theo/.claude"));
    }

    #[test]
    fn un_worktree_orca_conta_come_il_suo_repo() {
        assert_eq!(
            aree(json!({"command": "git -C /Users/theo/orca/workspaces/whatsapp/x status"})),
            set(&["whatsapp"])
        );
        // QUIRK CONSERVATO: `packages` e `dev-stack` non hanno la variante
        // `workspaces/`, quindi una loro copia di lavoro non accende niente.
        // È così nell'originale e in `measure-drift.py`: cambiarlo qui
        // sposterebbe i numeri della regola senza dirlo a nessuno.
        assert_eq!(
            aree(json!({"command": "ls /Users/theo/orca/workspaces/packages/wt"})),
            set(&[])
        );
    }

    #[test]
    fn il_confine_di_parola_distingue_un_repo_da_uno_che_gli_somiglia() {
        // `\b` guarda i caratteri di parola, non i segmenti di percorso: una
        // lettera in coda esclude, un trattino no. Verificato contro il Python
        // il 17/08/2026, perché avevo scritto `suite-vecchia` fra gli esclusi e
        // il confronto lo avrebbe trovato divergente sul traffico vero — dove
        // un ramo `suite-qualcosa` è un nome normale.
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/suiteXX/x.ts"})),
            set(&[])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/suite-vecchia/x.ts"})),
            set(&["suite"])
        );
        assert_eq!(
            aree(json!({"file_path": "/Users/theo/gyver/work/suite"})),
            set(&["suite"])
        );
    }

    #[test]
    fn due_aree_in_un_comando_solo() {
        assert_eq!(
            aree(json!({"command":
                "diff /Users/theo/gyver/work/suite/a /Users/theo/gyver/work/packages/b"})),
            set(&["packages", "suite"])
        );
    }

    #[test]
    fn i_campi_si_leggono_tutti_e_solo_se_sono_stringhe() {
        // Il percorso sta in un campo diverso da `command`: vanno guardati tutti.
        assert_eq!(
            aree(json!({"path": "/Users/theo/gyver/work/suite/x"})),
            set(&["suite"])
        );
        assert_eq!(
            aree(json!({"notebook_path": "/Users/theo/gyver/work/suite/x.ipynb"})),
            set(&["suite"])
        );
        // Un campo che non è una stringa non si legge, e non deve far cadere niente.
        assert_eq!(tool_text(&json!({"command": 42, "file_path": null})), "");
        assert_eq!(tool_text(&json!({})), "");
        // Due campi pieni: si concatenano su righe diverse.
        assert_eq!(
            tool_text(&json!({"command": "a", "file_path": "b", "ignoto": "c"})),
            "a\nb"
        );
    }

    #[test]
    fn niente_di_riconoscibile_non_accende_niente() {
        assert_eq!(aree(json!({"command": "ls /tmp"})), set(&[]));
    }

    #[test]
    fn la_soglia_e_la_terza_area() {
        assert!(!decide(&set(&[]), &set(&["suite"]), false).speak);
        assert!(!decide(&set(&["suite"]), &set(&["packages"]), false).speak);
        assert!(decide(&set(&["suite", "packages"]), &set(&["configurazione"]), false).speak);
    }

    /// MUTANTE: se la soglia fosse `>` invece di `>=`, o valesse 2, questi cadono.
    #[test]
    fn due_aree_in_un_colpo_solo_non_bastano_e_tre_si() {
        assert!(!decide(&set(&[]), &set(&["suite", "packages"]), false).speak);
        assert!(decide(&set(&[]), &set(&["suite", "packages", "configurazione"]), false).speak);
    }

    /// MUTANTE: senza il flag l'avviso tornerebbe a ogni area successiva.
    #[test]
    fn detto_una_volta_non_lo_ripete() {
        let dopo = decide(
            &set(&["suite", "packages", "configurazione"]),
            &set(&["whatsapp"]),
            true,
        );
        assert!(!dopo.speak);
        // Ma lo stato si aggiorna lo stesso: l'area nuova va ricordata.
        assert!(dopo.write);
        assert_eq!(dopo.areas.len(), 4);
    }

    /// MUTANTE: senza questo si riscriverebbe il file di stato a ogni chiamata.
    #[test]
    fn niente_di_nuovo_niente_da_scrivere() {
        let fermo = decide(&set(&["suite"]), &set(&["suite"]), false);
        assert!(!fermo.write);
        assert!(!fermo.speak);
        assert!(decide(&set(&["suite"]), &set(&["packages"]), false).write);
        // Il caso che il ramo «niente di nuovo» maschererebbe: già a tre aree, e
        // non ancora detto. Se si tornasse a parlare qui, l'avviso uscirebbe a
        // ogni tocco di un'area già nota.
        let gia_tre = set(&["suite", "packages", "configurazione"]);
        let ristagno = decide(&gia_tre, &set(&["suite"]), false);
        assert!(!ristagno.write);
        assert!(!ristagno.speak);
    }

    #[test]
    fn l_avviso_dice_cosa_fare_non_solo_cosa_succede() {
        let text = notice(&set(&["configurazione", "packages", "suite"]));
        assert!(text.starts_with("AMBITI: questa sessione ne ha toccati 3 — "));
        assert!(text.contains("configurazione, packages, suite"));
        assert!(text.contains("handoff"));
        assert!(text.contains("--display-name"));
        // Quattro aree: il conteggio è quello vero, non la soglia.
        assert!(notice(&set(&["a", "b", "c", "d"])).contains("toccati 4"));
    }

    #[test]
    fn le_radici_sono_sette_come_in_measure_drift() {
        // Sei riconoscitori più `configurazione`, che è scritta a mano.
        assert_eq!(roots().len() + 1, 7);
    }
}
