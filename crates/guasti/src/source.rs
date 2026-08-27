//! Dove finisce il codice e comincia il testo.
//!
//! Guastare un operatore dentro una stringa o dentro un commento produce un
//! guasto che nessuna prova può uccidere — e nel rapporto quel verde si
//! leggerebbe come una lacuna della batteria. Da qui esce la maschera che dice,
//! byte per byte, se quel punto è codice vero; e l'elenco dei letterali, che
//! serve alla regola sul carattere di confine (`"…/"` contro `"…"`), l'unica
//! che avrebbe preso il guasto sopravvissuto trovato dal manutentore.

/// Le famiglie di sintassi che cambiano il modo di leggere stringhe e commenti.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// TypeScript, JavaScript e parenti: `//`, `/* */`, `"`, `'`, backtick con
    /// interpolazione.
    Braces,
    /// Rust: come sopra ma senza backtick, con le stringhe grezze, e con
    /// l'apostrofo che quasi sempre è una durata di vita e non un carattere.
    Rust,
    /// Python, shell, TOML, YAML: commento con `#`.
    Hash,
    /// Nessuna regola nota: tutto conta come codice.
    Plain,
}

impl Language {
    /// La famiglia dall'estensione del file.
    pub fn from_path(path: &str) -> Language {
        let extension = path.rsplit('.').next().unwrap_or("");
        match extension {
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" | "java" | "go" | "c"
            | "h" | "cc" | "cpp" | "hpp" | "cs" | "swift" | "kt" | "php" | "scala" => {
                Language::Braces
            }
            "rs" => Language::Rust,
            "py" | "sh" | "bash" | "zsh" | "toml" | "yml" | "yaml" | "rb" | "pl" => Language::Hash,
            _ => Language::Plain,
        }
    }

    /// Se `guasti` sa generare guasti utili per questa famiglia.
    pub fn is_known(self) -> bool {
        self != Language::Plain
    }
}

/// Un letterale di testo: gli estremi del **contenuto**, virgolette escluse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    pub start: usize,
    pub end: usize,
}

/// La lettura di un file: cosa è codice, dove stanno i letterali, dove
/// cominciano le righe.
#[derive(Debug, Clone)]
pub struct Scan {
    code: Vec<bool>,
    pub literals: Vec<Literal>,
    line_starts: Vec<usize>,
}

impl Scan {
    /// Se quel byte è codice vero e non testo o commento.
    pub fn is_code(&self, offset: usize) -> bool {
        self.code.get(offset).copied().unwrap_or(false)
    }

    /// Se l'intervallo è tutto codice vero.
    pub fn is_code_range(&self, offset: usize, length: usize) -> bool {
        (offset..offset + length).all(|byte| self.is_code(byte))
    }

    /// La riga 1-based in cui cade quel byte.
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    /// Il byte d'inizio della riga 1-based.
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line.checked_sub(1)?).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    /// Codice dentro un `${…}` di un template: si esce alla graffa che pareggia.
    Interpolation(usize),
    LineComment,
    BlockComment,
    /// Stringa con quel delimitatore.
    Quoted(u8),
    /// Stringa grezza di Rust: `r#"…"#`, con quanti cancelletti la chiudono.
    RawString(usize),
    /// Le tre virgolette di Python.
    TripleQuoted(u8),
}

