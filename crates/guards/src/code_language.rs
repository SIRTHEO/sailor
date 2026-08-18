//! Segnala l'italiano dove la convenzione chiede l'inglese.
//!
//! Porta di `skills/hooks/code-language.py`. Il riconoscimento della lingua sta
//! in `crate::language`, che era già condiviso con `pr-title`; qui c'è cosa si
//! guarda, dove, e come lo si dice.
//!
//! PERCHÉ ESISTE. Decisione di Theo del 13/08/2026, raffinata il 14/08: le
//! descrizioni dei test e i messaggi degli script vanno in inglese, i commenti
//! restano in italiano. Il 14/08 la prescrizione era già scritta e già
//! disattesa — descrizioni dei test all'87/65/95/86% in italiano nei quattro
//! repo, **e in crescita**. L'unica superficie che reggeva era quella dei
//! messaggi di commit, ed era l'unica con un controllo.
//!
//! ```text
//! descrizioni dei test        describe/it/test      inglese
//! messaggi di script e gate   echo/console/throw    inglese
//! nomi di funzioni e file     —                     inglese
//! commenti nel sorgente       —                     italiano, non guardati
//! ```
//!
//! Legge **solo ciò che è stato appena scritto**: su `Edit` il testo nuovo, su
//! `Write` il contenuto consegnato. Un controllo che leggesse il file intero
//! accuserebbe per righe scritte mesi fa da altri, e al primo rimprovero
//! ingiusto verrebbe spento.
//!
//! È un rilevatore **prudente, non esatto**: la copertura misurata il 14/08 su
//! un corpus vero è 4 su 6, e va detto ovunque lo si citi.

use crate::language::{is_italian, is_italian_name};
use regex::Regex;
use std::sync::OnceLock;

/// Quante voci si elencano prima di riassumere.
const MAX_ENTRIES: usize = 8;

/// Le due famiglie di file dove la convenzione chiede l'inglese.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Family {
    Test,
    Gate,
}

/// `/crates/` e' entrato il 18/08/2026 insieme a `.rs`: senza, l'aggiunta
/// dell'estensione non copriva un solo file, perche' i 55 sorgenti Rust della
/// configurazione stanno tutti sotto `rust/crates/`.
const GATE_DIRS: &[&str] = &[
    "/scripts/", "/.github/workflows/", "/hooks/", "/.husky/", "/crates/",
];

/// `.py` è entrato il 14/08/2026: i ganci che applicano questa regola sono
/// scritti in Python e stanno in `hooks/`, cartella già sorvegliata — cioè il
/// controllo non si applicava a sé stesso.
/// `.rs` è entrato il 18/08/2026, cinque giorni dopo la prescrizione: 201 nomi
/// di funzione italiani nei sorgenti Rust, in 55 file toccati *dopo* la
/// richiesta. Il codice nuovo nasceva fuori dal controllo mentre lo si scriveva.
const GATE_EXTENSIONS: &[&str] = &[
    ".mjs", ".js", ".cjs", ".ts", ".sh", ".bash", ".yml", ".yaml", ".py", ".rs",
];

const EXCLUDED: &[&str] = &[
    "node_modules", "/dist/", "/build/", "/out/", "/.next/", "/generated/",
    "/.mastra/", "/coverage/",
];

fn test_file() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.(test|spec)\.[cm]?[jt]sx?$").unwrap())
}

/// A quale famiglia appartiene il file, se a una.
pub fn family(path: &str) -> Option<Family> {
    let p = path.replace('\\', "/");
    if EXCLUDED.iter().any(|x| p.contains(x)) {
        return None;
    }
    if test_file().is_match(&p) {
        return Some(Family::Test);
    }
    if GATE_DIRS.iter().any(|d| p.contains(d)) && GATE_EXTENSIONS.iter().any(|e| suffix_is(&p, e)) {
        return Some(Family::Gate);
    }
    None
}

/// `Path(p).suffix` di Python: l'ultima estensione, e solo se il nome non
/// comincia col punto. `x.test.ts` ha suffisso `.ts`, `.gitignore` non ne ha.
fn suffix_is(path: &str, wanted: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) | None => false,
        Some(i) => &name[i..] == wanted,
    }
}

