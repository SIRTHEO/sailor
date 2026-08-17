//! Nega i rimandi a documenti locali dentro i commenti del codice.
//!
//! Decisione di Theo, 17/08/2026: «il codice parla più di ogni cosa, i commenti
//! non devono dichiarare percorsi di documenti locali».
//!
//! La misura che l'ha motivata, sui tre repo (2.271 file sorgente): 996 righe
//! di commento contengono un rimando, in 526 file, e **66 sono morti**. Il caso
//! peggiore è un documento di architettura citato 39 volte in un solo repo, con
//! rimandi ai suoi paragrafi, che non è mai stato scritto — non cancellato, mai
//! creato. Altri dieci puntano dentro un albero ignorato da git: su una copia
//! pulita sono morti al 100%, anche quando esistono sul disco di chi li scrisse.
//!
//! Porta `~/.claude/skills/hooks/comment-refs.py`, che resta l'oracolo finché
//! `tests/equivalence.rs` non dice che le due decisioni coincidono.
//!
//! Cosa NON guarda, e sono i tre falsi allarmi misurati quel giorno: i numeri di
//! segnalazione (`#123` — la maggior parte sono numeri di fase, e il riscontro
//! spezzerebbe i colori esadecimali: `#3d5ccc` diventa `#3`); il README di un
//! pacchetto esterno quando porta un indirizzo accanto; i percorsi di **codice**,
//! compresi quelli verso un altro repo, che dicono dove vive il contratto
//! rispecchiato e sono la parte utile del commento.

use hook_io::Decision;
use regex::Regex;
use std::sync::OnceLock;

const EXCLUDED: [&str; 7] = [
    "node_modules", "/dist/", "/build/", "/out/", "/.next/", "/generated/", "/coverage/",
];

const CODE_EXTENSIONS: [&str; 11] = [
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "sh", "bash", "sql", "prisma",
];

/// Il file dichiara di essere il banco di prova di questo freno, e quindi cita
/// per forza ciò che vieta.
const MARKER: &str = "comment-refs: banco di prova";

struct Pattern {
    label: &'static str,
    re: Regex,
}

fn patterns() -> &'static Vec<Pattern> {
    static P: OnceLock<Vec<Pattern>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            // La forma a data va provata **per prima**: `ADR[\s-]?\d{3,}`
            // riconoscerebbe «ADR 2026» dentro «ADR 2026-07-01» e l'etichetta
            // sarebbe quella sbagliata. L'esito non cambia, il messaggio sì.
            Pattern {
                label: "un documento di decisione (per data)",
                re: Regex::new(r"(?i)\bADR[\s\-]?\d{4}-\d{2}-\d{2}\b").unwrap(),
            },
            // Tre cifre o più: `ADR 12` non esiste, e due prenderebbero le sigle.
            Pattern {
                label: "un documento di decisione",
                re: Regex::new(r"(?i)\bADR[\s\-]?\d{3,}\b").unwrap(),
            },
            Pattern {
                label: "la cartella dei documenti di decisione",
                re: Regex::new(r"(?i)(docs/)?adr/").unwrap(),
            },
            Pattern {
                label: "un piano o un rapporto",
                re: Regex::new(r"(?i)\bPlans\.md\b|\bdocs/(plans|reports)/").unwrap(),
            },
            Pattern {
                label: "un albero ignorato da git",
                re: Regex::new(r"\.claude/").unwrap(),
            },
            Pattern {
                label: "un documento di contesto",
                re: Regex::new(r"\b(CONTEXT|CLAUDE|AGENTS)\.md\b").unwrap(),
            },
        ]
    })
}

fn re_readme() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\bREADME\b").unwrap())
}

fn re_url() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"https?://").unwrap())
}

/// Finché `scripts/review-residuals.sh` pretende un puntatore accanto a ogni
/// `TODO`/`FIXME`, quelle righe non possono obbedire a entrambe le regole.
fn re_todo() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(TODO|FIXME|HACK|XXX)\b").unwrap())
}

