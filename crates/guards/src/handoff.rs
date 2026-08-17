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
        return field("input_tokens")
            + field("cache_read_input_tokens")
            + field("cache_creation_input_tokens");
    }
    0
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
