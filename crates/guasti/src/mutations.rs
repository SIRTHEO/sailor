//! Le regole che ricavano i guasti dal codice, senza che nessuno li elenchi.
//!
//! Sono sei famiglie, e nessuna guarda a cosa il codice *fa*: guardano a cosa
//! il codice *dice*. Un confronto si può invertire, un limite si può spostare
//! di uno, una congiunzione si può cambiare in disgiunzione, una negazione si
//! può togliere, un carattere di confine si può togliere da una stringa, e un
//! corpo di funzione si può buttare via intero.
//!
//! L'ultima famiglia è quella che nessun elenco scritto a mano produce mai:
//! **buttare via una funzione intera** invece di rompere una riga è il modo di
//! scoprire un controllo finto, cioè una prova che resta verde anche quando il
//! codice che dichiara di provare non c'è più.
//!
//! La quinta è quella che avrebbe preso il guasto sopravvissuto del 27/08:
//! `relFile.startsWith(`${moduleDir}/`)` contro `relFile.startsWith(moduleDir)`
//! è esattamente un carattere di confine tolto da un letterale.

use crate::source::{enclosing_body, scan, Language, Scan};
use crate::Fault;
use std::collections::BTreeMap;

/// Quanti guasti al massimo da una riga sola, prima di passare alla
/// successiva: una riga fitta di operatori altrimenti si mangia il giro, e la
/// varietà vale più della densità.
pub const DEFAULT_PER_LINE_CAP: usize = 4;

/// Le coppie di operatori, dal più lungo al più corto: `===` va guardato prima
/// di `==`, o il guasto morde in mezzo a un operatore e produce testo che non
/// compila.
const OPERATORS: &[(&str, &str, &str)] = &[
    ("===", "!==", "confronto invertito"),
    ("!==", "===", "confronto invertito"),
    ("==", "!=", "confronto invertito"),
    ("!=", "==", "confronto invertito"),
    ("<=", "<", "limite stretto di uno"),
    (">=", ">", "limite stretto di uno"),
    ("&&", "||", "congiunzione allargata"),
    ("||", "&&", "disgiunzione ristretta"),
    ("??", "||", "nullish letto come falsy"),
];

/// I metodi che dicono *dove* una cosa deve stare: allargarli è il guasto che
/// una prova sul caso felice non vede mai.
const CALLS: &[(&str, &str)] = &[
    (".startsWith(", ".includes("),
    (".endsWith(", ".includes("),
    (".includes(", ".startsWith("),
    (".starts_with(", ".contains("),
    (".ends_with(", ".contains("),
    (".contains(", ".starts_with("),
    (".every(", ".some("),
    (".some(", ".every("),
    (".all(", ".any("),
    (".any(", ".all("),
];

/// I caratteri che in una stringa fanno da confine, e che tolti cambiano cosa
/// quella stringa combacia.
const BOUNDARY_CHARS: &[char] = &['/', '.', '-', ':', '\\', ' ', '_'];

/// Tutti i guasti che il codice modificato di questo file propone.
///
/// `touched` sono le righe 1-based del lato nuovo del diff. Un file di prova
/// non arriva fin qui: lo scarta chi chiama, perché guastare una prova non
/// dice niente su nessun controllo.
pub fn faults_for_file(
    path: &str,
    source: &str,
    touched: &[usize],
    per_line_cap: usize,
) -> Vec<Fault> {
    let language = Language::from_path(path);
    if !language.is_known() {
        return Vec::new();
    }
    let scanned = scan(source, language);
    let skip = test_blocks(source, &scanned, language);
    let mut found: BTreeMap<usize, Fault> = BTreeMap::new();

    for &line in touched {
        let Some(start) = scanned.line_start(line) else {
            continue;
        };
        let end = scanned
            .line_start(line + 1)
            .unwrap_or(source.len())
            .min(source.len());
        if start >= end {
            continue;
        }
        let mut on_this_line = 0usize;
        for fault in line_faults(path, source, &scanned, line, start, end) {
            if on_this_line >= per_line_cap {
                break;
            }
            if skip.iter().any(|(from, to)| fault.offset >= *from && fault.offset < *to) {
                continue;
            }
            if found.contains_key(&fault.offset) {
                continue;
            }
            on_this_line += 1;
            found.insert(fault.offset, fault);
        }
    }

    // Il corpo intero: uno per funzione toccata, non uno per riga.
    let mut bodies: BTreeMap<usize, Fault> = BTreeMap::new();
    for &line in touched {
        let Some(start) = scanned.line_start(line) else {
            continue;
        };
        if skip.iter().any(|(from, to)| start >= *from && start < *to) {
            continue;
        }
        let Some(body) = enclosing_body(source, &scanned, start) else {
            continue;
        };
        if body.end <= body.start {
            continue;
        }
        let replacement = match language {
            // `throw` e `todo!()` hanno il tipo che serve ovunque: un corpo
            // svuotato così compila sempre, quindi un verde è per forza una
            // lacuna delle prove e non un errore del compilatore.
            Language::Braces => "\n  throw new Error(\"guasto\");\n".to_string(),
            Language::Rust => "\n    todo!()\n".to_string(),
            _ => continue,
        };
        bodies.entry(body.start).or_insert_with(|| Fault {
            file: path.to_string(),
            line: body.header_line,
            offset: body.start,
            length: body.end - body.start,
            label: "corpo della funzione buttato via".to_string(),
            before: source[body.start..body.end].to_string(),
            after: replacement,
        });
    }

    let mut all: Vec<Fault> = found.into_values().chain(bodies.into_values()).collect();
    all.sort_by_key(|fault| (fault.line, fault.offset));
    all
}