pub fn is_code(path: &str) -> bool {
    if EXCLUDED.iter().any(|p| path.contains(p)) {
        return false;
    }
    match path.rsplit_once('.') {
        Some((_, ext)) => CODE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

fn hash_style(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()),
        Some(ref e) if e == "py" || e == "sh" || e == "bash"
    )
}

/// Le sole porzioni di commento, con il numero di riga.
///
/// Le stringhe non entrano: un `//` dentro un indirizzo web non apre un
/// commento, ed è il primo falso allarme da evitare. Una stringa non chiusa
/// chiude la riga — non apre un commento a metà.
pub fn comment_spans(text: &str, path: &str) -> Vec<(usize, String)> {
    let hash = hash_style(path);
    let mut out = Vec::new();
    let mut in_block = false;

    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        let mut rest: &str = line;
        loop {
            if rest.is_empty() {
                break;
            }
            if in_block {
                match rest.find("*/") {
                    None => {
                        out.push((lineno, rest.to_string()));
                        break;
                    }
                    Some(end) => {
                        out.push((lineno, rest[..end].to_string()));
                        rest = &rest[end + 2..];
                        in_block = false;
                        continue;
                    }
                }
            }
            let mut best: Option<(usize, &str)> = None;
            let mut consider = |pos: Option<usize>, token: &'static str| {
                if let Some(p) = pos {
                    if best.map_or(true, |(bp, _)| p < bp) {
                        best = Some((p, token));
                    }
                }
            };
            consider(rest.find("/*"), "/*");
            consider(rest.find("//"), "//");
            if hash {
                consider(rest.find('#'), "#");
            }
            consider(rest.find('"'), "\"");
            consider(rest.find('\''), "'");
            consider(rest.find('`'), "`");

            let Some((pos, token)) = best else { break };
            match token {
                "\"" | "'" | "`" => {
                    let after = &rest[pos + token.len()..];
                    match after.find(token) {
                        None => break,
                        Some(end) => {
                            rest = &after[end + token.len()..];
                            continue;
                        }
                    }
                }
                "/*" => {
                    in_block = true;
                    rest = &rest[pos + 2..];
                    continue;
                }
                _ => {
                    out.push((lineno, rest[pos..].to_string()));
                    break;
                }
            }
        }
    }
    out
}

/// I rimandi vietati: (riga, cosa, frammento).
pub fn findings(text: &str, path: &str) -> Vec<(usize, &'static str, String)> {
    let mut found = Vec::new();
    for (lineno, comment) in comment_spans(text, path) {
        if re_todo().is_match(&comment) {
            continue;
        }
        let mut hit = false;
        for p in patterns() {
            if let Some(m) = p.re.find(&comment) {
                found.push((lineno, p.label, m.as_str().to_string()));
                hit = true;
                break;
            }
        }
        if !hit && re_readme().is_match(&comment) && !re_url().is_match(&comment) {
            found.push((lineno, "un documento di contesto", "README".to_string()));
        }
    }
    found
}