/// Legge il file una volta sola e risponde a tutte e tre le domande.
pub fn scan(source: &str, language: Language) -> Scan {
    let bytes = source.as_bytes();
    let mut code = vec![false; bytes.len()];
    let mut literals = Vec::new();
    let mut line_starts = vec![0usize];
    let mut stack: Vec<Mode> = vec![Mode::Code];
    let mut literal_start: Option<usize> = None;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\n' {
            line_starts.push(index + 1);
        }
        let mode = *stack.last().unwrap_or(&Mode::Code);
        match mode {
            Mode::Code | Mode::Interpolation(_) => {
                // Le graffe contano solo per sapere quando finisce un `${…}`.
                if let Mode::Interpolation(depth) = mode {
                    if byte == b'{' {
                        *stack.last_mut().unwrap() = Mode::Interpolation(depth + 1);
                    } else if byte == b'}' {
                        if depth == 0 {
                            stack.pop();
                            index += 1;
                            continue;
                        }
                        *stack.last_mut().unwrap() = Mode::Interpolation(depth - 1);
                    }
                }
                let two = bytes.get(index + 1).copied();
                if matches!(language, Language::Braces | Language::Rust)
                    && byte == b'/'
                    && two == Some(b'/')
                {
                    stack.push(Mode::LineComment);
                    index += 2;
                    continue;
                }
                if matches!(language, Language::Braces | Language::Rust)
                    && byte == b'/'
                    && two == Some(b'*')
                {
                    stack.push(Mode::BlockComment);
                    index += 2;
                    continue;
                }
                if language == Language::Hash && byte == b'#' {
                    stack.push(Mode::LineComment);
                    index += 1;
                    continue;
                }
                // `r"…"` e `r#"…"#`: in Rust il cancelletto non apre un
                // commento, e dentro una stringa grezza le barre rovesciate
                // non scappano niente.
                if language == Language::Rust && byte == b'r' {
                    let mut hashes = 0usize;
                    while bytes.get(index + 1 + hashes) == Some(&b'#') {
                        hashes += 1;
                    }
                    if bytes.get(index + 1 + hashes) == Some(&b'"') {
                        stack.push(Mode::RawString(hashes));
                        literal_start = Some(index + 2 + hashes);
                        index += 2 + hashes;
                        continue;
                    }
                }
                if language == Language::Hash && (byte == b'"' || byte == b'\'') {
                    let triple = bytes.get(index + 1) == Some(&byte)
                        && bytes.get(index + 2) == Some(&byte);
                    if triple {
                        stack.push(Mode::TripleQuoted(byte));
                        literal_start = Some(index + 3);
                        index += 3;
                        continue;
                    }
                }
                let opens_quote = match language {
                    Language::Braces => byte == b'"' || byte == b'\'' || byte == b'`',
                    // In Rust l'apostrofo è quasi sempre una durata di vita:
                    // `&'a str`. Conta come carattere solo se si chiude entro
                    // il posto che un carattere occupa.
                    Language::Rust => {
                        byte == b'"' || (byte == b'\'' && rust_char_literal(bytes, index))
                    }
                    Language::Hash => byte == b'"' || byte == b'\'',
                    Language::Plain => false,
                };
                if opens_quote {
                    stack.push(Mode::Quoted(byte));
                    literal_start = Some(index + 1);
                    index += 1;
                    continue;
                }
                code[index] = true;
                index += 1;
            }
            Mode::LineComment => {
                if byte == b'\n' {
                    stack.pop();
                }
                index += 1;
            }
            Mode::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    stack.pop();
                    index += 2;
                    continue;
                }
                index += 1;
            }
            Mode::Quoted(delimiter) => {
                if byte == b'\\' {
                    index += 2;
                    continue;
                }
                if delimiter == b'`' && byte == b'$' && bytes.get(index + 1) == Some(&b'{') {
                    stack.push(Mode::Interpolation(0));
                    index += 2;
                    continue;
                }
                if byte == delimiter {
                    if let Some(start) = literal_start.take() {
                        literals.push(Literal { start, end: index });
                    }
                    stack.pop();
                }
                index += 1;
            }
            Mode::RawString(hashes) => {
                if byte == b'"' {
                    let closed = (0..hashes).all(|k| bytes.get(index + 1 + k) == Some(&b'#'));
                    if closed {
                        if let Some(start) = literal_start.take() {
                            literals.push(Literal { start, end: index });
                        }
                        stack.pop();
                        index += 1 + hashes;
                        continue;
                    }
                }
                index += 1;
            }
            Mode::TripleQuoted(delimiter) => {
                if byte == delimiter
                    && bytes.get(index + 1) == Some(&delimiter)
                    && bytes.get(index + 2) == Some(&delimiter)
                {
                    if let Some(start) = literal_start.take() {
                        literals.push(Literal { start, end: index });
                    }
                    stack.pop();
                    index += 3;
                    continue;
                }
                index += 1;
            }
        }
    }

    Scan {
        code,
        literals,
        line_starts,
    }
}

/// Se l'apostrofo a quell'indice apre un carattere e non una durata di vita.
fn rust_char_literal(bytes: &[u8], index: usize) -> bool {
    // `'a'`, `'\n'`, `'\u{1F600}'`: si chiude entro pochi byte. `'a` di
    // `&'a str` no, e nemmeno `'static`.
    let limit = (index + 12).min(bytes.len());
    let escaped = bytes.get(index + 1) == Some(&b'\\');
    let mut cursor = index + if escaped { 2 } else { 1 };
    while cursor < limit {
        if bytes[cursor] == b'\'' {
            return escaped || cursor <= index + 4;
        }
        if !escaped && cursor > index + 4 {
            return false;
        }
        cursor += 1;
    }
    false
}

