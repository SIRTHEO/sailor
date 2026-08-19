//! Ancorare una memoria al codice che la rende vera, e accorgersi quando quel
//! codice cambia.
//!
//! IL DIFETTO. Una memoria è una fotografia con la data sopra, e il sistema la
//! rilegge come se fosse lo stato di adesso. Misurato il 19/08/2026 sulle 151
//! memorie di `orca/general`: **563 citazioni di file, 242 non si risolvono** —
//! il 43%. E quelle che si risolvono non dicono niente sul contenuto: un
//! riferimento a riga 1050 continua a puntare a una riga che nel frattempo è
//! diventata un'altra funzione.
//!
//! LA CURA, presa da Drift (Fiberplane): l'appunto porta con sé
//! `percorso#simbolo@impronta`. L'impronta è calcolata sul **corpo normalizzato**
//! del simbolo — via i commenti, via gli spazi — così riformattare non fa
//! scattare niente e cambiare il codice sì. È l'unica delle tre strade valutate
//! che **toglie** una verifica a mano invece di aggiungerne una: al posto di
//! rileggere la memoria e andare a controllare, si esegue un comando.
//!
//! COSA NON FA. Non è un parser: l'impronta è lessicale, non sintattica.
//! Spostare una funzione da un file all'altro conta come sparizione, e va bene
//! così — anche quella è una notizia per chi legge la memoria. E non giudica il
//! **senso**: dice che il codice sotto è cambiato, non che la memoria sia
//! diventata falsa. Il verdetto sul senso resta di chi legge.
//!
//! Qui non si tocca il disco: percorsi, cartelle e scritture stanno nel braccio
//! `claude-hooks/src/memory_anchors.rs`.

// ─── Il riferimento ──────────────────────────────────────────────────────────

/// Un ancoraggio: dove vive il fatto, e com'era quando l'abbiamo scritto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// Percorso del file, come scritto nella memoria (`~` compreso).
    pub path: String,
    /// Il simbolo dentro il file. Assente = l'ancoraggio è al file intero.
    pub symbol: Option<String>,
    /// L'impronta del corpo al momento del timbro. Assente = mai timbrato.
    pub print: Option<String>,
}

impl Anchor {
    /// La forma canonica, quella che si scrive nel frontmatter.
    pub fn render(&self) -> String {
        let mut s = self.path.clone();
        if let Some(sym) = &self.symbol {
            s.push('#');
            s.push_str(sym);
        }
        if let Some(p) = &self.print {
            s.push('@');
            s.push_str(p);
        }
        s
    }
}

/// Legge `percorso[#simbolo][@impronta]`.
///
/// L'ordine dei separatori conta: `@` si cerca **dopo** l'ultimo `#`, altrimenti
/// un percorso con la chiocciola dentro (`@scope/pacchetto/file.ts`) verrebbe
/// spezzato a metà e il simbolo finirebbe nell'impronta.
pub fn parse_anchor(raw: &str) -> Option<Anchor> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (head, print) = match raw.rfind('@') {
        // Una chiocciola vale come separatore solo se ciò che segue ha la forma
        // di un'impronta: esadecimale, e nient'altro.
        Some(i) if is_print(&raw[i + 1..]) => (&raw[..i], Some(raw[i + 1..].to_string())),
        _ => (raw, None),
    };
    let (path, symbol) = match head.rfind('#') {
        Some(i) => (&head[..i], Some(head[i + 1..].to_string())),
        None => (head, None),
    };
    if path.is_empty() {
        return None;
    }
    Some(Anchor {
        path: path.to_string(),
        symbol: symbol.filter(|s| !s.is_empty()),
        print,
    })
}

fn is_print(s: &str) -> bool {
    !s.is_empty() && s.len() <= 24 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ─── Il verdetto ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Il codice sotto è quello di allora.
    Steady,
    /// Il simbolo c'è ancora, ma il corpo è cambiato: l'impronta di adesso.
    Drifted { current: String },
    /// Il file c'è, il simbolo no: rinominato, spostato o cancellato.
    Vanished,
    /// Il file non esiste più al percorso scritto.
    FileGone,
    /// C'è il riferimento ma non l'impronta: nessuno l'ha mai timbrato.
    Unstamped { current: Option<String> },
}

impl Verdict {
    /// Vero quando il verdetto chiede una rilettura della memoria.
    pub fn alarming(&self) -> bool {
        matches!(
            self,
            Verdict::Drifted { .. } | Verdict::Vanished | Verdict::FileGone
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Steady => "fermo",
            Verdict::Drifted { .. } => "DERIVA",
            Verdict::Vanished => "SPARITO",
            Verdict::FileGone => "FILE MANCANTE",
            Verdict::Unstamped { .. } => "non timbrato",
        }
    }
}

