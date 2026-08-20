//! Quando vale la pena rilanciare la cura che collega le regole nei worktree.
//!
//! Porto del giudizio di `skills/hooks/link-worktree-rules.py`. Qui non si
//! legge, non si scrive e non si lancia niente: entra l'evento del gancio con il
//! suo payload, esce un sì o un no; ed entra l'uscita dello script, esce quante
//! copie di lavoro ha collegato.
//!
//! IL COME-SI-COLLEGA NON STA QUI, e non ci sta nemmeno nell'originale: lo fa
//! `scripts/link-worktree-rules.sh`, che resta invocabile a mano ed è
//! documentato. Il gancio — e quindi questo modulo — decide solo il **quando**.
//!
//! DUE MOMENTI. `SessionStart` copre la copia nata mentre non c'eravamo;
//! `PostToolUse` su un comando che crea un worktree copre quella che nasce
//! adesso, che altrimenti resterebbe scoperta per tutta la sessione in cui ci si
//! lavora.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// I gesti che creano una copia di lavoro: `orca worktree create` è la via
/// prescritta, `git worktree add` quella che si usa lo stesso.
///
/// L'espressione è quella dell'originale, carattere per carattere. Il `\b`
/// finale è ciò che distingue `worktree add` da `worktree added`, e il gruppo
/// opzionale `-C <path>` è l'unica forma di `git` con l'opzione in mezzo che si
/// vede nei comandi veri.
fn re_creates_worktree() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:orca\s+worktree\s+create|git\s+(?:-C\s+\S+\s+)?worktree\s+add)\b")
            .unwrap()
    })
}

pub fn creates_worktree(command: &str) -> bool {
    re_creates_worktree().is_match(command)
}

/// Vero se questo evento merita una passata di collegamento.
///
/// I RAMI IN CUI L'ORIGINALE MUORE VALGONO FALSO, e non è una scorciatoia: in
/// Python `(payload.get('tool_input') or {}).get('command')` su un `tool_input`
/// vero ma non-dizionario è un `AttributeError`, e `RE.search(42)` su un comando
/// numerico è un `TypeError`. Nessuno dei due è catturato dentro `wants_run`:
/// risalgono al `try` che avvolge `main()`, che esce 0 senza stampare e senza
/// lanciare lo script. Cioè esattamente ciò che succede quando la risposta è no.
/// Riprodurli come «falso» è equivalente osservabile, non un'approssimazione.
pub fn wants_run(event: &str, payload: &Value) -> bool {
    if event == "SessionStart" {
        return true;
    }
    if event != "PostToolUse" {
        return false;
    }
    if payload.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return false;
    }
    // `payload.get('tool_input') or {}`: i valori falsi di Python (assente,
    // `null`, `{}`, `[]`, `""`, `0`, `false`) diventano il dizionario vuoto, e
    // da lì il comando è assente. Tutto il resto che non sia un oggetto è la
    // strada dell'eccezione, cioè falso.
    let Some(args) = payload.get("tool_input") else {
        return false;
    };
    if !args.is_object() {
        return false;
    }
    // `... .get('command') or ''`: solo una stringa non vuota arriva
    // all'espressione regolare.
    match args.get("command").and_then(Value::as_str) {
        Some(command) => creates_worktree(command),
        None => false,
    }
}

/// Quante righe dell'uscita dello script dicono di aver fatto qualcosa.
///
/// Lo script stampa una riga per copia di lavoro — `ok`, `FOUND`, `linked` o
/// `synced` — e solo le ultime due sono un lavoro fatto. Il gancio parla
/// soltanto se questo numero è maggiore di zero: una passata che non trova
/// niente da fare, che è il caso normale, deve restare muta.
///
/// LE FORME DA CONTARE SONO DUE DAL 21/08/2026, e la seconda è quella che porta
/// le regole: `linked` per le voci collegate, `synced` per le regole ricopiate.
/// Contare solo la prima farebbe dire «0 copie di lavoro» proprio nella passata
/// che ne ha riparate dodici — un controllo che mente sul proprio lavoro. La
/// forma italiana `collegato` non si conta più perché lo script non la stampa
/// più: se ricomparisse sarebbe un binario vecchio, e vale zero come prima.
pub fn linked_count(stdout: &str) -> usize {
    splitlines(stdout)
        .into_iter()
        .filter(|r| {
            let r = strip(r);
            r.starts_with("linked") || r.starts_with("synced")
        })
        .count()
}