/// I guasti di una riga sola.
fn line_faults(
    path: &str,
    source: &str,
    scanned: &Scan,
    line: usize,
    start: usize,
    end: usize,
) -> Vec<Fault> {
    let bytes = source.as_bytes();
    let text = &source[start..end];
    let mut out: Vec<Fault> = Vec::new();
    let make = |offset: usize, length: usize, label: &str, after: &str| Fault {
        file: path.to_string(),
        line,
        offset,
        length,
        label: label.to_string(),
        before: source[offset..offset + length].to_string(),
        after: after.to_string(),
    };

    // Le parentesi dei tipi generici, a coppie: `Map<K, V>` ha un angolo che
    // apre e uno che chiude, e riconoscere solo il primo lascia il secondo a
    // farsi guastare come se fosse un confronto.
    let type_angles = type_angle_offsets(source, scanned, start, end);

    // 1-3. Gli operatori a due e tre caratteri.
    let mut index = start;
    while index < end {
        if !scanned.is_code(index) {
            index += 1;
            continue;
        }
        let mut matched = false;
        for (from, to, label) in OPERATORS {
            // Il confronto va fatto sui byte: tagliare la stringa a
            // `index + from.len()` fa cadere il programma quando lì comincia
            // un carattere multibyte, e i commenti di questa casa sono pieni
            // di accenti e di puntini di sospensione. Gli operatori sono tutti
            // ASCII, quindi un riscontro sui byte è già un riscontro sui
            // caratteri.
            let stop = end.min(index + from.len());
            if bytes[index..stop] != *from.as_bytes() {
                continue;
            }
            if !scanned.is_code_range(index, from.len()) {
                continue;
            }
            // `==` dentro `===` non si tocca: la tabella è ordinata, ma il
            // carattere prima va guardato lo stesso.
            let before_byte = index.checked_sub(1).map(|k| bytes[k]);
            let after_byte = bytes.get(index + from.len()).copied();
            if from.len() == 2
                && (matches!(before_byte, Some(b'=') | Some(b'!') | Some(b'<') | Some(b'>'))
                    || matches!(after_byte, Some(b'=')))
            {
                continue;
            }
            out.push(make(index, from.len(), label, to));
            index += from.len();
            matched = true;
            break;
        }
        if matched {
            continue;
        }
        // 4. I confronti stretti, che dicono dove finisce un intervallo.
        if (bytes[index] == b'<' || bytes[index] == b'>')
            && !type_angles.contains(&index)
            && angle_is_comparison(bytes, index)
        {
            let to = if bytes[index] == b'<' { "<=" } else { ">=" };
            out.push(make(index, 1, "limite allargato di uno", to));
            index += 1;
            continue;
        }
        // 5. La negazione tolta.
        if bytes[index] == b'!' && negation_is_prefix(bytes, index) {
            out.push(make(index, 1, "negazione tolta", ""));
            index += 1;
            continue;
        }
        index += 1;
    }

    // 6. Le parole che decidono da sole.
    for (word, replacement) in [("true", "false"), ("false", "true")] {
        for hit in find_words(text, word) {
            let offset = start + hit;
            if scanned.is_code_range(offset, word.len()) {
                out.push(make(offset, word.len(), "booleano rovesciato", replacement));
            }
        }
    }

    // 7. I metodi che dicono dove una cosa deve stare.
    for (from, to) in CALLS {
        let mut cursor = 0usize;
        while let Some(hit) = text[cursor..].find(from) {
            let offset = start + cursor + hit;
            if scanned.is_code_range(offset, from.len()) {
                out.push(make(offset, from.len(), "guardia di posizione allargata", to));
            }
            cursor += hit + from.len();
        }
    }

    // 8. Il carattere di confine dentro un letterale: `"…/"` contro `"…"`.
    for literal in &scanned.literals {
        if literal.start < start || literal.start >= end {
            continue;
        }
        let content = &source[literal.start..literal.end];
        if content.chars().count() < 2 {
            continue;
        }
        if let Some(last) = content.chars().next_back() {
            if BOUNDARY_CHARS.contains(&last) {
                let offset = literal.end - last.len_utf8();
                out.push(make(
                    offset,
                    last.len_utf8(),
                    "confine di stringa tolto in coda",
                    "",
                ));
            }
        }
        if let Some(first) = content.chars().next() {
            if BOUNDARY_CHARS.contains(&first) {
                out.push(make(
                    literal.start,
                    first.len_utf8(),
                    "confine di stringa tolto in testa",
                    "",
                ));
            }
        }
    }

    // 9. Il limite numerico spostato, dove un confronto lo usa.
    if text.contains('<') || text.contains('>') || text.contains("==") {
        for (offset, digits) in find_integers(text) {
            let at = start + offset;
            if !scanned.is_code_range(at, digits.len()) {
                continue;
            }
            let Ok(value) = digits.parse::<i64>() else {
                continue;
            };
            out.push(make(at, digits.len(), "limite numerico spostato", &(value + 1).to_string()));
        }
    }

    out.sort_by_key(|fault| fault.offset);
    out
}