/// Un corpo di funzione: gli estremi del testo fra le graffe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// Riga 1-based dell'intestazione.
    pub header_line: usize,
    /// Byte dopo la graffa aperta.
    pub start: usize,
    /// Byte della graffa chiusa.
    pub end: usize,
}

/// Il corpo di funzione più interno che contiene quel byte.
///
/// Serve al guasto che **butta via una funzione intera** invece di rompere una
/// riga: è il modo del manutentore per scoprire un controllo finto, e nessun
/// cambio di operatore ci arriva. Le famiglie senza graffe non ce l'hanno.
pub fn enclosing_body(source: &str, scan: &Scan, offset: usize) -> Option<Body> {
    let bytes = source.as_bytes();
    let mut open_stack: Vec<usize> = Vec::new();
    let mut best: Option<Body> = None;
    for index in 0..bytes.len() {
        if !scan.is_code(index) {
            continue;
        }
        match bytes[index] {
            b'{' => open_stack.push(index),
            b'}' => {
                if let Some(open) = open_stack.pop() {
                    if open < offset && offset <= index && looks_like_function(source, scan, open) {
                        let candidate = Body {
                            header_line: scan.line_of(open),
                            start: open + 1,
                            end: index,
                        };
                        // Il più interno vince: `best` tiene quello che
                        // comincia più tardi.
                        best = match best {
                            Some(previous) if previous.start > candidate.start => Some(previous),
                            _ => Some(candidate),
                        };
                    }
                }
            }
            _ => {}
        }
    }
    best
}