/// Confronta un ancoraggio col sorgente di adesso.
///
/// `source` è `None` quando il file non esiste: il braccio ha già guardato il
/// disco, qui si decide soltanto.
pub fn judge(anchor: &Anchor, source: Option<&str>) -> Verdict {
    let Some(source) = source else {
        return Verdict::FileGone;
    };
    let lang = Lang::from_path(&anchor.path);
    let body = match &anchor.symbol {
        Some(sym) => match extract_symbol(source, sym, lang) {
            Some(b) => b,
            None => return Verdict::Vanished,
        },
        // Percorso nudo, senza simbolo e senza impronta: l'ancoraggio dice
        // «questo file esiste», e il file esiste. Un'impronta sul file intero
        // suonerebbe a ogni commit — su un `settings.json` ogni giorno — e un
        // allarme che suona sempre lo si smette di guardare.
        None if anchor.print.is_none() => return Verdict::Steady,
        None => source.to_string(),
    };
    let current = fingerprint(&normalise(&body, lang));
    match &anchor.print {
        None => Verdict::Unstamped {
            current: Some(current),
        },
        Some(before) if before.eq_ignore_ascii_case(&current) => Verdict::Steady,
        Some(_) => Verdict::Drifted { current },
    }
}

// ─── L'impronta ──────────────────────────────────────────────────────────────

/// FNV-1a a 64 bit, i primi 12 esadecimali.
///
/// Perché non sha256: l'impronta serve a dire «è cambiato», non a resistere a
/// chi vuole ingannarla, e ogni dipendenza in più su questo workspace è tempo di
/// avvio che i ganci pagano a ogni chiamata. Dodici cifre sono 48 bit: su
/// qualche migliaio di ancoraggi la collisione è teoria.
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")[..12].to_string()
}

// ─── I linguaggi ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Corpo per graffe, commenti `//` e `/* */`: Rust, TS, JS, Go, C.
    Braces,
    /// Corpo per rientro, commenti `#`: Python.
    Indent,
    /// Corpo per graffe, commenti `#`: shell.
    Shell,
    /// Nessuna struttura riconosciuta: si ancora al file intero.
    Plain,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go" | "c" | "h" | "java"
            | "swift" | "kt" | "prisma" => Lang::Braces,
            "py" => Lang::Indent,
            "sh" | "bash" | "zsh" => Lang::Shell,
            _ => Lang::Plain,
        }
    }

    fn hash_is_comment(self) -> bool {
        matches!(self, Lang::Indent | Lang::Shell | Lang::Plain)
    }

    fn slash_is_comment(self) -> bool {
        matches!(self, Lang::Braces)
    }
}

// ─── La normalizzazione ──────────────────────────────────────────────────────

/// Toglie ciò che cambia senza che il codice cambi: commenti, righe vuote,
/// rientri, spazi ripetuti.
///
/// È la riga di confine fra «l'hanno riformattato» e «l'hanno riscritto». Il
/// commento sparisce di proposito: una memoria ancorata a una funzione non deve
/// gridare perché qualcuno ha corretto un refuso nella sua premessa.
///
/// **Lo spazio fra due token si toglie, quello dentro una parola no**:
/// `format! ("x")` e `format!("x")` sono lo stesso codice, `pub fn` e `pubfn`
/// no. Serve perché un `rustfmt` che manda a capo un argomento cambierebbe ogni
/// impronta del file, e un allarme che scatta a ogni riformattazione lo si
/// spegne dopo due giorni.
///
/// **Il rientro resta solo dove è sintassi** — Python. Negli altri linguaggi il
/// corpo diventa una riga sola, così spostare una graffa non conta.
///
/// Non è un lexer: una graffa dentro una stringa la conta come graffa. Il costo
/// di sbagliare è un falso allarme su un simbolo, non una decisione presa male.
pub fn normalise(source: &str, lang: Lang) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_block_comment = false;
    for raw in source.lines() {
        let mut line = raw.trim();
        if in_block_comment {
            match line.find("*/") {
                Some(i) => {
                    line = line[i + 2..].trim();
                    in_block_comment = false;
                }
                None => continue,
            }
        }
        let mut kept = String::new();
        if lang.slash_is_comment() {
            // `/* … */` aperto su questa riga: si tiene ciò che sta prima.
            if let Some(i) = line.find("/*") {
                if line[i..].find("*/").is_none() {
                    in_block_comment = true;
                    line = line[..i].trim();
                }
            }
            match line.find("//") {
                Some(i) => kept.push_str(line[..i].trim()),
                None => kept.push_str(line),
            }
        } else if lang.hash_is_comment() && line.starts_with('#') {
            // Solo la riga **intera** di commento: un `#` a metà riga può essere
            // dentro una stringa, e in Rust sarebbe un attributo.
            continue;
        } else {
            kept.push_str(line);
        }
        let squeezed = squeeze(&kept);
        if squeezed.is_empty() {
            continue;
        }
        if matches!(lang, Lang::Indent) {
            // Il rientro è la sintassi: si conserva com'è scritto.
            let indent = raw.len() - raw.trim_start().len();
            out.push_str(&" ".repeat(indent));
            out.push_str(&squeezed);
            out.push('\n');
        } else {
            // Il confine fra due righe segue la stessa regola del confine fra
            // due token: si tiene uno spazio solo se serve a non incollare due
            // parole.
            if needs_space(out.chars().last(), squeezed.chars().next()) {
                out.push(' ');
            }
            out.push_str(&squeezed);
        }
    }
    out
}

/// Toglie gli spazi che non separano due parole.
fn squeeze(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending_space = false;
    for c in line.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space && needs_space(out.chars().last(), Some(c)) {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
    }
    out
}

