//! Il canarino del formato dei transcript: muore rumorosamente al posto delle
//! misure che leggono i JSONL di Claude Code.
//!
//! PERCHÉ ESISTE. Costi, consegna, sessione lunga, ripartenze e staffetta
//! leggono tutti lo stesso file — `~/.claude/projects/<slug>/<sessione>.jsonl` —
//! e ogni lettore salta in silenzio la riga che non capisce. È la scelta giusta
//! per un gancio (una riga tagliata dal `seek` non deve fermare niente), ma
//! rende un cambio di schema indistinguibile da un file sano: se Anthropic
//! rinomina `message.usage`, i conteggi vanno a zero e nessuno se ne accorge.
//! Qui le assunzioni sono scritte una per una e provate su righe vere: quando
//! cadono, si dice QUALE campo è cambiato e in quale riga del campione.
//!
//! Giudizio puro: entrano le righe, esce il rapporto. Chi cerca il transcript
//! sul disco e chi decide il codice d'uscita sta in
//! `claude-hooks/src/transcript_canary.rs`.

use crate::stale_facts::Date;
use serde_json::Value;

/// Un'assunzione condivisa dai lettori di transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assumption {
    /// Nome stabile, citato dai messaggi di morte del canarino.
    pub id: &'static str,
    /// Il campo, nella forma in cui lo scrive chi lo legge.
    pub field: &'static str,
    /// La forma attesa.
    pub shape: &'static str,
    /// Chi si rompe se cade.
    pub readers: &'static str,
}

/// Le assunzioni censite il 24/08/2026 sui lettori di transcript di questa casa.
///
/// L'elenco è a mano perché è un contratto con un formato che non controlliamo:
/// dedurlo dal codice darebbe la forma che i lettori *tollerano*, non quella su
/// cui contano. Ogni voce nomina chi si rompe, così una morte del canarino dice
/// subito quali misure stanno già mentendo.
pub const ASSUMPTIONS: &[Assumption] = &[
    Assumption {
        id: "jsonl",
        field: "<riga>",
        shape: "un oggetto JSON per riga",
        readers: "tutti i lettori",
    },
    Assumption {
        id: "type",
        field: "type",
        shape: "stringa su ogni record",
        readers: "cost_ledger, handoff, restart, turn_status",
    },
    Assumption {
        id: "type-assistant",
        field: "type == \"assistant\"",
        shape: "esiste nel campione",
        readers: "cost_ledger, resume_point, turn_status",
    },
    Assumption {
        id: "type-user",
        field: "type == \"user\"",
        shape: "esiste nel campione",
        readers: "worked_after_handoff, restart, turn_status",
    },
    Assumption {
        id: "timestamp",
        field: "timestamp",
        shape: "ISO-8601 UTC `YYYY-MM-DDTHH:MM:SS...Z`",
        readers: "worked_after_handoff, last_wakeup, cost_ledger",
    },
    Assumption {
        id: "session-id",
        field: "sessionId",
        shape: "stringa",
        readers: "cost_ledger",
    },
    Assumption {
        id: "is-sidechain",
        field: "isSidechain",
        shape: "booleano",
        readers: "cost_ledger (turni dei subagent)",
    },
    Assumption {
        id: "message",
        field: "message",
        shape: "oggetto sui record `assistant` e `user`",
        readers: "tutti i lettori",
    },
    Assumption {
        id: "message-id",
        field: "message.id",
        shape: "stringa",
        readers: "cost_ledger (deduplica dello streaming), long_session",
    },
    Assumption {
        id: "message-model",
        field: "message.model",
        shape: "stringa",
        readers: "model_from_lines (budget e soglie), cost_ledger",
    },
    Assumption {
        id: "usage",
        field: "message.usage",
        shape: "oggetto",
        readers: "context_used, long_session, cost_ledger",
    },
    Assumption {
        id: "usage-fields",
        field: "message.usage.{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}",
        shape: "interi non negativi",
        readers: "context_used (somma dei tre d'ingresso), cost_ledger",
    },
    Assumption {
        id: "content-array",
        field: "message.content (assistant)",
        shape: "array di blocchi",
        readers: "resume_point, last_wakeup, turn_status, cost_ledger",
    },
    Assumption {
        id: "content-user",
        field: "message.content (user)",
        shape: "array di blocchi oppure stringa",
        readers: "worked_after_handoff, restart",
    },
    Assumption {
        id: "block-type",
        field: "message.content[].type",
        shape: "stringa su ogni blocco",
        readers: "tutti i lettori del contenuto",
    },
    Assumption {
        id: "block-text",
        field: "blocco `text` → campo `text`",
        shape: "stringa",
        readers: "resume_point, worked_after_handoff, restart",
    },
    Assumption {
        id: "block-tool-use",
        field: "blocco `tool_use` → campi `name` e `input`",
        shape: "stringa e oggetto",
        readers: "last_wakeup (ScheduleWakeup), cost_ledger (primo strumento)",
    },
    Assumption {
        id: "block-tool-result",
        field: "blocco `tool_result` nel contenuto `user`",
        shape: "esiste nel campione",
        readers: "worked_after_handoff (distingue il lavoro vero dagli esiti)",
    },
];