/// Il punto NON attraversa le righe nel primo ramo: con il flag globale il
/// commento di una riga si mangiava tutto il resto del file, e il controllo
/// taceva su qualunque cosa venisse dopo. L'ha trovato la prova mutante, non la
/// rilettura.
fn comment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[ \t]*(?:#|//).*$|(?s:/\*.*?\*/)").unwrap())
}

/// Via i commenti prima di cercare: citare una chiamata non è farla.
///
/// Un commento italiano che nominava una funzione di stampa fra apici inversi
/// veniva letto come una chiamata vera, e il gancio bloccava chi documenta bene
/// — con la via d'uscita più comoda che era peggiorare il commento.
pub fn strip_comments(text: &str) -> String {
    comment().replace_all(text, " ").into_owned()
}

/// Le descrizioni dei test.
///
/// L'originale chiude la stringa con una backreference — `(['"`])(.*?)\1` — che
/// il motore di Rust non ha: qui le tre virgolette sono tre alternative
/// esplicite, `[^']*` al posto di `.*?`. Accettano lo stesso testo, perché il
/// non-greedy si ferma comunque alla prima occorrenza della virgoletta.
///
/// Una regex con backreference non si porta meccanicamente, ed è proprio qui
/// che si è visto: l'originale aveva `.+?` — con il più — e su `echo ''` non
/// poteva chiudere sulla stringa vuota, quindi agganciava il prossimo apice del
/// file, anche mille righe più in là, segnalando come «italiana» una porzione
/// arbitraria di script. Quattro file veri su 1.292. Corretto in entrambe le
/// implementazioni il 17/08/2026: è un cambio di comportamento, ed è quello
/// voluto — un rimprovero a torto è ciò che fa spegnere un controllo.
fn descriptions() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?s)\b(?:describe|it|test)(?:\.\w+)?\s*\(\s*(?:'([^']*)'|"([^"]*)"|`([^`]*)`)"#,
        )
        .unwrap()
    })
}

/// I messaggi degli script e dei gate.
fn messages() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?s)(?:(?:console\.\w+|throw\s+new\s+\w*Error|echo|printf)\s*\(?|(?:print|sys\.exit|raise\s+\w*Error)\s*\()\s*(?:'([^']*)'|"([^"]*)"|`([^`]*)`)"#,
        )
        .unwrap()
    })
}

fn interpolation() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{[^}]*\}").unwrap())
}

/// Le stringhe da giudicare. I commenti non entrano: restano in italiano.
pub fn strings(text: &str, family: Family) -> Vec<String> {
    let text = strip_comments(text);
    let rule = if family == Family::Test {
        descriptions()
    } else {
        messages()
    };
    let mut found = Vec::new();
    for m in rule.captures_iter(&text) {
        let Some(raw) = (1..=3).find_map(|i| m.get(i)) else {
            continue;
        };
        let s = raw.as_str().trim();
        // Un'interpolazione sola (`${x}`) o una stringa cortissima non dicono
        // niente sulla lingua.
        let bare = interpolation().replace_all(s, "");
        let bare = bare.trim();
        if bare.chars().count() < 12 {
            continue;
        }
        if is_italian(bare) {
            found.push(cut(&s.replace('\n', " "), 70));
        }
    }
    found
}

/// Taglia a un numero di **caratteri**, come l'affettamento di Python.
fn cut(s: &str, limit: usize) -> String {
    s.chars().take(limit).collect()
}

fn declared() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?:^|\n)\s*(?:def|class)\s+([A-Za-z_]\w*)",             // python
            r"|(?:^|\n)\s*([A-Za-z_]\w*)\s*(?::[^=\n]+)?=[^=]",       // python: assegnazione
            r"|\b(?:function|const|let|var|class)\s+([A-Za-z_$]\w*)", // js/ts
            // Rust, aggiunto il 18/08/2026: senza, il gate accettava i `.rs` e
            // non sapeva leggerli. Estensione e cartella senza la sintassi
            // coprono zero casi, e il controllo sembra acceso mentre e' cieco.
            r"|\b(?:fn|struct|enum|trait|mod|type|impl)\s+([A-Za-z_]\w*)", // rust
        ))
        .unwrap()
    })
}