fn needs_space(before: Option<char>, after: Option<char>) -> bool {
    match (before, after) {
        (Some(a), Some(b)) => is_word(a) && is_word(b),
        _ => false,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ─── L'estrazione del simbolo ────────────────────────────────────────────────

/// Il corpo del simbolo, dalla riga che lo dichiara alla sua chiusura.
///
/// La dichiarazione si riconosce dal nome preceduto da una parola chiave del
/// linguaggio: `fn`, `struct`, `def`, `class`, `const`, `function`. Un nome che
/// compare solo come chiamata non conta — altrimenti l'ancoraggio si aggancerebbe
/// al primo uso invece che alla definizione.
pub fn extract_symbol(source: &str, symbol: &str, lang: Lang) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let start = declaration_line(source, symbol, lang)?;
    let end = match lang {
        Lang::Indent => end_by_indent(&lines, start),
        _ => end_by_delimiters(&lines, start),
    };
    Some(lines[start..=end].join("\n"))
}

/// La riga (0-based) che dichiara il simbolo.
///
/// Sta a parte perche' chi propone un ancoraggio deve poter chiedere «e' una
/// definizione di primo livello?»: un `hex` o un `phase` che vive dentro una
/// funzione e' un nome comune, non un appiglio, e ancorarcisi produce allarmi
/// che non significano niente.
pub fn declaration_line(source: &str, symbol: &str, lang: Lang) -> Option<usize> {
    source.lines().position(|l| declares(l, symbol, lang))
}

/// Il simbolo che contiene una riga data (1-based), cercato all'indietro.
///
/// Serve per convertire i riferimenti `file:riga` che le memorie già portano:
/// una riga scorre al primo inserimento sopra, un simbolo no.
pub fn symbol_at_line(source: &str, line: usize, lang: Lang) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }
    for i in (0..line).rev() {
        if let Some(name) = declared_name(lines[i], lang) {
            // Il simbolo vale solo se la riga cercata cade dentro il suo corpo:
            // altrimenti si prenderebbe la funzione **precedente** per una riga
            // che sta fra due definizioni.
            let end = match lang {
                Lang::Indent => end_by_indent(&lines, i),
                _ => end_by_delimiters(&lines, i),
            };
            if line - 1 <= end {
                return Some(name);
            }
            return None;
        }
    }
    None
}

/// Le parole chiave che introducono una definizione, per linguaggio.
const KEYWORDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "impl",
    "mod",
    "const",
    "static",
    "type",
    "union",
    "macro",
    "def",
    "class",
    "function",
    "interface",
    "let",
    "var",
    "async",
];

fn declares(line: &str, symbol: &str, lang: Lang) -> bool {
    declared_name(line, lang).as_deref() == Some(symbol)
}