/// `str.splitlines()` di Python, che non è `lines()` di Rust.
///
/// Python divide anche su tabulazione verticale, avanzamento pagina, i
/// separatori di file/gruppo/record (`\x1c`-`\x1e`) e su `\u{85}`, `\u{2028}`,
/// `\u{2029}`. Su un'uscita normale non cambia niente; su una riga che contenga
/// uno di quei caratteri il conteggio delle copie collegate cambierebbe — ed è
/// un numero che il gancio stampa.
///
/// `\r` e `\r\n` non arrivano qui passando da `subprocess`, che in modo testo li
/// ha già tradotti in `\n`; restano gestiti perché il modulo è puro e va bene
/// anche a chi gli passa un testo grezzo.
fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        let salto = match c {
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => c.len_utf8(),
            '\r' => {
                if s[i + 1..].starts_with('\n') {
                    it.next();
                    2
                } else {
                    1
                }
            }
            _ => continue,
        };
        out.push(&s[start..i]);
        start = i + salto;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// `str.strip()` di Python, che toglie **quattro caratteri in più** di `trim()`.
///
/// `is_whitespace` di Rust è la proprietà Unicode `White_Space`; `str.isspace()`
/// di Python le aggiunge i separatori `\x1c`-`\x1f`. La differenza si vede solo
/// su una riga che cominci con uno di quelli: lì il Python vedrebbe `collegato`
/// e il porto no.
fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// L'avviso, parola per parola come l'originale. Una riga sola, e cita il modo
/// di guardare cosa è successo invece di elencarlo.
pub fn notice(count: usize, script: &str) -> String {
    format!(
        "Linked repo config into {count} worktree(s) that had none. \
         Inspect with: bash {script} --conta"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_start_always_runs() {
        assert!(wants_run("SessionStart", &json!({"source": "startup"})));
        assert!(wants_run("SessionStart", &json!({})));
    }

    #[test]
    fn only_worktree_creation_runs_on_post_tool_use() {
        let bash = |c: &str| json!({"tool_name": "Bash", "tool_input": {"command": c}});
        assert!(wants_run("PostToolUse", &bash("orca worktree create --name y")));
        assert!(wants_run(
            "PostToolUse",
            &bash("git -C /home/someone/other-repo/work/suite worktree add ../x")
        ));
        assert!(!wants_run("PostToolUse", &bash("npm test")));
        assert!(!wants_run("PostToolUse", &bash("orca worktree list")));
        assert!(!wants_run("PostToolUse", &bash("git worktree remove ../x")));
        // Il `\b` finale: «added» non è «add».
        assert!(!wants_run("PostToolUse", &bash("git worktree added")));
        assert!(!wants_run("PostToolUse", &bash("xorca worktree create")));
        // Maiuscole: l'originale non ha `re.I`.
        assert!(!wants_run("PostToolUse", &bash("ORCA WORKTREE CREATE")));
        // `-C` senza il suo percorso non è la forma prevista.
        assert!(!wants_run("PostToolUse", &bash("git -C worktree add")));
    }

    #[test]
    fn another_tool_and_unknown_events_never_run() {
        assert!(!wants_run(
            "PostToolUse",
            &json!({"tool_name": "Write", "tool_input": {}})
        ));
        assert!(!wants_run("Stop", &json!({})));
        assert!(!wants_run(
            "PreToolUse",
            &json!({"tool_name": "Bash", "tool_input": {"command": "orca worktree create"}})
        ));
    }

    /// Gli ingressi su cui l'originale solleva: qui valgono no, e il gancio
    /// deve restare fermo esattamente come là.
    #[test]
    fn malformed_tool_input_never_runs() {
        for storto in [json!("orca worktree create"), json!([1, 2]), json!(42)] {
            assert!(!wants_run(
                "PostToolUse",
                &json!({"tool_name": "Bash", "tool_input": storto})
            ));
        }
        // `command` non testuale: `re.search` su un numero è un TypeError.
        assert!(!wants_run(
            "PostToolUse",
            &json!({"tool_name": "Bash", "tool_input": {"command": 42}})
        ));
        // I valori falsi diventano il dizionario vuoto: nessun comando.
        for vuoto in [json!(null), json!(""), json!(0), json!(false), json!([])] {
            assert!(!wants_run(
                "PostToolUse",
                &json!({"tool_name": "Bash", "tool_input": vuoto})
            ));
        }
    }

    #[test]
    fn only_lines_that_did_something_are_counted() {
        let uscita = "  ok        other-repo/work/suite/x\n  \
                      linked    other-repo/work/suite/y : CLAUDE.md\n  \
                      FOUND     other-repo/work/suite/z : CLAUDE.md\n  \
                      synced    orca/workspaces/a-client/w : .claude/rules (21 rules)\n";
        assert_eq!(linked_count(uscita), 2);
        assert_eq!(linked_count(""), 0);
        assert_eq!(linked_count("  ok  a\n  ok  b\n"), 0);
        // MUTANTE: contando solo `linked`, questo caso scende a 1. È la passata
        // che ripara le regole, cioè quella che non deve mai risultare vuota.
        assert_eq!(linked_count("  synced    a : .claude/rules (20 rules)\n"), 1);
        // Il confronto è su un prefisso, non su una parola intera: è
        // `startswith` anche nell'originale.
        assert_eq!(linked_count("linkedXYZ\n"), 1);
        // Ma il prefisso deve stare in testa alla riga ripulita.
        assert_eq!(linked_count("  ho linked x\n"), 0);
        // La forma italiana non si stampa più: se ricompare è un binario
        // vecchio, e vale zero.
        assert_eq!(linked_count("collegato a\n"), 0);
    }

    /// I due dettagli in cui Python non è Rust, e che cambiano un numero
    /// stampato: dove si spezza una riga e cosa si toglie dai suoi bordi.
    #[test]
    fn python_line_splitting_and_stripping_are_reproduced() {
        // `\x0b` spezza in Python, `lines()` di Rust no: sono due righe.
        assert_eq!(linked_count("linked a\u{b}synced b"), 2);
        assert_eq!(linked_count("linked a\u{2028}synced b"), 2);
        // `\x1f` è spazio per `str.strip()` e non per `trim()`, ed è l'unico
        // dei quattro separatori che serva a provarlo: `\x1c`, `\x1d` e `\x1e`
        // sono anche fine riga, quindi `splitlines` se li mangia prima che
        // qualcuno ripulisca i bordi. Con `\x1c` al loro posto, il mutante che
        // mette `trim()` qui sopravvive — misurato al primo giro di mutanti.
        assert_eq!(linked_count("\u{1f}linked a\n"), 1);
        assert_eq!(linked_count("\u{1c}linked a\n"), 1);
        assert_eq!(linked_count("\u{a0}linked a\n"), 1);
        // `\r\n` vale un salto solo, non due righe vuote in mezzo.
        assert_eq!(linked_count("linked a\r\nsynced b\r\n"), 2);
    }

    #[test]
    fn the_notice_is_one_line_and_names_the_script() {
        assert_eq!(
            notice(3, "/x/.claude/scripts/link-worktree-rules.sh"),
            "Linked repo config into 3 worktree(s) that had none. \
             Inspect with: bash /x/.claude/scripts/link-worktree-rules.sh --conta"
        );
    }
}