fn report(found: &[(usize, &'static str, String)]) -> String {
    // I messaggi dei gate restano in inglese: finiscono nei registri.
    let mut s = String::from(
        "Comment references a local document. The code must carry the reason itself \
         — a pointer goes stale and then misleads (66 dead references measured \
         across the three repos on 2026-08-17, 39 of them to a document that was \
         never written).",
    );
    for (n, what, frag) in found.iter().take(6) {
        s.push_str(&format!("\n  line {n}: {what} — \"{frag}\""));
    }
    s.push_str(
        "\nWrite the constraint, not its address. History belongs in the commit \
         message and in Linear. Valve: COMMENT_REFS=off.",
    );
    s
}

/// Dal percorso e dal testo appena scritto alla decisione. Pura: nessun accesso
/// a stdin o all'ambiente, così il confronto col Python prova la decisione e non
/// il processo. `exempt` lo passa il chiamante, che è l'unico a poter leggere il
/// file già su disco.
pub fn judge(path: &str, text: &str, exempt: bool) -> Decision {
    if !is_code(path) || exempt {
        return Decision::Pass;
    }
    let found = findings(text, path);
    if found.is_empty() {
        Decision::Pass
    } else {
        Decision::Deny(report(&found))
    }
}

/// Il file dichiara di essere il banco di prova di questo freno.
pub fn declares_marker(contents: &str) -> bool {
    contents.contains(MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "/x/src/a.ts";
    const PY: &str = "/x/scripts/a.py";

    fn denies(text: &str, path: &str) -> bool {
        matches!(judge(path, text, false), Decision::Deny(_))
    }

    #[test]
    fn it_denies_a_decision_document_reference() {
        assert!(denies("// ADR 0008 #61: il downgrade è eliminato", TS));
    }

    #[test]
    fn it_stays_silent_without_the_reference() {
        assert!(!denies("// il downgrade categoriale è eliminato", TS));
    }

    #[test]
    fn it_denies_the_dated_form_a_numeric_pattern_would_miss() {
        assert!(denies("// Account-linking scoped a Google (ADR 2026-07-01)", TS));
    }

    #[test]
    fn it_labels_the_dated_form_as_such() {
        let f = findings("// (ADR 2026-07-01)", TS);
        assert_eq!(f[0].1, "un documento di decisione (per data)");
    }

    #[test]
    fn a_bare_date_is_not_a_reference() {
        assert!(!denies("// deciso il 2026-07-01, e da allora vale", TS));
    }

    #[test]
    fn it_denies_plans_and_reports() {
        assert!(denies("// Deferral accettato (vedi Plans.md FLD-8)", TS));
        assert!(denies("/* fonte: docs/reports/2026-05-08-audit.md */", TS));
    }

    #[test]
    fn it_denies_a_git_ignored_tree() {
        assert!(denies("// Spec: .claude/docs/specs/2026-06-10-x.md", TS));
    }

    #[test]
    fn a_phase_number_is_not_a_reference() {
        assert!(!denies("// Phase 3 #14: cache lookup unificato", TS));
    }

    #[test]
    fn a_hex_colour_is_not_a_reference() {
        assert!(!denies("// il tracciato usa #3d5ccc come tinta", TS));
    }

    #[test]
    fn an_external_readme_with_a_url_survives() {
        assert!(!denies("// codici da https://github.com/aircall/x (README API)", TS));
    }

    #[test]
    fn our_own_readme_without_a_url_does_not() {
        assert!(denies("// il resto è spiegato nel README", TS));
    }

    #[test]
    fn a_code_path_survives_even_across_repos() {
        assert!(!denies(
            "// SSOT: a-service src/utils/schemas/api/responses.ts",
            TS
        ));
    }

    #[test]
    fn a_todo_line_is_exempt_while_the_other_gate_demands_the_pointer() {
        assert!(!denies("// TODO (Plans.md DEL-2): manca il ritiro", TS));
    }

    #[test]
    fn the_same_line_without_todo_is_not_exempt() {
        assert!(denies("// manca il ritiro (Plans.md DEL-2)", TS));
    }

    #[test]
    fn a_path_inside_a_string_is_not_a_comment() {
        assert!(!denies("const dir = \"docs/adr/\";", TS));
    }

    #[test]
    fn the_same_text_in_a_comment_is() {
        assert!(denies("// la cartella è docs/adr/", TS));
    }

    #[test]
    fn a_url_does_not_open_a_comment() {
        assert!(!denies("const u = \"https://x.dev/CONTEXT.md\";", TS));
    }

    #[test]
    fn hash_is_a_comment_only_where_that_is_the_right_form() {
        assert!(denies("# vedi ADR 0008 per il resto", PY));
        assert!(!denies("# vedi ADR 0008 per il resto", TS));
    }

    #[test]
    fn it_reads_a_multi_line_block() {
        assert!(denies("/*\n * Formule canoniche (ADR 0008 §2):\n */", TS));
    }

    #[test]
    fn generated_files_are_out() {
        assert!(!is_code("/x/src/generated/a.ts"));
    }

    #[test]
    fn a_document_is_not_code() {
        assert!(!is_code("/x/docs/nota.md"));
    }

    #[test]
    fn an_exempt_file_passes() {
        assert!(matches!(
            judge(TS, "// ADR 0008 ovunque", true),
            Decision::Pass
        ));
    }
}