/// Quanti record `assistant` servono perché un'assenza valga come prova.
///
/// Sotto questa soglia un campo può mancare per caso — un campione corto pescato
/// in una coda di sole annotazioni — e gridare sarebbe un falso allarme. Il
/// canarino allora non dice «vivo»: dice «non ho misurato».
pub const MIN_ASSISTANT: usize = 5;

/// Come sopra per i record `user`: ne basta uno, ma zero non è un campione.
pub const MIN_USER: usize = 1;

/// Quante righe si stampano per una stessa assunzione caduta, prima di contarle.
pub const MAX_LINES_PER_ASSUMPTION: usize = 3;

/// Un'assunzione caduta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// L'`id` della voce in [`ASSUMPTIONS`].
    pub assumption: &'static str,
    /// Il campo che è cambiato.
    pub field: String,
    /// La riga del campione (1 = prima riga passata), se il difetto è di forma.
    /// `None` quando il campo è sparito dall'intero campione.
    pub line: Option<usize>,
    pub detail: String,
}

/// Un'assenza che non basta a condannare: la forma non è stata violata, quel
/// pezzo di schema semplicemente non compare nel campione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub assumption: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Le assunzioni reggono.
    Alive,
    /// Almeno una è caduta: le misure che la usano stanno già mentendo.
    Dead,
    /// Campione troppo povero per giudicare. Non è un verde.
    NotMeasured,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub lines: usize,
    pub records: usize,
    pub unreadable: usize,
    pub assistant: usize,
    pub user: usize,
    pub findings: Vec<Finding>,
    pub notes: Vec<Note>,
}

impl Report {
    pub fn verdict(&self) -> Verdict {
        if !self.findings.is_empty() {
            return Verdict::Dead;
        }
        if self.assistant < MIN_ASSISTANT || self.user < MIN_USER {
            return Verdict::NotMeasured;
        }
        Verdict::Alive
    }
}

/// Ciò che il campione ha visto almeno una volta.
#[derive(Default)]
struct Seen {
    timestamp: bool,
    session_id: bool,
    message: bool,
    message_id: bool,
    model: bool,
    usage: bool,
    usage_field: [bool; 4],
    content_array: bool,
    content_user: bool,
    block_type: bool,
    text_block: bool,
    tool_use_block: bool,
    tool_result_block: bool,
}

/// I quattro campi di `usage` su cui poggia ogni misura di contesto e di costo.
const USAGE_FIELDS: [&str; 4] = [
    "input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
];

/// Le assunzioni provate riga per riga sul campione.
///
/// Due difetti diversi, tenuti separati apposta: un campo **di forma sbagliata**
/// condanna da solo e porta il numero di riga (è il caso «content da array a
/// stringa»); un campo **sparito dall'intero campione** condanna solo se il
/// campione era abbastanza ricco da doverlo contenere (è il caso «campo
/// rinominato»). Il resto — un blocco `tool_use` che in questa coda non c'è — è
/// una nota, perché una sessione può davvero non averne usati.
pub fn check(lines: &[&str]) -> Report {
    let mut report = Report { lines: lines.len(), ..Report::default() };
    let mut seen = Seen::default();
    let last = lines.len().saturating_sub(1);

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let no = i + 1;
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                report.unreadable += 1;
                // L'ultima riga può essere un record a metà scrittura: il file
                // cresce mentre lo leggiamo, e quello non è un cambio di schema.
                if i != last {
                    report.findings.push(Finding {
                        assumption: "jsonl",
                        field: "<riga>".into(),
                        line: Some(no),
                        detail: format!("non è JSON: {e}"),
                    });
                }
                continue;
            }
        };
        report.records += 1;
        let Some(obj) = value.as_object() else {
            report.findings.push(Finding {
                assumption: "jsonl",
                field: "<riga>".into(),
                line: Some(no),
                detail: "JSON valido ma non è un oggetto".into(),
            });
            continue;
        };

        let kind = match obj.get("type") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            other => {
                report.findings.push(Finding {
                    assumption: "type",
                    field: "type".into(),
                    line: Some(no),
                    detail: describe("stringa non vuota", other),
                });
                continue;
            }
        };

        check_root_fields(obj, no, &mut seen, &mut report);

        // Le annotazioni che i ganci di questa casa appendono in coda — `system`,
        // `attachment`, `atis-latch` — non sono turni e non promettono niente:
        // oltre ai campi di radice qui sopra non c'è nulla da controllare.
        if kind != "assistant" && kind != "user" {
            continue;
        }
        if kind == "assistant" {
            report.assistant += 1;
        } else {
            report.user += 1;
        }

        // UN CAMPO ASSENTE NON CONDANNA MAI DA SOLO, qui e più sotto: chi legge
        // salta il record e basta, e un turno senza `message` può essere una
        // riga speciale che nessuno ci ha detto. Un campo **rinominato** sparisce
        // invece da tutto il campione, e lo prende `check_presence`; un campo
        // presente con la forma sbagliata condanna subito, perché quello i
        // lettori lo interpretano male in silenzio.
        let message = match obj.get("message") {
            Some(Value::Object(m)) => {
                seen.message = true;
                m
            }
            None => continue,
            other => {
                report.findings.push(Finding {
                    assumption: "message",
                    field: "message".into(),
                    line: Some(no),
                    detail: format!("su un record `{kind}`: {}", describe("oggetto", other)),
                });
                continue;
            }
        };

        if kind == "assistant" {
            check_id_and_model(message, no, &mut seen, &mut report);
            check_usage(message, no, &mut seen, &mut report);
        }
        check_content(message, &kind, no, &mut seen, &mut report);
    }

    check_presence(&seen, &mut report);
    report
}