/// Il nome definito da questa riga, se ne definisce uno.
fn declared_name(line: &str, lang: Lang) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('*') {
        return None;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    // Shell: `nome() {` e `function nome`.
    if matches!(lang, Lang::Shell) {
        if let Some(name) = words[0].strip_suffix("()") {
            return clean_name(name);
        }
    }
    for (i, w) in words.iter().enumerate() {
        // `pub`, `export`, `async`, `unsafe` e compagnia si attraversano.
        let key = w.trim_start_matches("pub(crate)").trim_start_matches("pub");
        if KEYWORDS.contains(&key.trim()) {
            if let Some(next) = words.get(i + 1) {
                if let Some(name) = clean_name(next) {
                    // `async fn nome`: `async` non introduce niente da solo.
                    if key == "async" && KEYWORDS.contains(&name.as_str()) {
                        continue;
                    }
                    return Some(name);
                }
            }
        }
    }
    // Un metodo non porta parole chiave: `load(url) {` dentro una classe, o un
    // membro di un oggetto TypeScript. Si riconosce dalla forma — nome, parentesi,
    // e la riga che apre un blocco — e si scartano le parole del controllo di
    // flusso, che hanno la stessa forma e non definiscono niente.
    if matches!(lang, Lang::Braces) && t.ends_with('{') {
        if let Some((head, _)) = t.split_once('(') {
            let name = head.trim().trim_start_matches("async ").trim();
            if !name.is_empty()
                && !CONTROL_FLOW.contains(&name)
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Parole che aprono un blocco con la stessa forma di un metodo.
const CONTROL_FLOW: &[&str] = &[
    "if", "for", "while", "match", "switch", "catch", "loop", "else", "do", "try", "with",
    "return", "unsafe", "impl",
];

/// Vero se questo simbolo regge un ancoraggio.
///
/// Il criterio non è il rientro: un metodo dentro un `impl` o una classe è
/// rientrato ed è esattamente ciò che le memorie nominano. Quello che non regge è
/// il **legame locale** — un `let hex = …` dentro una funzione — perché è un nome
/// comune che combacia per caso, e l'ancoraggio che ne esce non dice niente.
pub fn is_anchorable(source: &str, symbol: &str, lang: Lang) -> bool {
    let Some(i) = declaration_line(source, symbol, lang) else {
        return false;
    };
    let Some(line) = source.lines().nth(i) else {
        return false;
    };
    if !line.starts_with(' ') && !line.starts_with('\t') {
        return true; // primo livello: sempre buono
    }
    let first = line.split_whitespace().next().unwrap_or("");
    let key = first
        .trim_start_matches("pub(crate)")
        .trim_start_matches("pub");
    // Rientrato: vale solo se lo introduce una parola che **definisce**.
    matches!(
        key,
        "fn" | "def"
            | "class"
            | "struct"
            | "enum"
            | "trait"
            | "type"
            | "interface"
            | "impl"
            | "function"
            | "async"
            | "static"
            | "mod"
    ) || declared_by_shape(line, lang)
}

/// Vero se la riga definisce un metodo per forma (nome, parentesi, blocco).
fn declared_by_shape(line: &str, lang: Lang) -> bool {
    let t = line.trim();
    matches!(lang, Lang::Braces)
        && t.ends_with('{')
        && t.split_once('(')
            .map(|(head, _)| {
                let name = head.trim().trim_start_matches("async ").trim();
                !name.is_empty() && !CONTROL_FLOW.contains(&name)
            })
            .unwrap_or(false)
}

/// Toglie a un identificatore ciò che lo segue: `nome(`, `nome:`, `nome<T>`,
/// `nome{`, `nome,`, `nome=`.
fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// La fine di un blocco a graffe, contando anche parentesi e quadre.
///
/// Il conteggio congiunto serve per le costanti senza corpo:
/// `const X: &[&str] = &[ … ];` non apre mai una graffa, ma apre una quadra, e
/// finisce quando il bilancio torna a zero.
fn end_by_delimiters(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    let mut opened = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let code = strip_line_comment(line);
        for c in code.chars() {
            match c {
                '{' | '(' | '[' => {
                    depth += 1;
                    opened = true;
                }
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return i;
        }
        // Dichiarazione che si chiude sulla riga stessa: `type A = B;`
        if !opened && code.trim_end().ends_with(';') {
            return i;
        }
    }
    lines.len() - 1
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// La fine di un blocco a rientro: la prima riga non vuota che torna al livello
/// della dichiarazione o più a sinistra.
fn end_by_indent(lines: &[&str], start: usize) -> usize {
    let base = indent_of(lines[start]);
    let mut last = start;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if indent_of(line) <= base {
            return last;
        }
        last = i;
    }
    last
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

// ─── Il frontmatter della memoria ────────────────────────────────────────────

/// Gli ancoraggi dichiarati nel frontmatter, nella forma
///
/// ```text
/// anchors:
///   - ~/.claude/rust/crates/guards/src/relay.rs#note_successor@a1b2c3d4e5f6
/// ```
///
/// Un parser YAML intero qui sarebbe una dipendenza per leggere una lista di
/// stringhe: si legge il blocco a mano, e ci si ferma alla prima chiave allo
/// stesso livello.
pub fn anchors_in(text: &str) -> Vec<Anchor> {
    let Some(front) = frontmatter(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut inside = false;
    for line in front.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("anchors:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = item.trim().trim_matches('"').trim_matches('\'');
            if let Some(a) = parse_anchor(item) {
                out.push(a);
            }
            continue;
        }
        // Una chiave nuova chiude la lista; una riga vuota no.
        if trimmed.is_empty() {
            continue;
        }
        break;
    }
    out
}

/// Il testo fra i due `---` di testa, senza i delimitatori.
pub fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Riscrive la memoria con questi ancoraggi al posto dei suoi.
///
/// Se il blocco `anchors:` non c'è si aggiunge in fondo al frontmatter, che è
/// dove finisce ogni chiave nuova senza disturbare `name` e `description`.
pub fn with_anchors(text: &str, anchors: &[Anchor]) -> String {
    let Some(front) = frontmatter(text) else {
        return text.to_string();
    };
    let body_start = 4 + front.len(); // "---\n" + frontmatter
    let body = &text[body_start..];

    let mut kept: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in front.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("anchors:") {
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed.starts_with("- ") || trimmed.is_empty() {
                continue;
            }
            skipping = false;
        }
        kept.push(line);
    }

    let mut front_new = kept.join("\n");
    if !front_new.ends_with('\n') {
        front_new.push('\n');
    }
    if !anchors.is_empty() {
        front_new.push_str("anchors:\n");
        for a in anchors {
            front_new.push_str("  - ");
            front_new.push_str(&a.render());
            front_new.push('\n');
        }
    }
    // `body` comincia col `\n---` di chiusura: il frontmatter nuovo finisce già
    // con un a capo, quindi si toglie quello di troppo.
    let body = body.strip_prefix('\n').unwrap_or(body);
    format!("---\n{front_new}{body}")
}

// ─── Le citazioni nel corpo ──────────────────────────────────────────────────

/// Un percorso citato fra apici inversi, con l'eventuale posizione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub path: String,
    /// Il numero di riga, quando la citazione è `file.rs:123`.
    pub line: Option<usize>,
    /// Il simbolo, quando la citazione è già `file.rs#nome`.
    pub symbol: Option<String>,
}

const CODE_EXT: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "sh", "bash", "go", "java", "swift",
    "prisma", "sql", "toml", "json", "yml", "yaml",
];