/// Gli identificatori italiani fra quelli **dichiarati** nel testo nuovo.
///
/// Solo le dichiarazioni, non ogni occorrenza: chi *usa* un nome italiano
/// scritto altrove non lo sta introducendo, e segnalarglielo sposterebbe la
/// colpa sul chiamante invece che su chi lo ha creato.
pub fn italian_names(text: &str) -> Vec<String> {
    let text = strip_comments(text);
    let mut found: Vec<String> = Vec::new();
    for m in declared().captures_iter(&text) {
        // 1..=4, non 1..=3: il Python scorre `m.groups()` per intero, quindi
        // un ramo nuovo nell'espressione gli arriva da solo. Qui l'intervallo e'
        // scritto a mano, e il 18/08/2026 il ramo Rust — il quarto — e' rimasto
        // muto per questo: il gate diceva di guardare i `.rs` e non li leggeva.
        // Chi aggiunge un gruppo alla regex deve allargare anche questo.
        let name = (1..=4)
            .find_map(|i| m.get(i))
            .map(|g| g.as_str())
            .unwrap_or("");
        // `name.isupper()` di Python: almeno una maiuscola e nessuna minuscola.
        let all_upper = name.chars().any(char::is_uppercase) && !name.chars().any(char::is_lowercase);
        if name.chars().count() < 4 || (all_upper && name.chars().count() < 6) {
            continue;
        }
        if is_italian_name(name) && !found.iter().any(|f| f == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// Vero se il **nome del file** è italiano. Un file si rinomina una volta sola,
/// quando nasce: dopo, ogni gancio e ogni regola lo nominano.
pub fn italian_filename(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    let stem = base.split('.').next().unwrap_or(base);
    is_italian_name(stem)
}

/// Le vie d'uscita, dichiarate in testa al file che le usa e visibili nel diff.
///
/// La prima serve a un caso solo ma inevitabile — il banco di prova di un
/// controllo sulla lingua deve contenere esempi nella lingua che vieta. La
/// seconda è del 16/08: alcuni script non stampano messaggi, **scrivono un
/// documento** che leggerà una persona, e per la regola quella è prosa, che sta
/// in italiano; tradurla sarebbe un peggioramento fatto per compiacere un
/// controllo.
///
/// La misura del 14/08/2026 dice che una via d'uscita visibile quasi non viene
/// usata (5 volte in 1.280 file), mentre una che non lascia traccia viene usata
/// sempre.
pub const EXEMPT_MARKERS: &[&str] = &[
    "lingua-codice: banco di prova",
    "lingua-codice: documento per umani",
];

/// Vero se il file dichiara in testa un banco di prova o un documento.
pub fn is_exempt(path: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let head: String = text.lines().take(60).collect::<Vec<_>>().join("\n");
    EXEMPT_MARKERS.iter().any(|m| head.contains(m))
}

pub fn report(family: Family, found: &[String]) -> String {
    let where_ = if family == Family::Test {
        "Le descrizioni dei test vanno in inglese"
    } else {
        "I messaggi di questo script vanno in inglese"
    };
    let lines: Vec<String> = found
        .iter()
        .take(MAX_ENTRIES)
        .map(|s| format!("    · {s}"))
        .collect();
    let extra = if found.len() > MAX_ENTRIES {
        format!("\n    ... e altre {}", found.len() - MAX_ENTRIES)
    } else {
        String::new()
    };
    format!(
        "{where_} (decisione di Theo, 13/08/2026, raffinata il 14/08).\n\
         Le legge chi guarda una pipeline o un registro, non chi lavora qui.\n\n\
         Scritte in italiano:\n\n{}{extra}\n\n\
         I commenti restano in italiano: quelli non si toccano.\n\
         Traduci queste ora, nello stesso turno.",
        lines.join("\n")
    )
}

pub fn report_names(names: &[String], filename: Option<&str>) -> String {
    let lines: Vec<String> = names
        .iter()
        .take(MAX_ENTRIES)
        .map(|n| format!("    · {n}"))
        .collect();
    let extra = if names.len() > MAX_ENTRIES {
        format!("\n    ... e altri {}", names.len() - MAX_ENTRIES)
    } else {
        String::new()
    };
    let head = if filename.is_some() {
        "Il nome di questo file va in inglese"
    } else {
        "Gli identificatori vanno in inglese"
    };
    let body = match filename {
        Some(f) => format!("    · {f}\n"),
        None => format!("{}{extra}\n", lines.join("\n")),
    };
    format!(
        "{head} (regola globale §Lingua: «identificatori di codice, nomi di \
         file, comandi» restano in inglese).\n\
         Misurato il 16/08/2026: 322 identificatori italiani in 29 file su 51, \
         perche' nessun controllo li guardava.\n\n\
         In italiano:\n\n{body}\n\
         I commenti restano in italiano: quelli non si toccano.\n\
         Dai un nome inglese ora, prima che lo citino i ganci e le regole."
    )
}

/// Il testo appena scritto: su `Edit` il nuovo, su `Write` tutto.
/// Il gate nasceva cieco su Bash: guardava `Write|Edit|MultiEdit` e non vedeva
/// `cat > file <<EOF`. La modalità operativa di alcune sessioni prescrive proprio
/// Bash al posto di Write, quindi il buco non era teorico — il 18/08/2026 un
/// blocco Rust è passato senza controllo, e il gate ha parlato solo quando la
/// stessa sessione è tornata a Edit. Un gate aggirabile è peggio di nessun gate.
///
/// Niente riferimento all'indietro sul delimitatore: la crate `regex` non li ha,
/// e gli apici si tolgono dopo — dalla stessa parte del gemello Python.
fn heredoc() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?:^|[|;&]|\n)\s*(?:cat|tee)\s+(?:-a\s+)?(?:>>?\s*)?",
            r"([^\s<>|;&]+)\s*(?:>>?\s*[^\s<>|;&]+\s*)?<<-?\s*",
            r"(['\x22]?[A-Za-z_][A-Za-z0-9_]*['\x22]?)",
        ))
        .unwrap()
    })
}