/// I campi che stanno alla radice del record, non dentro `message`.
fn check_root_fields(
    obj: &serde_json::Map<String, Value>,
    no: usize,
    seen: &mut Seen,
    report: &mut Report,
) {
    if let Some(ts) = obj.get("timestamp") {
        match ts.as_str().filter(|s| is_iso_utc(s)) {
            Some(_) => seen.timestamp = true,
            None => report.findings.push(Finding {
                assumption: "timestamp",
                field: "timestamp".into(),
                line: Some(no),
                detail: describe("ISO-8601 UTC `YYYY-MM-DDTHH:MM:SS...Z`", Some(ts)),
            }),
        }
    }
    if let Some(session) = obj.get("sessionId") {
        match session.as_str() {
            Some(_) => seen.session_id = true,
            None => report.findings.push(Finding {
                assumption: "session-id",
                field: "sessionId".into(),
                line: Some(no),
                detail: describe("stringa", Some(session)),
            }),
        }
    }
    if let Some(side) = obj.get("isSidechain") {
        if !side.is_boolean() {
            report.findings.push(Finding {
                assumption: "is-sidechain",
                field: "isSidechain".into(),
                line: Some(no),
                detail: describe("booleano", Some(side)),
            });
        }
    }
}

fn check_id_and_model(
    message: &serde_json::Map<String, Value>,
    no: usize,
    seen: &mut Seen,
    report: &mut Report,
) {
    for (key, id) in [("id", "message-id"), ("model", "message-model")] {
        let Some(value) = message.get(key) else { continue };
        if value.as_str().is_none() {
            report.findings.push(Finding {
                assumption: id,
                field: format!("message.{key}"),
                line: Some(no),
                detail: describe("stringa", Some(value)),
            });
            continue;
        }
        if key == "id" {
            seen.message_id = true;
        } else {
            seen.model = true;
        }
    }
}

fn check_usage(
    message: &serde_json::Map<String, Value>,
    no: usize,
    seen: &mut Seen,
    report: &mut Report,
) {
    let Some(usage) = message.get("usage") else { return };
    let Some(fields) = usage.as_object() else {
        report.findings.push(Finding {
            assumption: "usage",
            field: "message.usage".into(),
            line: Some(no),
            detail: describe("oggetto", Some(usage)),
        });
        return;
    };
    seen.usage = true;
    for (k, name) in USAGE_FIELDS.iter().enumerate() {
        let Some(value) = fields.get(*name) else { continue };
        if value.as_u64().is_some() {
            seen.usage_field[k] = true;
        } else {
            report.findings.push(Finding {
                assumption: "usage-fields",
                field: format!("message.usage.{name}"),
                line: Some(no),
                detail: describe("intero non negativo", Some(value)),
            });
        }
    }
}

fn check_content(
    message: &serde_json::Map<String, Value>,
    kind: &str,
    no: usize,
    seen: &mut Seen,
    report: &mut Report,
) {
    let assistant = kind == "assistant";
    match message.get("content") {
        Some(Value::Array(blocks)) => {
            if assistant {
                seen.content_array = true;
            } else {
                seen.content_user = true;
            }
            check_blocks(blocks, no, seen, report);
        }
        // Solo il contenuto di un `user` può essere una stringa nuda: è la forma
        // di un prompt digitato a mano, e i lettori la accettano già.
        Some(Value::String(_)) if !assistant => seen.content_user = true,
        None => {}
        other => report.findings.push(Finding {
            assumption: if assistant { "content-array" } else { "content-user" },
            field: format!("message.content ({kind})"),
            line: Some(no),
            detail: describe(if assistant { "array" } else { "array o stringa" }, other),
        }),
    }
}