/// Estrae dalle memorie i riferimenti a file che già ci sono scritti.
///
/// Serve perché ancorare 151 memorie a mano non lo fa nessuno: il timbro parte
/// da ciò che la memoria dice già, e il riferimento `file:riga` diventa
/// `file#simbolo` quando il simbolo si trova.
/// I nomi che la memoria dichiara superati, cioe' scritti a sinistra di una
/// freccia: `` `vecchio.py` → `nuovo.rs` ``.
///
/// Legge il testo intero, blocchi citati compresi, perche' la nota di mappatura
/// si scrive quasi sempre li'.
fn mapped_names(text: &str) -> std::collections::HashSet<String> {
    let pieces: Vec<&str> = text.split('`').collect();
    let mut out = std::collections::HashSet::new();
    for i in (1..pieces.len()).step_by(2) {
        let follows_arrow = pieces
            .get(i + 1)
            .map(|after| after.trim_start())
            .is_some_and(|a| a.starts_with('→') || a.starts_with("->"));
        if follows_arrow {
            out.insert(pieces[i].trim().to_string());
        }
    }
    out
}

pub fn citations(text: &str) -> Vec<Citation> {
    let mut out: Vec<Citation> = Vec::new();
    // Le righe citate (`>`) sono la nota storica — «questo file viveva qui, ora
    // sta là» — e nominano il vecchio percorso **per dire che non c'e' piu'**.
    // Contarle fra i rimandi vivi fa salire il conteggio dei morti proprio
    // quando qualcuno bonifica una memoria: successo il 19/08/2026, 191 → 197.
    let body: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n");
    // A sinistra di una freccia c'e' il nome di ieri: la nota «dove vive oggi
    // questo codice» si scrive `handoff_common.py` → `guards/src/handoff.rs`, e
    // nomina il file vecchio **per dire che non c'e' piu'**. Chi la scrive ha
    // gia' fatto il lavoro, quindi il nome mappato e' coperto **in tutto il
    // file**, non solo su quella riga: il racconto sotto continua a nominarlo,
    // ed e' giusto che lo faccia. Segnalarlo lo stesso rendeva la bonifica
    // invisibile — sei memorie annotate a regola d'arte, morti da 24 a 22.
    // La nota si scrive quasi sempre in un blocco citato, che il filtro qui
    // sopra toglie di mezzo: la raccolta legge percio' il testo intero.
    let mapped = mapped_names(text);
    let pieces: Vec<&str> = body.split('`').collect();
    for i in (1..pieces.len()).step_by(2) {
        let token = pieces[i].trim();
        if token.is_empty() || token.contains(' ') || token.len() > 200 {
            continue;
        }
        if mapped.contains(token) {
            continue;
        }
        // Una classe di file non e' un file: `compare-*.py`, `skills/hooks/*.py`,
        // `rust/tools/oracle/*.json` nominano un insieme, e cercarli sul disco
        // li dichiarerebbe morti per sempre.
        if token.contains('*') || token.contains('?') {
            continue;
        }
        // Un token che apre col punto e' un file solo se ha una barra in mezzo.
        // Senza, e' un'estensione (`.py`), un dominio (`.lvh.me`), una classe
        // CSS (`.animate-fade-up`); con la barra in fondo e' una cartella
        // (`.claude/rules/`). Scartarli tutti insieme, com'era, portava via
        // anche i 19 `.github/workflows/*.yml` che sono file veri e citati.
        if token.starts_with('.') && (!token.contains('/') || token.ends_with('/')) {
            continue;
        }
        // Un percorso con un segnaposto e' un modello, non una citazione:
        // `<scratchpad>/consegna-effettiva.py`, `sessioni-vive/<sess>.json`,
        // `state/sessioni-vive/{sess}.json`, `suite/.../whatsapp-claim.ts`.
        // Nessuno di questi esistera' mai sul disco con quel nome, quindi il
        // rilevatore li dichiarava morti per sempre: 20 righe su 439 il
        // 19/08/2026, tutte a torto. Vale anche per l'espansione a graffe
        // (`{timeline-section,note-composer}.tsx`), che nomina due file veri e
        // nessuno dei due si chiama cosi'.
        if token.contains('<') || token.contains('{') || token.contains("...") {
            continue;
        }
        let (path_part, tail) = split_locator(token);
        // Senza punto non c'è estensione, e `rsplit('.')` su un token senza
        // punto restituisce il token intero: `Bash` diventava «estensione bash»
        // e finiva fra i file introvabili, sette volte. Anche `sh` e `Write`.
        let Some((_, ext)) = path_part.rsplit_once('.') else {
            continue;
        };
        if !CODE_EXT.contains(&ext.to_ascii_lowercase().as_str()) {
            continue;
        }
        let cit = Citation {
            path: path_part.to_string(),
            line: tail
                .as_ref()
                .and_then(|t| t.strip_prefix(':'))
                .and_then(|n| n.split('-').next().unwrap_or(n).parse::<usize>().ok()),
            symbol: tail
                .as_ref()
                .and_then(|t| t.strip_prefix('#'))
                .map(|s| s.to_string()),
        };
        if !out.contains(&cit) {
            out.push(cit);
        }
    }
    out
}