fn redirect() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|[|;&])\s*(?:echo|printf)\s+(.*?)\s*>>?\s*([^\s<>|;&]+)\s*$")
            .unwrap()
    })
}

/// Il corpo di un heredoc: dalla riga dopo l'apertura fino al delimitatore da
/// solo su una riga. Se il delimitatore non si chiude il comando è troncato, e
/// un comando troncato non si giudica.
fn heredoc_body(command: &str, delimiter: &str, start: usize) -> Option<String> {
    let rest = &command[start..];
    let newline = rest.find('\n')?;
    let mut body: Vec<&str> = Vec::new();
    for line in rest[newline + 1..].split('\n') {
        if line.trim() == delimiter {
            return Some(body.join("\n"));
        }
        body.push(line);
    }
    None
}

/// Le scritture su file che un comando Bash sta per fare, come coppie
/// (percorso, contenuto). Copre gli heredoc e le redirezioni di `echo`/`printf`:
/// sono le due forme con cui una sessione scrive codice senza passare da `Write`.
/// Quel che non si estrae con certezza non si giudica — meglio un buco noto che
/// un falso allarme, che insegna a ignorare il gate.
pub fn writes_from_bash(command: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    if command.is_empty() {
        return found;
    }
    for m in heredoc().captures_iter(command) {
        let target = m.get(1).map(|x| x.as_str()).unwrap_or("");
        let raw = m.get(2).map(|x| x.as_str()).unwrap_or("");
        let delimiter = raw.trim_matches(|c| c == '\'' || c == '"');
        let end = m.get(0).map(|x| x.end()).unwrap_or(0);
        if let Some(body) = heredoc_body(command, delimiter, end) {
            found.push((target.to_string(), body));
        }
    }
    for m in redirect().captures_iter(command) {
        let arg = m.get(1).map(|x| x.as_str()).unwrap_or("").trim().to_string();
        let arg = if arg.len() >= 2
            && (arg.starts_with('\'') || arg.starts_with('"'))
            && arg.ends_with(arg.chars().next().unwrap())
        {
            arg[1..arg.len() - 1].to_string()
        } else {
            arg
        };
        let target = m.get(2).map(|x| x.as_str()).unwrap_or("");
        found.push((target.to_string(), arg));
    }
    found
}