fn check_blocks(blocks: &[Value], no: usize, seen: &mut Seen, report: &mut Report) {
    for raw in blocks {
        let Some(block) = raw.as_object() else {
            report.findings.push(Finding {
                assumption: "block-type",
                field: "message.content[]".into(),
                line: Some(no),
                detail: describe("oggetto", Some(raw)),
            });
            continue;
        };
        let Some(kind) = block.get("type").and_then(|v| v.as_str()) else {
            report.findings.push(Finding {
                assumption: "block-type",
                field: "message.content[].type".into(),
                line: Some(no),
                detail: describe("stringa", block.get("type")),
            });
            continue;
        };
        seen.block_type = true;
        match kind {
            "text" => match block.get("text") {
                Some(Value::String(_)) => seen.text_block = true,
                other => report.findings.push(Finding {
                    assumption: "block-text",
                    field: "message.content[].text".into(),
                    line: Some(no),
                    detail: describe("stringa nel blocco `text`", other),
                }),
            },
            "tool_use" => {
                let named = block.get("name").is_some_and(|v| v.is_string());
                let has_input = block.get("input").is_some_and(|v| v.is_object());
                if named && has_input {
                    seen.tool_use_block = true;
                } else if named {
                    report.findings.push(Finding {
                        assumption: "block-tool-use",
                        field: "message.content[].input".into(),
                        line: Some(no),
                        detail: describe("oggetto nel blocco `tool_use`", block.get("input")),
                    });
                } else {
                    report.findings.push(Finding {
                        assumption: "block-tool-use",
                        field: "message.content[].name".into(),
                        line: Some(no),
                        detail: describe("stringa nel blocco `tool_use`", block.get("name")),
                    });
                }
            }
            "tool_result" => seen.tool_result_block = true,
            _ => {}
        }
    }
}

/// Le assunzioni che si provano solo sull'insieme: un campo sparito da tutto il
/// campione è un campo rinominato.
fn check_presence(seen: &Seen, report: &mut Report) {
    // Con un campione povero l'assenza non prova niente: il verdetto sarà «non
    // misurato», e condannare qui lo trasformerebbe in una morte falsa.
    if report.assistant < MIN_ASSISTANT || report.user < MIN_USER {
        return;
    }
    let mut vanished = |present: bool, assumption: &'static str, field: &str, detail: &str| {
        if !present {
            report.findings.push(Finding {
                assumption,
                field: field.to_string(),
                line: None,
                detail: detail.to_string(),
            });
        }
    };
    vanished(seen.message, "message", "message", "nessun record `assistant`/`user` lo porta");
    vanished(seen.timestamp, "timestamp", "timestamp", "nessun record lo porta");
    vanished(seen.session_id, "session-id", "sessionId", "nessun record lo porta");
    vanished(seen.message_id, "message-id", "message.id", "nessun turno `assistant` lo porta");
    vanished(seen.model, "message-model", "message.model", "nessun turno `assistant` lo porta");
    vanished(seen.usage, "usage", "message.usage", "nessun turno `assistant` lo porta");
    vanished(
        seen.content_array,
        "content-array",
        "message.content (assistant)",
        "nessun turno `assistant` porta un array di blocchi",
    );
    vanished(
        seen.content_user,
        "content-user",
        "message.content (user)",
        "nessun record `user` porta un array o una stringa",
    );
    vanished(seen.block_type, "block-type", "message.content[].type", "nessun blocco lo porta");
    for (k, name) in USAGE_FIELDS.iter().enumerate() {
        if !seen.usage_field[k] {
            report.findings.push(Finding {
                assumption: "usage-fields",
                field: format!("message.usage.{name}"),
                line: None,
                detail: "nessun `usage` del campione lo porta".into(),
            });
        }
    }

    // Le tre che non condannano: dipendono da cosa la sessione ha fatto, non da
    // come Anthropic scrive il file.
    for (present, assumption, detail) in [
        (seen.text_block, "block-text", "nessun blocco `text` nel campione"),
        (seen.tool_use_block, "block-tool-use", "nessun blocco `tool_use` nel campione"),
        (seen.tool_result_block, "block-tool-result", "nessun blocco `tool_result` nel campione"),
    ] {
        if !present {
            report.notes.push(Note { assumption, detail: detail.to_string() });
        }
    }
}

/// «atteso X, trovato Y»: il messaggio che nomina il campo cambiato.
fn describe(expected: &str, found: Option<&Value>) -> String {
    let got = match found {
        None => "assente".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(_)) => "booleano".to_string(),
        Some(Value::Number(n)) => format!("numero ({n})"),
        Some(Value::String(s)) => {
            let head: String = s.chars().take(40).collect();
            format!("stringa (\"{head}\")")
        }
        Some(Value::Array(_)) => "array".to_string(),
        Some(Value::Object(_)) => "oggetto".to_string(),
    };
    format!("atteso {expected}, trovato {got}")
}