/// Gli identificatori citati fra apici inversi: `note_successor`,
/// `guards::handoff::resolve_terminal_handle`, `judge`.
///
/// Serve perché il riferimento a riga è raro — 16 citazioni su 563 — mentre il
/// nome della funzione la memoria lo scrive quasi sempre. Chi chiama decide se
/// quel nome è davvero un simbolo: qui si raccolgono i candidati, e la prova è
/// che il simbolo si trovi nel file citato dalla stessa memoria.
pub fn identifiers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for chunk in text.split('`').skip(1).step_by(2) {
        let token = chunk.trim();
        if token.contains(' ') || token.contains('/') {
            continue;
        }
        // Un percorso di file non è un identificatore: lo dice l'estensione.
        if token
            .rsplit_once('.')
            .is_some_and(|(_, ext)| CODE_EXT.contains(&ext.to_ascii_lowercase().as_str()))
        {
            continue;
        }
        // L'ultimo segmento di `guards::handoff::resolve_terminal_handle` è il
        // nome che compare nel file. Vale anche per `Classe.metodo` e
        // `oggetto.campo`: sono 221 dei 708 identificatori citati dalle memorie,
        // e prendere solo la prima parte li perdeva tutti.
        let name = token.rsplit("::").next().unwrap_or(token);
        let name = name.rsplit('.').next().unwrap_or(name);
        let name = name.trim_end_matches("()");
        if name.len() < 3 || name.len() > 60 {
            continue;
        }
        let mut chars = name.chars();
        let starts_well = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !starts_well || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if !out.iter().any(|x| x == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Separa `percorso` da `:123`, `:12-30` o `#simbolo`.
fn split_locator(token: &str) -> (&str, Option<&str>) {
    if let Some(i) = token.rfind('#') {
        return (&token[..i], Some(&token[i..]));
    }
    if let Some(i) = token.rfind(':') {
        let tail = &token[i + 1..];
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return (&token[..i], Some(&token[i..]));
        }
    }
    (token, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = r#"
//! premessa del modulo
use std::fmt;

/// Il commento che spiega.
pub fn greet(name: &str) -> String {
    // un commento dentro
    format!("ciao {name}")
}

pub const LIST: &[&str] = &[
    "uno",
    "due",
];

fn other() -> u8 {
    7
}
"#;

    const PY: &str =
        "import os\n\n\ndef first(a):\n    # nota\n    return a + 1\n\n\ndef second(b):\n    return b\n";

    #[test]
    fn reads_the_three_parts_of_a_reference() {
        let a = parse_anchor("src/relay.rs#note_successor@a1b2c3").unwrap();
        assert_eq!(a.path, "src/relay.rs");
        assert_eq!(a.symbol.as_deref(), Some("note_successor"));
        assert_eq!(a.print.as_deref(), Some("a1b2c3"));
        assert_eq!(a.render(), "src/relay.rs#note_successor@a1b2c3");
    }

    #[test]
    fn an_at_sign_inside_a_path_is_not_a_fingerprint() {
        let a = parse_anchor("@scope/pacchetto/src/api.ts#load").unwrap();
        assert_eq!(a.path, "@scope/pacchetto/src/api.ts");
        assert_eq!(a.symbol.as_deref(), Some("load"));
        assert_eq!(a.print, None);
    }

    #[test]
    fn extracts_a_rust_function_and_stops_at_the_brace() {
        let body = extract_symbol(RUST, "greet", Lang::Braces).unwrap();
        assert!(body.starts_with("pub fn greet"));
        assert!(body.trim_end().ends_with('}'));
        assert!(!body.contains("LIST"));
    }

    #[test]
    fn extracts_a_const_without_braces() {
        let body = extract_symbol(RUST, "LIST", Lang::Braces).unwrap();
        assert!(body.contains("\"due\""));
        assert!(!body.contains("fn other"));
    }

    #[test]
    fn extracts_a_python_function_by_indent() {
        let body = extract_symbol(PY, "first", Lang::Indent).unwrap();
        assert!(body.starts_with("def first"));
        assert!(body.contains("return a + 1"));
        assert!(!body.contains("def second"));
    }

    #[test]
    fn reformatting_does_not_move_the_fingerprint() {
        let before = extract_symbol(RUST, "greet", Lang::Braces).unwrap();
        let reformatted = RUST
            .replace("// un commento dentro", "// tutt'altro commento")
            .replace("    format!", "        format!    ");
        let after = extract_symbol(&reformatted, "greet", Lang::Braces).unwrap();
        assert_eq!(
            fingerprint(&normalise(&before, Lang::Braces)),
            fingerprint(&normalise(&after, Lang::Braces))
        );
    }

    #[test]
    fn changing_the_code_moves_the_fingerprint() {
        let before = extract_symbol(RUST, "greet", Lang::Braces).unwrap();
        let changed = RUST.replace("format!(\"ciao {name}\")", "format!(\"salve {name}\")");
        let after = extract_symbol(&changed, "greet", Lang::Braces).unwrap();
        assert_ne!(
            fingerprint(&normalise(&before, Lang::Braces)),
            fingerprint(&normalise(&after, Lang::Braces))
        );
    }

    #[test]
    fn the_five_verdicts() {
        let body = extract_symbol(RUST, "greet", Lang::Braces).unwrap();
        let print = fingerprint(&normalise(&body, Lang::Braces));

        let steady = parse_anchor(&format!("a.rs#greet@{print}")).unwrap();
        assert_eq!(judge(&steady, Some(RUST)), Verdict::Steady);

        let drifted = parse_anchor("a.rs#greet@000000000000").unwrap();
        assert!(matches!(
            judge(&drifted, Some(RUST)),
            Verdict::Drifted { .. }
        ));

        let vanished = parse_anchor("a.rs#never_existed@000000000000").unwrap();
        assert_eq!(judge(&vanished, Some(RUST)), Verdict::Vanished);

        assert_eq!(judge(&steady, None), Verdict::FileGone);

        let bare = parse_anchor("a.rs#greet").unwrap();
        assert!(matches!(
            judge(&bare, Some(RUST)),
            Verdict::Unstamped { .. }
        ));
    }

    #[test]
    fn a_rust_attribute_is_not_a_comment() {
        let src = "#[derive(Debug)]\npub struct A {\n    b: u8,\n}\n";
        let n = normalise(src, Lang::Braces);
        assert!(n.contains("#[derive(Debug)]"), "{n}");
    }

    #[test]
    fn a_whole_python_comment_line_disappears() {
        let n = normalise("# nota\nx = 1\n", Lang::Indent);
        assert_eq!(n, "x=1\n");
    }

    /// In Python il rientro è sintassi, e un blocco spostato di un livello è
    /// codice diverso: qui l'impronta deve muoversi.
    #[test]
    fn a_python_dedent_moves_the_fingerprint() {
        let inside = "def f(a):\n    if a:\n        return 1\n    return 0\n";
        let outside = "def f(a):\n    if a:\n        return 1\n        return 0\n";
        assert_ne!(
            fingerprint(&normalise(inside, Lang::Indent)),
            fingerprint(&normalise(outside, Lang::Indent))
        );
    }

    /// Un `rustfmt` che manda a capo un argomento non è una notizia.
    #[test]
    fn a_line_break_inside_a_call_does_not_move_the_fingerprint() {
        let one = "fn f() {\n    call(a, b);\n}\n";
        let wrapped = "fn f() {\n    call(\n        a,\n        b,\n    );\n}\n";
        // La virgola in coda che rustfmt aggiunge resta una differenza vera di
        // testo: si confronta ciò che il formattatore produce davvero.
        let wrapped = wrapped.replace("b,\n", "b\n");
        assert_eq!(
            fingerprint(&normalise(one, Lang::Braces)),
            fingerprint(&normalise(&wrapped, Lang::Braces))
        );
    }

    #[test]
    fn from_a_line_to_its_symbol() {
        let line = RUST.lines().position(|l| l.contains("format!")).unwrap() + 1;
        assert_eq!(
            symbol_at_line(RUST, line, Lang::Braces).as_deref(),
            Some("greet")
        );
    }

    #[test]
    fn a_line_between_two_definitions_has_no_symbol() {
        let line = RUST.lines().position(|l| l.starts_with("use std")).unwrap() + 1;
        assert_eq!(symbol_at_line(RUST, line, Lang::Braces), None);
    }

    #[test]
    fn reads_the_citations_in_the_body() {
        let text = "La cura sta in `relay.rs:1050`, e il simbolo è `guards/src/chain.rs#judge`. \
                     Non `una frase lunga`, non `MEMORY.md` senza estensione di codice.";
        let c = citations(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].path, "relay.rs");
        assert_eq!(c[0].line, Some(1050));
        assert_eq!(c[1].symbol.as_deref(), Some("judge"));
    }

    #[test]
    fn the_left_side_of_an_arrow_is_yesterdays_name() {
        let text = "Dove vive oggi: `handoff_common.py` → `guards/src/handoff.rs`, \
                    e `relay.py` -> `claude-hooks/src/relay.rs`.";
        let c = citations(text);
        let paths: Vec<&str> = c.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["guards/src/handoff.rs", "claude-hooks/src/relay.rs"]
        );
    }

    #[test]
    fn a_mapped_name_stays_covered_further_down_the_page() {
        // La nota sta in un blocco citato, il racconto continua a nominare il
        // file vecchio: e' proprio quello che deve fare, e non va segnalato.
        let text = "> `handoff_common.py` → `rust/crates/guards/src/handoff.rs`\n\
                    \n\
                    In `handoff_common.py` la soglia era 200k, e `attese.py` non e' mappato.";
        let paths: Vec<String> = citations(text).into_iter().map(|c| c.path).collect();
        assert_eq!(paths, vec!["attese.py"], "{paths:?}");
    }

    #[test]
    fn a_placeholder_in_the_path_is_a_template_not_a_citation() {
        let text = "Lo strumento sta in `<scratchpad>/consegna-effettiva.py`, lo stato in \
                    `state/sessioni-vive/{sess}.json`, la rotta in `suite/.../claim.ts`. \
                    Il file vero è `relay.rs`.";
        let c = citations(text);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].path, "relay.rs");
    }

    #[test]
    fn reads_the_identifiers_the_memory_names() {
        let text = "Si passa da `guards::handoff::resolve_terminal_handle`, che parte da \
                    `ORCA_TAB_ID`. Non `src/relay.rs`, non `un po` di prosa, non `ab`.";
        let ids = identifiers(text);
        assert!(
            ids.contains(&"resolve_terminal_handle".to_string()),
            "{ids:?}"
        );
        assert!(ids.contains(&"ORCA_TAB_ID".to_string()));
        assert!(!ids.iter().any(|i| i.contains('/')));
        assert!(!ids.contains(&"ab".to_string()));
    }

    #[test]
    fn the_declaration_line_tells_a_top_level_symbol_from_a_nested_one() {
        let src = "fn outer() {\n    let hidden = 1;\n}\n";
        let outer = declaration_line(src, "outer", Lang::Braces).unwrap();
        let hidden = declaration_line(src, "hidden", Lang::Braces).unwrap();
        assert!(!src.lines().nth(outer).unwrap().starts_with(' '));
        assert!(src.lines().nth(hidden).unwrap().starts_with(' '));
    }

    const IMPL: &str = "impl Relay {\n    pub fn regenerate(&self) -> u8 {\n        let hex = 1;\n        hex\n    }\n}\n";
    const TS: &str = "export class Api {\n  load(url: string) {\n    const parsed = 1;\n    return parsed;\n  }\n}\n\nexport function fetchAll() {\n  return 2;\n}\n";

    #[test]
    fn a_method_inside_an_impl_holds_an_anchor() {
        assert!(is_anchorable(IMPL, "regenerate", Lang::Braces));
        let body = extract_symbol(IMPL, "regenerate", Lang::Braces).unwrap();
        assert!(body.contains("let hex"));
    }

    #[test]
    fn a_local_binding_does_not_hold_an_anchor() {
        // `hex` esiste nel file e si estrae, ma e' un nome comune che combacia
        // per caso: 4 ancoraggi su 20 della prima passata erano di questa specie.
        assert!(extract_symbol(IMPL, "hex", Lang::Braces).is_some());
        assert!(!is_anchorable(IMPL, "hex", Lang::Braces));
    }

    #[test]
    fn a_class_method_is_recognised_by_its_shape() {
        assert_eq!(
            declared_name("  load(url: string) {", Lang::Braces).as_deref(),
            Some("load")
        );
        assert!(is_anchorable(TS, "load", Lang::Braces));
        assert!(is_anchorable(TS, "fetchAll", Lang::Braces));
        assert!(!is_anchorable(TS, "parsed", Lang::Braces));
    }

    #[test]
    fn a_control_word_is_not_a_method() {
        for line in ["  if (a) {", "    for (const x of y) {", "  while (true) {"] {
            assert_eq!(declared_name(line, Lang::Braces), None, "su {line}");
        }
    }

    #[test]
    fn a_word_without_a_dot_is_not_a_file() {
        let text = "Il gancio su `Bash`, la fase `sh`, e il vero file `chain.rs`.";
        let c = citations(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].path, "chain.rs");
    }

    #[test]
    fn a_quoted_note_is_history_not_a_reference() {
        let text = "> Dove vive oggi: `relay.py` → `relay.rs`\n\nIl gancio `chain.rs` decide.";
        let c = citations(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].path, "chain.rs");
    }

    #[test]
    fn a_class_of_files_is_not_a_citation() {
        let text = "I banchi `compare-*.py` e i moduli `skills/hooks/*.py` sono spariti, \
                    e cosi' ogni `.py` di quella cartella. Resta `relay.rs`.";
        let c = citations(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].path, "relay.rs");
    }

    #[test]
    fn a_dotted_token_is_a_citation_only_with_a_slash_inside() {
        // Scartare tutto cio' che apre col punto portava via i 19
        // `.github/workflows/*.yml` citati dalle memorie, che sono file veri.
        let text = "Il flusso `.github/workflows/ci.yml` gira su `.lvh.me`, la classe \
                    `.animate-fade-up` anima la riga, ogni `.py` e' migrato e le regole \
                    stanno in `.claude/rules/`.";
        let c = citations(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].path, ".github/workflows/ci.yml");
    }

    #[test]
    fn writes_and_rewrites_the_frontmatter_block() {
        let memory = "---\nname: prova\ndescription: una prova\n---\n\nIl corpo resta.\n";
        let one = parse_anchor("a.rs#greet@aaaa").unwrap();
        let written = with_anchors(memory, &[one]);
        assert!(written.contains("anchors:\n  - a.rs#greet@aaaa"));
        assert!(written.contains("Il corpo resta."));
        assert_eq!(anchors_in(&written).len(), 1);

        let two = parse_anchor("b.py#first@bbbb").unwrap();
        let rewritten = with_anchors(&written, &[two]);
        assert!(!rewritten.contains("a.rs#greet"));
        assert_eq!(anchors_in(&rewritten).len(), 1);
        assert!(rewritten.contains("description: una prova"));
    }

    #[test]
    fn a_memory_without_frontmatter_stays_untouched() {
        let text = "solo corpo\n";
        let a = parse_anchor("a.rs@aaaa").unwrap();
        assert_eq!(with_anchors(text, &[a]), text);
        assert!(anchors_in(text).is_empty());
    }
}