pub fn written_text(tool_input: &serde_json::Value) -> String {
    if let Some(s) = tool_input.get("new_string").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(edits) = tool_input.get("edits").and_then(|v| v.as_array()) {
        return edits
            .iter()
            .map(|e| e.get("new_string").and_then(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
    }
    tool_input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Il rimprovero, o `None` se non c'è niente da dire.
///
/// I nomi vengono prima: un identificatore sbagliato lo citeranno i ganci, le
/// regole e le memorie, e rinominarlo dopo costa dieci volte tanto.
pub fn judge(path: &str, text: &str, file_exists: bool) -> Option<String> {
    let family = family(path)?;
    if is_exempt(path) {
        return None;
    }
    let names = italian_names(text);
    if !file_exists && italian_filename(path) {
        let base = path.rsplit('/').next().unwrap_or(path);
        return Some(report_names(&[], Some(base)));
    }
    if !names.is_empty() {
        return Some(report_names(&names, None));
    }
    let found = strings(text, family);
    if found.is_empty() {
        return None;
    }
    Some(report(family, &found))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il canarino del difetto del 18/08/2026: `italian_names` leggeva i gruppi
    /// di cattura 1..=3 mentre il ramo Rust dell'espressione era il quarto, e il
    /// gate diceva di guardare i `.rs` senza leggerne un solo nome. Chi
    /// restringe di nuovo quell'intervallo fa fallire questa prova.
    #[test]
    fn a_rust_declaration_is_read_like_a_python_one() {
        assert_eq!(italian_names("fn raccogli_worktree() {}"), ["raccogli_worktree"]);
        assert!(italian_names("fn collect_worktrees() {}").is_empty());
        assert_eq!(italian_names("struct Elenco_Copie;"), ["Elenco_Copie"]);
    }

    #[test]
    fn a_heredoc_write_is_seen() {
        let command = "cat > /x/crates/a.rs <<'EOF'\nfn raccogli() {}\nEOF";
        assert_eq!(
            writes_from_bash(command),
            [("/x/crates/a.rs".to_string(), "fn raccogli() {}".to_string())]
        );
    }

    #[test]
    fn an_unquoted_delimiter_works_too() {
        let command = "cat > /x/crates/a.rs <<PY\nfn raccogli() {}\nPY";
        assert_eq!(writes_from_bash(command).len(), 1);
    }

    /// Un comando troncato non si giudica: senza il delimitatore di chiusura non
    /// si sa dove finisca il corpo, e indovinarlo produce falsi allarmi.
    #[test]
    fn an_unclosed_heredoc_yields_nothing() {
        let command = "cat > /x/crates/a.rs <<'EOF'\nfn raccogli() {}";
        assert!(writes_from_bash(command).is_empty());
    }

    #[test]
    fn an_echo_redirect_is_seen_and_unquoted() {
        let command = "echo 'fn raccogli() {}' > /x/crates/a.rs";
        assert_eq!(
            writes_from_bash(command),
            [("/x/crates/a.rs".to_string(), "fn raccogli() {}".to_string())]
        );
    }

    #[test]
    fn a_command_that_writes_nothing_yields_nothing() {
        assert!(writes_from_bash("git status --short").is_empty());
        assert!(writes_from_bash("").is_empty());
    }

    /// La cartella conta quanto l'estensione: i 55 sorgenti Rust della
    /// configurazione stanno sotto `crates/`, e senza quella voce l'aggiunta di
    /// `.rs` non copriva un solo file.
    #[test]
    fn rust_sources_of_the_configuration_are_covered() {
        assert_eq!(family("/Users/x/.claude/rust/crates/guards/src/a.rs"), Some(Family::Gate));
        assert_eq!(family("/Users/x/repo/src/main.rs"), None);
    }

    #[test]
    fn it_knows_which_files_the_rule_covers() {
        assert_eq!(family("/x/a.test.ts"), Some(Family::Test));
        assert_eq!(family("/x/a.spec.tsx"), Some(Family::Test));
        assert_eq!(family("/x/scripts/deploy.sh"), Some(Family::Gate));
        assert_eq!(family("/x/hooks/guard.py"), Some(Family::Gate));
        assert_eq!(family("/x/src/index.ts"), None);
        // il pregresso escluso non si giudica
        assert_eq!(family("/x/node_modules/a.test.ts"), None);
        assert_eq!(family("/x/dist/scripts/deploy.sh"), None);
    }

    #[test]
    fn a_test_description_in_italian_is_reported_and_english_is_not() {
        assert_eq!(
            strings("it('rifiuta le date future', () => {})", Family::Test),
            ["rifiuta le date future"]
        );
        assert!(strings("it('rejects future dates', () => {})", Family::Test).is_empty());
    }

    #[test]
    fn a_script_message_in_italian_is_reported() {
        assert_eq!(
            strings(r#"echo "il file non esiste ancora""#, Family::Gate),
            ["il file non esiste ancora"]
        );
        assert!(strings(r#"echo "the file does not exist yet""#, Family::Gate).is_empty());
    }

    /// Il difetto che la prova mutante trovò nell'originale: con il punto che
    /// attraversa le righe, un commento si mangiava il resto del file e il
    /// controllo taceva su tutto ciò che veniva dopo.
    #[test]
    fn a_comment_does_not_swallow_the_rest_of_the_file() {
        let text = "# un commento italiano qualsiasi\nit('rifiuta le date future', () => {})";
        assert_eq!(strings(text, Family::Test), ["rifiuta le date future"]);
    }

    /// Citare una chiamata dentro un commento non è farla.
    #[test]
    fn a_call_quoted_inside_a_comment_is_not_a_call() {
        let text = "// qui si usa `echo` per dire che il file non esiste ancora\n";
        assert!(strings(text, Family::Gate).is_empty());
    }

    #[test]
    fn it_reports_declared_names_not_used_ones() {
        assert_eq!(
            italian_names("def raccogli_worktree(x):\n    pass\n"),
            ["raccogli_worktree"]
        );
        assert!(italian_names("def collect_worktrees(x):\n    pass\n").is_empty());
        // usarlo non è dichiararlo: la colpa sta su chi lo ha creato
        assert!(italian_names("y = raccogli_worktree(2)\n").is_empty());
        // il camelCase di js e le assegnazioni
        assert_eq!(
            italian_names("const chiudiScheda = () => {}"),
            ["chiudiScheda"]
        );
        assert!(italian_names("nome_atteso = 3\n").iter().any(|n| n == "nome_atteso"));
    }

    /// Un argomento di chiamata non è una dichiarazione: `expect.stringContaining`
    /// non introduce nessun nome, e accusarlo sposterebbe la colpa sul chiamante.
    #[test]
    fn a_call_argument_is_not_a_declaration() {
        let text = "describe('x', () => {\n  it('works', () => {\n    \
                    expect(errors[0]).toMatchObject({\n      line: 3,\n      \
                    message: expect.stringContaining('column'),\n    });\n  })\n})\n";
        assert_eq!(italian_names(text), Vec::<String>::new());
    }

    #[test]
    fn a_short_string_says_nothing_about_the_language() {
        assert!(strings("it('un test', () => {})", Family::Test).is_empty());
        assert!(strings("it(`${x}`, () => {})", Family::Test).is_empty());
    }

    #[test]
    fn an_italian_filename_is_caught_only_when_the_file_is_new() {
        assert!(italian_filename("/x/hooks/raccogli-schede.py"));
        assert!(!italian_filename("/x/hooks/handoff-required.py"));
        assert!(judge("/x/hooks/raccogli-schede.py", "", false).is_some());
        // se il file esiste già, rinominarlo non è compito di chi lo modifica
        assert!(judge("/x/hooks/raccogli-schede.py", "", true).is_none());
    }

    #[test]
    fn the_suffix_is_the_last_extension() {
        assert!(suffix_is("/x/a.test.ts", ".ts"));
        assert!(!suffix_is("/x/a.test.ts", ".test"));
        assert!(!suffix_is("/x/.gitignore", ".gitignore"));
    }
}