/// La forma che `claude_hooks::handoff::epoch_from_iso` pretende: separatori al
/// loro posto, data che esiste davvero, ora numerica. Un timestamp che non la
/// rispetta non è «quasi giusto»: là dentro diventa `None`, e la guardia che lo
/// usa si spegne restando scritta.
fn is_iso_utc(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':'
    {
        return false;
    }
    let num = |a: usize, z: usize| s.get(a..z).and_then(|x| x.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day)) = (num(0, 4), num(5, 7), num(8, 10)) else {
        return false;
    };
    if Date::new(year, month, day).is_none() {
        return false;
    }
    let (Some(hour), Some(minute), Some(second)) = (num(11, 13), num(14, 16), num(17, 19)) else {
        return false;
    };
    // Il 60 esiste: è il secondo intercalare, e rifiutarlo sarebbe un falso
    // allarme una notte ogni qualche anno.
    (0..24).contains(&hour) && (0..60).contains(&minute) && (0..61).contains(&second)
}

/// Il rapporto in chiaro: cosa è stato letto, cosa è caduto, chi si rompe.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "canarino del formato transcript: {} righe, {} record ({} assistant, {} user), {} illeggibili\n",
        report.lines, report.records, report.assistant, report.user, report.unreadable
    ));
    match report.verdict() {
        Verdict::Alive => out.push_str(&format!(
            "VIVO: le {} assunzioni reggono sul campione.\n",
            ASSUMPTIONS.len()
        )),
        Verdict::NotMeasured => out.push_str(&format!(
            "NON MISURATO: servono almeno {MIN_ASSISTANT} record `assistant` e {MIN_USER} `user`, il campione ne ha {} e {}.\n",
            report.assistant, report.user
        )),
        Verdict::Dead => out.push_str(&format!(
            "MORTO: {} assunzioni cadute — le misure che le usano stanno già mentendo.\n",
            report.findings.len()
        )),
    }
    // Un campo cambiato cade su OGNI riga che lo porta: senza tetto, un rapporto
    // vero sono cinquanta righe identiche e chi legge non trova le altre
    // assunzioni cadute in mezzo. Si mostrano le prime, e le altre si contano.
    let mut shown: Vec<(&str, usize)> = Vec::new();
    for finding in &report.findings {
        let seen = match shown.iter_mut().find(|(id, _)| *id == finding.assumption) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                shown.push((finding.assumption, 1));
                1
            }
        };
        if seen > MAX_LINES_PER_ASSUMPTION {
            continue;
        }
        let at = match finding.line {
            Some(n) => format!("riga {n} del campione"),
            None => "in tutto il campione".to_string(),
        };
        let readers = ASSUMPTIONS
            .iter()
            .find(|a| a.id == finding.assumption)
            .map(|a| a.readers)
            .unwrap_or("?");
        out.push_str(&format!(
            "  [{}] {} — {} ({at}); si rompono: {readers}\n",
            finding.assumption, finding.field, finding.detail
        ));
    }
    for (id, count) in shown {
        if count > MAX_LINES_PER_ASSUMPTION {
            out.push_str(&format!(
                "  [{id}] … e altre {} righe con la stessa assunzione caduta\n",
                count - MAX_LINES_PER_ASSUMPTION
            ));
        }
    }
    for note in &report.notes {
        out.push_str(&format!("  nota [{}] {}\n", note.assumption, note.detail));
    }
    out
}