/// Gli angoli che aprono e chiudono un tipo generico dentro quella riga.
///
/// Si parte dagli angoli che aprono — un nome attaccato a sinistra, un altro
/// nome attaccato a destra, nessuno spazio in mezzo, che è come si scrive
/// `Vec<String>` e non come si scrive `a < b` — e si cammina fino a quello che
/// li pareggia, così anche la chiusura resta fuori dai guasti.
fn type_angle_offsets(
    source: &str,
    scanned: &Scan,
    start: usize,
    end: usize,
) -> std::collections::BTreeSet<usize> {
    let bytes = source.as_bytes();
    let mut out = std::collections::BTreeSet::new();
    let mut index = start;
    while index < end {
        if !scanned.is_code(index) || bytes[index] != b'<' || !opens_a_type(bytes, index) {
            index += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut cursor = index;
        while cursor < end {
            if scanned.is_code(cursor) {
                if bytes[cursor] == b'<' {
                    depth += 1;
                } else if bytes[cursor] == b'>' {
                    depth -= 1;
                    if depth == 0 {
                        out.insert(index);
                        out.insert(cursor);
                        break;
                    }
                }
            }
            cursor += 1;
        }
        index += 1;
    }
    out
}

/// Se quell'angolo apre un tipo: `Vec<String>`, `Map<K, V>`, `Array<number>`.
fn opens_a_type(bytes: &[u8], index: usize) -> bool {
    let previous = index.checked_sub(1).map(|k| bytes[k]);
    let next = bytes.get(index + 1).copied();
    let attached = previous.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    let named = next.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'\'' || byte == b'_');
    attached && named
}

/// Se quell'angolo è un confronto e non una parentesi di tipo generico.
fn angle_is_comparison(bytes: &[u8], index: usize) -> bool {
    let previous = index.checked_sub(1).map(|k| bytes[k]);
    let next = bytes.get(index + 1).copied();
    // `<=`, `>=`, `=>`, `->`, `<<`, `>>` sono già altri operatori.
    if matches!(next, Some(b'=') | Some(b'<') | Some(b'>')) {
        return false;
    }
    if matches!(previous, Some(b'=') | Some(b'-') | Some(b'<') | Some(b'>') | Some(b'!')) {
        return false;
    }
    // `Vec<String>`, `Array<number>`, `Map<K, V>`: nome attaccato all'angolo e
    // maiuscola o durata di vita subito dopo.
    let attached = previous.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    let typish = next.is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'\'');
    if attached && typish {
        return false;
    }
    // La chiusura di un generico: `>` attaccato a un nome da entrambe le parti.
    if bytes[index] == b'>'
        && previous.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && next.is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return false;
    }
    true
}

/// Se quel punto esclamativo nega qualcosa, invece di essere l'asserzione di
/// non-nullità che TypeScript scrive dopo il valore.
fn negation_is_prefix(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index + 1).copied() {
        Some(byte) => byte.is_ascii_alphabetic() || byte == b'(' || byte == b'_' || byte == b'[',
        None => false,
    }
}