/// Se la graffa aperta a quell'indice chiude un'intestazione di funzione.
///
/// Basta il testo della riga: una lista di argomenti chiusa prima della
/// graffa. Prende anche i metodi e le funzioni a freccia, e lascia fuori
/// oggetti letterali, blocchi `if` e `match`.
fn looks_like_function(source: &str, scan: &Scan, brace: usize) -> bool {
    let line_start = scan
        .line_starts
        .binary_search(&brace)
        .unwrap_or_else(|index| index.saturating_sub(1));
    let start = scan.line_starts.get(line_start).copied().unwrap_or(0);
    let header = &source[start..brace];
    let trimmed = header.trim_end();
    if !trimmed.ends_with(')') && !trimmed.ends_with("=>") && !trimmed.contains("=>") {
        // Un tipo di ritorno scritto dopo la parentesi: `): string {`.
        if !(trimmed.contains(')') && trimmed.contains(':')) {
            return false;
        }
    }
    let opening = trimmed.contains('(');
    let control = ["if", "for", "while", "switch", "catch", "match", "unsafe"]
        .iter()
        .any(|word| {
            trimmed.trim_start().starts_with(word)
                && trimmed
                    .trim_start()
                    .get(word.len()..word.len() + 1)
                    .is_none_or(|next| next == " " || next == "(")
        });
    opening && !control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comparison_inside_a_string_is_not_code() {
        let source = "const a = \"x === y\"; if (b === c) {}";
        let scanned = scan(source, Language::Braces);
        let inside = source.find("x ===").unwrap() + 2;
        let outside = source.rfind("===").unwrap();
        assert!(!scanned.is_code(inside), "dentro la stringa non è codice");
        assert!(scanned.is_code(outside), "fuori dalla stringa è codice");
    }

    #[test]
    fn a_comparison_inside_a_comment_is_not_code() {
        let source = "// a === b\nconst c = 1;";
        let scanned = scan(source, Language::Braces);
        assert!(!scanned.is_code(source.find("===").unwrap()));
        assert!(scanned.is_code(source.find("const").unwrap()));
    }

    /// Il blocco di commento attraversa le righe: senza stato sul file intero,
    /// la seconda riga tornerebbe a contare come codice.
    #[test]
    fn a_block_comment_keeps_its_state_across_lines() {
        let source = "/* a === b\n   c === d */\nconst e = 1;";
        let scanned = scan(source, Language::Braces);
        assert!(!scanned.is_code(source.find("c ===").unwrap() + 2));
        assert!(scanned.is_code(source.find("const").unwrap()));
    }

    #[test]
    fn an_interpolation_holds_real_code() {
        let source = "const a = `${x === y}/`;";
        let scanned = scan(source, Language::Braces);
        assert!(scanned.is_code(source.find("===").unwrap()));
    }

    /// Il caso che conta: il contenuto del template finisce con la barra, ed è
    /// quel carattere che il guasto toglie.
    #[test]
    fn a_template_literal_reports_its_content() {
        let source = "relFile.startsWith(`${moduleDir}/`)";
        let scanned = scan(source, Language::Braces);
        let literal = scanned.literals.first().expect("un letterale");
        assert_eq!(&source[literal.start..literal.end], "${moduleDir}/");
    }

    #[test]
    fn a_rust_lifetime_is_not_a_string() {
        let source = "fn take<'a>(value: &'a str) -> bool { value == \"x\" }";
        let scanned = scan(source, Language::Rust);
        assert!(scanned.is_code(source.find("==").unwrap()), "il confronto resta codice");
        let literal = scanned.literals.first().expect("una stringa sola");
        assert_eq!(&source[literal.start..literal.end], "x");
    }

    #[test]
    fn a_rust_char_literal_is_a_string() {
        let source = "let quote = '\\'';\nlet other = 'a';";
        let scanned = scan(source, Language::Rust);
        assert!(scanned.is_code(source.find("let other").unwrap()));
    }

    /// Il cancelletto di una stringa grezza non apre un commento, e in Rust
    /// nemmeno da solo.
    #[test]
    fn a_rust_raw_string_is_not_a_comment() {
        let source = "let re = r#\"a === b\"#;\nlet n = 1 == 2;";
        let scanned = scan(source, Language::Rust);
        assert!(!scanned.is_code(source.find("a ===").unwrap() + 2));
        assert!(scanned.is_code(source.find("1 ==").unwrap() + 2));
    }

    #[test]
    fn a_hash_comment_hides_the_rest_of_the_line() {
        let source = "x = 1  # a == b\ny = 2 == 3";
        let scanned = scan(source, Language::Hash);
        assert!(!scanned.is_code(source.find("a ==").unwrap() + 2));
        assert!(scanned.is_code(source.find("2 ==").unwrap() + 2));
    }

    #[test]
    fn lines_are_counted_from_one() {
        let source = "a\nb\nc";
        let scanned = scan(source, Language::Braces);
        assert_eq!(scanned.line_of(0), 1);
        assert_eq!(scanned.line_of(2), 2);
        assert_eq!(scanned.line_of(4), 3);
    }

    #[test]
    fn a_function_body_is_found_around_a_line_inside_it() {
        let source = "function f(a: number) {\n  if (a > 1) {\n    return 2;\n  }\n  return 3;\n}\n";
        let scanned = scan(source, Language::Braces);
        let inside = source.find("return 2").unwrap();
        let body = enclosing_body(source, &scanned, inside).expect("un corpo");
        assert_eq!(body.header_line, 1);
        assert!(source[body.start..body.end].contains("return 3"));
    }

    /// Il blocco di un `if` non è una funzione: svuotarlo direbbe un'altra
    /// cosa, e il rapporto lo chiamerebbe col nome sbagliato.
    #[test]
    fn a_control_block_is_not_a_function_body() {
        let source = "if (a > 1) {\n  return 2;\n}\n";
        let scanned = scan(source, Language::Braces);
        let inside = source.find("return 2").unwrap();
        assert_eq!(enclosing_body(source, &scanned, inside), None);
    }

    #[test]
    fn a_rust_function_body_is_found_too() {
        let source = "fn resolve(a: usize) -> usize {\n    a + 1\n}\n";
        let scanned = scan(source, Language::Rust);
        let inside = source.find("a + 1").unwrap();
        let body = enclosing_body(source, &scanned, inside).expect("un corpo");
        assert_eq!(source[body.start..body.end].trim(), "a + 1");
    }

    #[test]
    fn the_language_comes_from_the_extension() {
        assert_eq!(Language::from_path("src/a.ts"), Language::Braces);
        assert_eq!(Language::from_path("src/a.rs"), Language::Rust);
        assert_eq!(Language::from_path("a.py"), Language::Hash);
        assert_eq!(Language::from_path("README.md"), Language::Plain);
        assert!(!Language::from_path("README.md").is_known());
    }
}