/// La tabella delle assunzioni, per chi vuole leggerle senza aprire il sorgente.
pub fn render_assumptions() -> String {
    let mut out = String::new();
    for a in ASSUMPTIONS {
        out.push_str(&format!("{:<18} {} — {} · {}\n", a.id, a.field, a.shape, a.readers));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un campione sano: sei turni `assistant` completi e due `user`.
    fn healthy_sample() -> Vec<String> {
        let mut lines = Vec::new();
        for i in 0..6 {
            lines.push(
                serde_json::json!({
                    "type": "assistant",
                    "timestamp": format!("2026-08-24T01:0{i}:11.123Z"),
                    "sessionId": "abc",
                    "isSidechain": false,
                    "message": {
                        "id": format!("msg_{i}"),
                        "model": "claude-opus-5",
                        "usage": {
                            "input_tokens": 10,
                            "output_tokens": 20,
                            "cache_read_input_tokens": 30,
                            "cache_creation_input_tokens": 40
                        },
                        "content": [
                            {"type": "text", "text": "Procedo con il canarino"},
                            {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}
                        ]
                    }
                })
                .to_string(),
            );
        }
        lines.push(
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-08-24T01:07:11.123Z",
                "sessionId": "abc",
                "message": {"content": [{"type": "tool_result", "content": "ok"}]}
            })
            .to_string(),
        );
        lines.push(
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-08-24T01:08:11.123Z",
                "sessionId": "abc",
                "message": {"content": "un prompt scritto a mano"}
            })
            .to_string(),
        );
        // Un'annotazione appesa da un gancio di casa: non è un turno.
        lines.push(r#"{"type":"atis-latch","sessionId":"abc"}"#.to_string());
        lines
    }

    /// Il campione più povero che il canarino accetta di giudicare: esattamente
    /// `MIN_ASSISTANT` turni `assistant` e `MIN_USER` record `user`. Le due
    /// soglie si provano sul numero esatto — con un campione più ricco un `<=`
    /// al posto del `<` passa inosservato in tutte e due le direzioni.
    fn minimal_sample() -> Vec<String> {
        let full = healthy_sample();
        let of_kind = |kind: &str, how_many: usize| -> Vec<String> {
            full.iter()
                .filter(|l| l.contains(&format!("\"type\":\"{kind}\"")))
                .take(how_many)
                .cloned()
                .collect()
        };
        [of_kind("assistant", MIN_ASSISTANT), of_kind("user", MIN_USER)].concat()
    }

    fn check_owned(lines: &[String]) -> Report {
        check(&lines.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// Lo stesso campione sano con un solo timestamp riscritto: le forme storte
    /// si provano una per volta, su una riga sola, così il difetto ha un numero.
    fn with_timestamp(replacement: &str) -> Vec<String> {
        healthy_sample()
            .iter()
            .map(|l| l.replace(HEALTHY_TIMESTAMP, replacement))
            .collect()
    }

    /// Il timestamp del primo record `user` del campione sano.
    const HEALTHY_TIMESTAMP: &str = "2026-08-24T01:07:11.123Z";

    fn rewritten(from: &str, to: &str) -> Vec<String> {
        healthy_sample().iter().map(|l| l.replace(from, to)).collect()
    }

    #[test]
    fn a_healthy_sample_keeps_the_canary_alive() {
        let sample = healthy_sample();
        let report = check_owned(&sample);
        assert_eq!(report.verdict(), Verdict::Alive, "{}", render(&report));
        assert_eq!(report.assistant, 6);
        assert_eq!(report.user, 2);
        assert!(report.notes.is_empty(), "{:?}", report.notes);
        // rotto così → rosso: conteggi in testa che restano a zero fanno dire
        // «0 righe, 0 record» in cima a un rapporto pieno
        assert_eq!((report.lines, report.records), (sample.len(), sample.len()));
        assert_eq!(report.unreadable, 0);
    }

    /// Le due soglie del verdetto si provano sul numero esatto che ammettono.
    #[test]
    fn the_smallest_sample_the_canary_will_judge_is_alive() {
        let report = check_owned(&minimal_sample());
        assert_eq!((report.assistant, report.user), (MIN_ASSISTANT, MIN_USER));
        // rotto così → rosso: `<=` su una delle due soglie dichiara «non
        // misurato» proprio il campione che le soglie ammettono
        assert_eq!(report.verdict(), Verdict::Alive, "{}", render(&report));
    }

    /// E sul campione minimo la verifica d'insieme deve girare davvero: è lì che
    /// si vede un campo rinominato, che riga per riga non lascia traccia.
    #[test]
    fn the_smallest_sample_still_condemns_a_vanished_field() {
        let sample: Vec<String> =
            minimal_sample().iter().map(|l| l.replace("\"usage\"", "\"tokenUsage\"")).collect();
        let report = check_owned(&sample);
        // rotto così → rosso: una soglia `<=` fa uscire subito la verifica
        // d'insieme, e il campo sparito passa per un campione sano
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        assert!(report.findings.iter().any(|f| f.assumption == "usage"), "{:?}", report.findings);
    }

    /// `"type": ""` è una stringa, ma non è un tipo di record: chi legge non
    /// riconosce più né i turni `assistant` né gli `user`.
    #[test]
    fn an_empty_record_type_kills_the_canary() {
        let mut sample = healthy_sample();
        sample[1] = sample[1].replace("\"type\":\"assistant\"", "\"type\":\"\"");
        let report = check_owned(&sample);
        // rotto così → rosso: accettare la stringa vuota fa passare il record
        // per un'annotazione qualunque, e nessuno dice che `type` è cambiato
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let finding = &report.findings[0];
        assert_eq!((finding.assumption, finding.line), ("type", Some(2)));
    }

    /// `message.id` e `message.model` non si scambiano: se sparisce il modello,
    /// il rapporto deve nominare il modello.
    #[test]
    fn a_vanished_model_is_named_instead_of_the_id() {
        let sample: Vec<String> = healthy_sample()
            .iter()
            .map(|l| l.replace("\"model\":", "\"modelName\":"))
            .collect();
        let report = check_owned(&sample);
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        // rotto così → rosso: scambiare i due campi dichiara sparito quello che
        // c'è, e sano quello che manca
        assert!(
            report.findings.iter().any(|f| f.assumption == "message-model"),
            "{:?}",
            report.findings
        );
        assert!(
            !report.findings.iter().any(|f| f.assumption == "message-id"),
            "{:?}",
            report.findings
        );
    }

    /// Un blocco `tool_use` col nome ma senza `input`: manca proprio ciò che i
    /// lettori vanno a prendere là dentro.
    #[test]
    fn a_tool_use_without_its_input_kills_the_canary() {
        let mut sample = healthy_sample();
        sample[0] = sample[0].replace(r#","input":{"command":"ls"}"#, "");
        let report = check_owned(&sample);
        // rotto così → rosso: `nome oppure input` conta per buono un blocco
        // dimezzato, e la nota «nessun tool_use» non arriva perché uno c'era
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let finding = &report.findings[0];
        assert_eq!(finding.assumption, "block-tool-use");
        assert_eq!(finding.field, "message.content[].input");
        assert_eq!(finding.line, Some(1));
    }

    /// Un separatore fuori posto per volta. Ognuno di questi timestamp diventa
    /// `None` in `epoch_from_iso`, e la guardia che lo usa si spegne restando
    /// scritta: il canarino deve gridare su tutti e cinque.
    #[test]
    fn a_timestamp_broken_at_any_separator_kills_the_canary() {
        for position in [4, 7, 10, 13, 16] {
            let mut bad: Vec<char> = HEALTHY_TIMESTAMP.chars().collect();
            bad[position] = 'X';
            let bad: String = bad.into_iter().collect();
            let report = check_owned(&with_timestamp(&bad));
            // rotto così → rosso: un `&&` al posto di un `||` fa bastare due
            // separatori storti insieme, e uno solo non condanna più
            assert_eq!(report.verdict(), Verdict::Dead, "byte {position}: {}", render(&report));
            assert!(
                report.findings.iter().any(|f| f.assumption == "timestamp"),
                "byte {position}: {:?}",
                report.findings
            );
        }
    }

    /// Ora, minuto e secondo fuori scala: la data è scritta bene ma non esiste.
    #[test]
    fn a_time_out_of_range_kills_the_canary() {
        for bad in [
            "2026-08-24T99:07:11.123Z",
            "2026-08-24T01:99:11.123Z",
            "2026-08-24T01:07:99.123Z",
        ] {
            let report = check_owned(&with_timestamp(bad));
            // rotto così → rosso: un `||` al posto di un `&&` fa bastare che uno
            // solo dei tre sia in scala
            assert_eq!(report.verdict(), Verdict::Dead, "«{bad}»: {}", render(&report));
        }
    }

    /// Diciannove byte sono la forma più corta che `epoch_from_iso` legge
    /// ancora: il canarino non deve gridare su un transcript sano che li porta.
    #[test]
    fn a_timestamp_of_exactly_nineteen_bytes_is_still_read() {
        let report = check_owned(&with_timestamp("2026-08-24T01:07:11"));
        // rotto così → rosso: `<= 19` rifiuta il caso limite e il canarino
        // muore su un campione che i lettori interpretano benissimo
        assert_eq!(report.verdict(), Verdict::Alive, "{}", render(&report));
        // Sotto quella soglia l'ora non c'è più, e quello è un difetto vero.
        let report = check_owned(&with_timestamp("2026-08-24"));
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
    }

    /// Il tetto per assunzione non scatta quando non ha nascosto niente: tre
    /// righe cadute sono tre righe stampate, e nessun conto in fondo.
    #[test]
    fn exactly_three_repeats_are_all_shown_without_a_tally_line() {
        let mut sample = healthy_sample();
        for line in sample.iter_mut().take(MAX_LINES_PER_ASSUMPTION) {
            *line = line.replace("\"input_tokens\":10", "\"input_tokens\":\"10\"");
        }
        let report = check_owned(&sample);
        assert_eq!(report.findings.len(), MAX_LINES_PER_ASSUMPTION, "{}", render(&report));
        let text = render(&report);
        // rotto così → rosso: `>=` aggiunge «… e altre 0 righe» proprio quando
        // il tetto non ha tagliato niente
        assert!(!text.contains("e altre"), "{text}");
        assert_eq!(
            text.matches("message.usage.input_tokens").count(),
            MAX_LINES_PER_ASSUMPTION,
            "{text}"
        );
    }

    #[test]
    fn a_renamed_field_kills_the_canary_and_gets_named() {
        let report = check_owned(&rewritten("\"usage\"", "\"tokenUsage\""));
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let text = render(&report);
        assert!(text.contains("message.usage"), "{text}");
        assert!(text.contains("input_tokens"), "{text}");
        // rotto così → rosso: cercare in tabella l'id **diverso** da quello
        // caduto stampa i lettori di un'altra assunzione, e il rapporto accusa
        // chi non si rompe
        let usage = ASSUMPTIONS.iter().find(|a| a.id == "usage").expect("assunzione censita");
        assert!(text.contains(&format!("si rompono: {}", usage.readers)), "{text}");
    }

    #[test]
    fn content_turned_from_array_to_string_names_the_line() {
        let mut lines = healthy_sample();
        lines[2] = lines[2].replace(
            r#""content":[{"type":"text","text":"Procedo con il canarino"},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]"#,
            r#""content":"Procedo con il canarino""#,
        );
        let report = check_owned(&lines);
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let finding = &report.findings[0];
        assert_eq!(finding.assumption, "content-array");
        assert_eq!(finding.line, Some(3), "la riga del campione va nominata");
        assert!(finding.detail.contains("atteso array"), "{}", finding.detail);
    }

    #[test]
    fn an_integer_turned_into_a_string_kills_the_canary() {
        let report = check_owned(&rewritten("\"input_tokens\":10", "\"input_tokens\":\"10\""));
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let text = render(&report);
        assert!(text.contains("message.usage.input_tokens"), "{text}");
    }

    #[test]
    fn a_vanished_record_type_is_not_measured_rather_than_green() {
        let report = check_owned(&rewritten("\"type\":\"assistant\"", "\"type\":\"model\""));
        assert_eq!(report.verdict(), Verdict::NotMeasured, "{}", render(&report));
        assert_eq!(report.assistant, 0);
    }

    #[test]
    fn a_timestamp_of_a_new_shape_kills_the_canary() {
        let report = check_owned(&rewritten("2026-08-24T01:07:11.123Z", "1787052662180"));
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        let text = render(&report);
        assert!(text.contains("timestamp"), "{text}");
    }

    #[test]
    fn an_unreadable_line_condemns_unless_it_is_the_last_one() {
        let mut middle = healthy_sample();
        middle.insert(3, "{\"type\":\"assistant\", troncata".to_string());
        let report = check_owned(&middle);
        assert_eq!(report.verdict(), Verdict::Dead, "{}", render(&report));
        assert_eq!(report.findings[0].line, Some(4));

        let mut tail = healthy_sample();
        tail.push("{\"type\":\"assistant\", a metà scrittura".to_string());
        let report = check_owned(&tail);
        assert_eq!(report.verdict(), Verdict::Alive, "{}", render(&report));
        assert_eq!(report.unreadable, 1);
    }

    #[test]
    fn a_thin_sample_is_never_green() {
        let report = check(&[r#"{"type":"assistant","message":{"id":"a","model":"m"}}"#]);
        assert_eq!(report.verdict(), Verdict::NotMeasured, "{}", render(&report));
        assert!(report.findings.is_empty(), "un campione corto non condanna: {:?}", report.findings);
    }

    #[test]
    fn a_session_without_tools_leaves_a_note_not_a_death() {
        let lines: Vec<String> = healthy_sample()
            .iter()
            .map(|l| {
                l.replace(r#",{"type":"tool_use","name":"Bash","input":{"command":"ls"}}"#, "")
                    .replace(
                        r#"{"type":"tool_result","content":"ok"}"#,
                        r#"{"type":"text","text":"ciao"}"#,
                    )
            })
            .collect();
        let report = check_owned(&lines);
        assert_eq!(report.verdict(), Verdict::Alive, "{}", render(&report));
        assert_eq!(report.notes.len(), 2, "{:?}", report.notes);
    }

    #[test]
    fn the_report_counts_the_repeats_instead_of_reprinting_them() {
        // Un campo cambiato cade su ogni riga: il rapporto deve restare leggibile.
        let lines: Vec<String> = healthy_sample()
            .iter()
            .map(|l| l.replace("\"input_tokens\":10", "\"input_tokens\":\"10\""))
            .collect();
        let report = check_owned(&lines);
        // Sei righe con la forma sbagliata, più la constatazione che nel
        // campione non è rimasto un solo `input_tokens` buono.
        assert_eq!(report.findings.len(), 7, "{}", render(&report));
        let text = render(&report);
        assert_eq!(
            text.matches("message.usage.input_tokens").count(),
            MAX_LINES_PER_ASSUMPTION,
            "{text}"
        );
        assert!(text.contains("e altre 4 righe"), "{text}");
    }

    #[test]
    fn every_assumption_has_a_unique_id_and_a_reader() {
        let mut ids: Vec<&str> = ASSUMPTIONS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        let total = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), total, "id doppio fra le assunzioni");
        assert!(ASSUMPTIONS.iter().all(|a| !a.readers.is_empty()));
        assert_eq!(render_assumptions().lines().count(), ASSUMPTIONS.len());
    }

    #[test]
    fn every_id_a_finding_cites_exists_in_the_table() {
        // Il rapporto cerca `readers` per id: un id scritto a mano e non censito
        // stamperebbe «?» al posto di chi si rompe.
        let mut sample = healthy_sample();
        sample[0] = sample[0].replace("\"model\":\"claude-opus-5\"", "\"model\":5");
        sample[1] = sample[1].replace("\"isSidechain\":false", "\"isSidechain\":\"no\"");
        let report = check_owned(&sample);
        assert!(!report.findings.is_empty());
        for finding in &report.findings {
            assert!(
                ASSUMPTIONS.iter().any(|a| a.id == finding.assumption),
                "id fuori tabella: {}",
                finding.assumption
            );
            assert!(!render(&report).contains("si rompono: ?"));
        }
    }
}