/// Le occorrenze di una parola intera dentro il testo.
fn find_words(text: &str, word: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(hit) = text[cursor..].find(word) {
        let at = cursor + hit;
        let before_ok = at
            .checked_sub(1)
            .map(|k| !(bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_'))
            .unwrap_or(true);
        let after = bytes.get(at + word.len()).copied();
        let after_ok = after
            .map(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
            .unwrap_or(true);
        if before_ok && after_ok {
            out.push(at);
        }
        cursor = at + word.len();
    }
    out
}

/// Gli interi scritti nel testo, esclusi quelli attaccati a lettere o a un
/// punto decimale.
fn find_integers(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let before = start.checked_sub(1).map(|k| bytes[k]);
        let after = bytes.get(index).copied();
        let attached = before.is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || byte == b'#'
        }) || after.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'.' || byte == b'_');
        if !attached {
            out.push((start, text[start..index].to_string()));
        }
    }
    out
}

/// Gli intervalli di codice che sono prove e non codice provato: in Rust le
/// prove vivono dentro lo stesso file, sotto `#[cfg(test)]`.
fn test_blocks(source: &str, scanned: &Scan, language: Language) -> Vec<(usize, usize)> {
    if language != Language::Rust {
        return Vec::new();
    }
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(hit) = source[cursor..].find("#[cfg(test)]") {
        let at = cursor + hit;
        cursor = at + "#[cfg(test)]".len();
        if !scanned.is_code(at) {
            continue;
        }
        // Dalla graffa che apre il modulo fino a quella che la pareggia.
        let Some(open) = source[at..].find('{').map(|k| at + k) else {
            break;
        };
        let mut depth = 0usize;
        let mut index = open;
        while index < bytes.len() {
            if scanned.is_code(index) {
                if bytes[index] == b'{' {
                    depth += 1;
                } else if bytes[index] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        out.push((at, index + 1));
                        break;
                    }
                }
            }
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply;

    fn labels(path: &str, source: &str, lines: &[usize]) -> Vec<String> {
        faults_for_file(path, source, lines, 99)
            .into_iter()
            .map(|fault| format!("{}: {} -> {}", fault.label, fault.before, fault.after))
            .collect()
    }

    #[test]
    fn a_strict_comparison_is_inverted() {
        let found = labels("a.ts", "const ok = a === b;\n", &[1]);
        assert!(
            found.iter().any(|line| line.starts_with("confronto invertito: === -> !==")),
            "{found:?}"
        );
    }

    /// IL CASO DEL 27/08. `relFile.startsWith(`${moduleDir}/`)` guastato in
    /// `relFile.startsWith(`${moduleDir}`)` è il guasto che è sopravvissuto
    /// alla batteria di SocratiCode, e che nessuno aveva messo in elenco.
    #[test]
    fn the_boundary_slash_of_a_template_is_a_fault_on_its_own() {
        let source = "const covers = relFile.startsWith(`${moduleDir}/`);\n";
        let faults = faults_for_file("a.ts", source, &[1], 99);
        let boundary = faults
            .iter()
            .find(|fault| fault.label.contains("confine di stringa"))
            .expect("il confine della stringa è un guasto");
        let mutated = apply(source, boundary).expect("si applica");
        assert_eq!(mutated, "const covers = relFile.startsWith(`${moduleDir}`);\n");
    }

    #[test]
    fn a_position_guard_is_widened() {
        let found = labels("a.ts", "if (name.startsWith(prefix)) return;\n", &[1]);
        assert!(
            found
                .iter()
                .any(|line| line.contains("guardia di posizione allargata: .startsWith( -> .includes(")),
            "{found:?}"
        );
    }

    #[test]
    fn a_range_bound_moves_by_one() {
        let found = labels("a.ts", "if (depth <= max) return depth;\n", &[1]);
        assert!(found.iter().any(|line| line.contains("<= -> <")), "{found:?}");
    }

    #[test]
    fn a_conjunction_becomes_a_disjunction() {
        let found = labels("a.ts", "if (a && b) return 1;\n", &[1]);
        assert!(found.iter().any(|line| line.contains("&& -> ||")), "{found:?}");
    }

    #[test]
    fn a_negation_is_dropped() {
        let found = labels("a.ts", "if (!ready) return null;\n", &[1]);
        assert!(found.iter().any(|line| line.contains("negazione tolta")), "{found:?}");
    }

    /// Il punto esclamativo di TypeScript dopo il valore non nega niente:
    /// toglierlo cambia solo i tipi, e ogni giro sprecherebbe una batteria.
    #[test]
    fn a_non_null_assertion_is_not_a_negation() {
        let found = labels("a.ts", "const value = maybe!.field;\n", &[1]);
        assert!(!found.iter().any(|line| line.contains("negazione tolta")), "{found:?}");
    }

    #[test]
    fn a_generic_parameter_is_not_a_comparison() {
        let found = labels("a.ts", "const list: Array<string> = new Map<K, V>();\n", &[1]);
        assert!(!found.iter().any(|line| line.contains("limite allargato")), "{found:?}");
    }

    #[test]
    fn a_real_comparison_next_to_a_generic_still_counts() {
        let found = labels("a.ts", "if (items.length > limit) return;\n", &[1]);
        assert!(found.iter().any(|line| line.contains("> -> >=")), "{found:?}");
    }

    #[test]
    fn a_touched_function_loses_its_whole_body() {
        let source = "export function resolve(a: string) {\n  return a.length;\n}\n";
        let faults = faults_for_file("a.ts", source, &[2], 99);
        let body = faults
            .iter()
            .find(|fault| fault.label.contains("corpo della funzione"))
            .expect("il corpo intero è un guasto");
        let mutated = apply(source, body).expect("si applica");
        assert!(mutated.contains("throw new Error"), "{mutated}");
        assert!(!mutated.contains("return a.length"), "{mutated}");
    }

    #[test]
    fn a_rust_body_is_replaced_by_todo() {
        let source = "pub fn resolve(a: usize) -> usize {\n    a + 1\n}\n";
        let faults = faults_for_file("a.rs", source, &[2], 99);
        let body = faults
            .iter()
            .find(|fault| fault.label.contains("corpo della funzione"))
            .expect("il corpo intero è un guasto");
        assert!(apply(source, body).expect("si applica").contains("todo!()"));
    }

    /// Guastare una prova non dice niente sui controlli: dice che la prova
    /// c'era. In Rust le prove stanno nello stesso file del codice provato.
    #[test]
    fn faults_inside_a_cfg_test_block_are_left_alone() {
        let source = "pub fn f(a: usize) -> bool {\n    a == 1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        assert!(super::f(1) == true);\n    }\n}\n";
        let faults = faults_for_file("a.rs", source, &[2, 9], 99);
        assert!(faults.iter().all(|fault| fault.line <= 3), "{faults:?}");
        assert!(faults.iter().any(|fault| fault.line == 2));
    }

    /// Il primo difetto che questo programma ha trovato in se stesso: guastato
    /// il proprio sorgente, cadeva sul taglio di una riga che porta un
    /// carattere multibyte — «…» dentro un commento — invece di riferire un
    /// verdetto. Chi misura va misurato.
    #[test]
    fn a_multibyte_character_on_the_line_does_not_bring_the_program_down() {
        let source = "// il testo si taglia qui…\nif (a === b) return 1;\n";
        let faults = faults_for_file("a.ts", source, &[1, 2], 99);
        assert!(faults.iter().any(|fault| fault.label.contains("confronto invertito")));
    }

    /// La stessa forma sulla riga guastata, non su quella prima: l'operatore
    /// sta dopo il carattere lungo.
    #[test]
    fn an_operator_after_a_multibyte_character_is_still_found() {
        let source = "const nota = \"è\"; if (a === b) return 1;\n";
        let faults = faults_for_file("a.ts", source, &[1], 99);
        assert!(faults.iter().any(|fault| fault.label.contains("confronto invertito")));
        for fault in &faults {
            assert!(apply(source, fault).is_some(), "{fault:?}");
        }
    }

    #[test]
    fn a_comparison_inside_a_comment_is_never_a_fault() {
        let found = labels("a.ts", "// a === b, ma non qui\nconst c = 1;\n", &[1]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_unknown_language_yields_nothing() {
        assert!(faults_for_file("README.md", "a === b\n", &[1], 99).is_empty());
    }

    /// La densità non deve mangiare la varietà: una riga fitta cede il posto
    /// alle altre righe del diff.
    #[test]
    fn the_per_line_cap_holds() {
        let source = "if (a === b && c !== d && e <= f && g >= h && i && j) return true;\n";
        let faults = faults_for_file("a.ts", source, &[1], 3);
        assert_eq!(faults.len(), 3, "{faults:?}");
    }

    #[test]
    fn every_generated_fault_applies_to_its_source() {
        let source = "export function f(list: string[], limit: number) {\n  if (list.length >= limit && !list.includes(\"a/\")) return true;\n  return false;\n}\n";
        let faults = faults_for_file("a.ts", source, &[2, 3], 99);
        assert!(faults.len() >= 5, "{faults:?}");
        for fault in &faults {
            assert!(apply(source, fault).is_some(), "guasto che non morde: {fault:?}");
        }
    }
}
