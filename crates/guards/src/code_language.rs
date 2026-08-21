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

/// Gli identificatori **dichiarati** nel testo, senza giudizio sulla lingua.
///
/// Sta separato perché la copertura del vocabolario si misura sul denominatore:
/// quanti nomi il gate vede in tutto, non solo quanti ne segnala. Lo strumento
/// che la misura (`examples/vocabulary.rs`) chiama questa, così legge gli stessi
/// nomi del gancio invece di una copia della regex destinata a divergerne.
pub fn declared_names(text: &str) -> Vec<String> {
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
        if !found.iter().any(|f| f == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// Gli identificatori italiani fra quelli **dichiarati** nel testo nuovo.
///
/// Solo le dichiarazioni, non ogni occorrenza: chi *usa* un nome italiano
/// scritto altrove non lo sta introducendo, e segnalarglielo sposterebbe la
/// colpa sul chiamante invece che su chi lo ha creato.
pub fn italian_names(text: &str) -> Vec<String> {
    declared_names(text)
        .into_iter()
        .filter(|name| is_italian_name(name))
        .collect()
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
         Dai un nome inglese ora, prima che lo citini un gancio o una regola."
    )
}

/// Il rimprovero per una scrittura che il gate non può leggere.
///
/// Non dice «hai scritto in italiano»: dice che **non si sa**, ed è una cosa
/// diversa che va detta con parole diverse. Chiedere di ripassare da
/// `Write`/`Edit` non è un capriccio di forma: lì il testo arriva al gate prima
/// di toccare il disco, che è l'unico momento in cui un rimprovero costa una
/// riscrittura invece di un commit.
///
/// DICEVA «NON LEGGIBILI», E QUELLO ERA FALSO SUL MONDO. Il 21/08/2026 il
/// capitano ha provato a collaudare lo script che toglie a Theo le
/// autorizzazioni a mano e si è visto negare il gesto con questo elenco dentro:
/// un file che esiste, si apre e si legge — `test -r` diceva sì — e che nessuno
/// aveva chiesto di aprire. Chi legge un messaggio così va a cercare un file
/// mancante, non lo trova mancante, e conclude che ha sbagliato lui.
///
/// **Questo gate non apre niente**: guarda il corpo che l'interprete eseguirà e
/// ne raccoglie i percorsi sorvegliati. Ciò che non riesce a leggere non è il
/// file: è **il testo che ci finirà dentro**, che a quel punto non esiste ancora
/// da nessuna parte. Le parole ora lo dicono.
///
/// E L'ELENCO SONO CANDIDATI, NON BERSAGLI. I percorsi si raccolgono dal codice
/// per nome, quindi un file che compare come **dato** — il valore di una chiave
/// JSON, un argomento da passare a qualcun altro — sta nell'elenco accanto a
/// quello che verrà davvero scritto, e i due non si distinguono. Distinguerli
/// vorrebbe dire capire il programma; dirlo costa una riga. È la stessa forma di
/// errore che questa casa conta da sedici occorrenze — un nome citato preso per
/// un nome agito — e finché resta, va dichiarata a chi legge invece che subita.
pub fn report_opaque(targets: &[String]) -> String {
    let lines: Vec<String> = targets
        .iter()
        .take(MAX_ENTRIES)
        .map(|t| format!("    · {t}"))
        .collect();
    format!(
        "Questo comando scrive un file sorvegliato passando da un interprete, e \
         da lì il gate della lingua non vede cosa ci finisce dentro.\n\n\
         Non si legge il TESTO FUTURO, non il file: nessuno di questi è stato \
         aperto, e se qui sotto ne vedi uno che non esiste — o che comincia con \
         il nome di una variabile — è perché il percorso è preso dal codice \
         com'è scritto, senza espandere niente. Sono i nomi sorvegliati che \
         compaiono nel codice, e almeno uno sta per essere scritto: quale, da \
         qui non si sa. Un percorso che compare come dato — un valore JSON, un \
         argomento da passare — è indistinguibile da un bersaglio.\n\n\
         Nominati qui dentro:\n\n{}\n\n\
         Riscrivilo con `Write` o `Edit`: il testo passa dal controllo prima di \
         toccare il disco. Misurato il 19/08/2026: un'intera nottata di codice \
         e' passata da `python3 - <<PY … write_text …` senza che il gate \
         leggesse una riga, e il 19/08 alle 17 sei identificatori italiani sono \
         entrati in `relay.rs` da uno script scritto e lanciato nello stesso \
         comando — la stessa cecità, con un file in mezzo.",
        lines.join("\n")
    )
}

/// Le righe di `new` assenti da `old`, come moltinsieme: una riga ripetuta due
/// volte nel vecchio ne assorbe due uguali nel nuovo, le altre occorrenze
/// restano come nuove. Si confrontano righe intere, non caratteri: un raffronto
/// per carattere spezzerebbe `it("descrizione", …)` a metà, lasciando fuori
/// dalla regex proprio l'`it(` che le serve per riconoscersi — e una riga
/// modificata, anche di un solo carattere, non ha più un gemello nel vecchio,
/// quindi resta.
fn new_lines(old: &str, new: &str) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in old.lines() {
        *counts.entry(line).or_insert(0) += 1;
    }
    new.lines()
        .filter(|line| match counts.get_mut(line) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Il testo appena scritto: su `Edit`/`MultiEdit` solo le righe assenti da
/// `old_string`, su `Write` tutto il contenuto.
///
/// PRIMA leggeva `new_string` senza mai guardare `old_string`: un `Edit` porta
/// per forza il contesto ricopiato attorno alla riga toccata, e bastava una
/// descrizione di test o un identificatore italiano preesistente in quel
/// contesto — non introdotto da questa scrittura — per negarla. Riprodotto il
/// 20/08/2026: `old_string`/`new_string` identici salvo un numero, con una
/// descrizione italiana ricopiata in entrambi, negava. Precedente in casa:
/// `memory_citation_gate`, che nega solo le citazioni *nuove*, non quelle già
/// sul file prima della scrittura.
pub fn written_text(tool_input: &serde_json::Value) -> String {
    if let Some(new) = tool_input.get("new_string").and_then(|v| v.as_str()) {
        let old = tool_input
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return new_lines(old, new);
    }
    if let Some(edits) = tool_input.get("edits").and_then(|v| v.as_array()) {
        return edits
            .iter()
            .map(|e| {
                let new = e.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                let old = e.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                new_lines(old, new)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    tool_input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
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

/// I file sorvegliati che un comando scrive per una via che non sappiamo leggere.
///
/// IL BUCO CHE MI RIGUARDAVA. `writes_from_bash` conosce due forme — `cat >
/// file <<EOF` e `echo … > file` — e in modalità bypass le sessioni non usano
/// nessuna delle due: scrivono con un interprete in linea
/// (`python3 - <<PY … p.write_text(…) … PY`). Misurato il 19/08/2026: tutto il
/// codice di quella notte è passato di lì, e il gate non ha visto una riga.
///
/// Qui non si prova a indovinare **cosa** verrà scritto: dentro un interprete il
/// contenuto può nascere da una variabile, da una lettura, da una sostituzione.
/// Si constata che un file sorvegliato sta per essere scritto da una via opaca,
/// e si dice di passare da `Write`/`Edit`, dove il testo è leggibile prima di
/// finire sul disco. Un controllo che non può guardare deve dirlo, non tacere.
///
/// Servono tutti e tre gli indizi — interprete in linea, un gesto di scrittura,
/// un percorso sorvegliato — perché due su tre prendono anche chi legge un file
/// con `python3 -c`, e un gate che rimprovera a torto viene spento.
///
/// E SI GUARDA SOLO DENTRO L'INTERPRETE, non in tutto il comando. Al primo giro
/// si cercavano gli indizi ovunque, e il gate ha negato il `git commit` che
/// *descriveva* questa correzione: il messaggio citava
/// `python3 - <<PY … write_text …` e i percorsi dei file toccati. Un comando che
/// parla di una scrittura non è una scrittura, e la differenza sta nel dove:
/// ciò che conta è il corpo che l'interprete eseguirà.
///
/// IL BUCO CHE RESTA, per scelta: `curl … | python3 -`, una sostituzione di
/// comando, o qualunque pipe che non nomini un file leggibile. Lì non c'è
/// niente sul disco né nel comando da leggere — non solo il contenuto, anche
/// l'esistenza di un bersaglio è ignota — e negare sul solo pattern «pipe verso
/// un interprete» prenderebbe anche `curl https://sh.rustup.rs | sh`. Si tace,
/// non si nega: un buco dichiarato costa meno di un rimprovero a torto.
pub fn opaque_writes(command: &str) -> Vec<String> {
    if command.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<String> = Vec::new();
    // `require_parent`: vero solo per il corpo di uno script già sul disco —
    // lì un candidato può essere un frammento di stringa (vedi `parent_exists`).
    // L'interprete in linea e lo script creato-e-lanciato nello stesso comando
    // restano senza filtro: il file che scrivono non esiste ancora per
    // definizione, e provarne il genitore prenderebbe anche i casi veri.
    let mut collect = |body: &str, require_parent: bool| {
        if !writes_a_file().is_match(body) {
            return;
        }
        // I BERSAGLI SI CERCANO NEL CODICE, NON NEI COMMENTI. Un programma che
        // *nomina* un altro programma nella sua descrizione veniva letto come un
        // programma che lo *scrive*, e il gate negava di leggerne l'aiuto:
        // riprodotto il 21/08/2026 su `close-finished-worktrees.py`, la cui
        // prima riga di documentazione dice di non duplicare un file omonimo. È
        // la stessa forma di errore di `comment-refs` in senso opposto — un
        // percorso citato scambiato per un percorso agito — e qui pesa il doppio
        // perché a essere impedita è una lettura.
        let code = code_without_comments(body);
        for c in quoted_path().captures_iter(&code) {
            let path = c.get(1).map(|x| x.as_str()).unwrap_or("");
            if require_parent && !parent_exists(path) {
                continue;
            }
            if family(path).is_some() && !found.iter().any(|f| f == path) {
                found.push(path.to_string());
            }
        }
    };
    for m in inline_interpreter().find_iter(command) {
        collect(&command[m.end()..], false);
    }
    for (body, from_disk) in script_bodies(command) {
        collect(&body, from_disk);
    }
    found
}

/// Il corpo senza commenti e senza documentazione, con le stringhe intatte.
///
/// Serve a una domanda sola: quali percorsi questo programma **tocca**. Un
/// percorso che compare in un commento o in una docstring è nominato, non
/// agito, e contarlo fa negare comandi che non scrivono niente.
///
/// LE STRINGHE RESTANO, ed è la metà che conta: un bersaglio di scrittura vive
/// quasi sempre dentro una stringa (`open("/x/a.ts", "w")`), e un `#` o un `//`
/// dentro quella stringa — un frammento di URL, un colore, uno shebang citato —
/// taglierebbe via proprio la riga che si vuole leggere. Per questo una stringa
/// si attraversa copiandola invece di saltarla.
///
/// IL BUCO CHE RESTA, dichiarato: una stringa aperta e mai chiusa sulla stessa
/// riga smette di essere trattata come tale a fine riga. Sbaglia tenendo il
/// testo, non buttandolo, e quindi al più fa cercare un percorso in più.
fn code_without_comments(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    while i < body.len() {
        let rest = &body[i..];
        // La documentazione di Python, che è una stringa tripla e può coprire
        // decine di righe: è il caso che ha prodotto il falso allarme.
        if let Some(delim) = ["\"\"\"", "'''"].iter().find(|d| rest.starts_with(**d)) {
            i += match rest[3..].find(*delim) {
                Some(end) => 3 + end + 3,
                None => rest.len(),
            };
            continue;
        }
        if rest.starts_with("/*") {
            i += match rest[2..].find("*/") {
                Some(end) => 2 + end + 2,
                None => rest.len(),
            };
            continue;
        }
        if rest.starts_with('#') || rest.starts_with("//") {
            // L'a capo si tiene: senza, due righe si saldano e nascono percorsi
            // che nel file non ci sono.
            i += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with('"') || rest.starts_with('\'') {
            let quote = rest.as_bytes()[0];
            out.push(quote as char);
            let mut j = 1;
            let mut escaped = false;
            while j < rest.len() {
                let b = rest.as_bytes()[j];
                if b == b'\n' {
                    break; // stringa non chiusa: si torna al testo normale
                }
                let ch = rest[j..].chars().next().unwrap_or('\n');
                out.push(ch);
                j += ch.len_utf8();
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == quote {
                    break;
                }
            }
            i += j;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\n');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Vero se la cartella che contiene `path` esiste sul disco.
///
/// Un frammento di stringa dentro il sorgente di uno script — un pezzo di
/// percorso senza `$HOME`, o dietro una variabile che la regex non espande
/// (`~/`, `$VAR/`, `${VAR}/`, tagliata al primo carattere non ammesso) — non
/// ha una cartella vera dietro, e non è un file che sta per nascere. Riprodotto
/// il 20/08/2026: `python3 sessions.py --test` veniva negato perché nel
/// sorgente compare `/.claude/scripts/nome.sh`, avanzo di
/// `~/.claude/scripts/nome.sh` dopo che la regex ha scartato la tilde. Chi non
/// è assoluto non ha un genitore verificabile: si tiene, un buco noto è meglio
/// di un falso allarme.
fn parent_exists(path: &str) -> bool {
    let resolved = match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => return true,
        },
        None => path.to_string(),
    };
    if !resolved.starts_with('/') {
        return true;
    }
    std::path::Path::new(&resolved)
        .parent()
        .map(|p| p.exists())
        .unwrap_or(true)
}

/// I corpi degli script che questo comando manda a un interprete.
///
/// IL BUCO CHE RESTAVA DOPO AVER CHIUSO L'INTERPRETE IN LINEA, e che il
/// 19/08/2026 alle 17 ha lasciato entrare in `relay.rs` sei identificatori
/// italiani: fra `python3 - <<PY` e `python3 script.py` cambia solo dove vive il
/// corpo, e il gate guardava soltanto il primo. Il rimprovero arrivava a lavoro
/// finito, da un'interrogazione fatta a mano.
///
/// Due strade, perché il corpo può non essere ancora sul disco: quando lo
/// **stesso** comando scrive lo script con un heredoc e poi lo lancia — la forma
/// più comune, ed è quella misurata — il testo sta lì davanti, e il file non
/// esiste finché il comando non parte. Altrimenti lo script c'è già e si legge.
///
/// Il tetto di lettura è la cautela solita: un percorso qualunque preso da una
/// riga di comando può essere un file da centinaia di MB, e un gate che si
/// ingoia un log non risponde più.
///
/// Il booleano dice se il corpo viene da un file già sul disco: solo lì un
/// candidato dentro il corpo può essere un frammento di stringa invece di un
/// file vero (`opaque_writes` applica `parent_exists` solo in quel caso — un
/// heredoc nello stesso comando scrive un file che non esiste ancora per
/// definizione, e lì il filtro prenderebbe anche i casi veri).
fn script_bodies(command: &str) -> Vec<(String, bool)> {
    const MAX_SCRIPT_BYTES: u64 = 256 * 1024;
    let mut bodies = Vec::new();
    for c in interpreter_with_script().captures_iter(command) {
        let Some(path) = c.get(1).map(|m| m.as_str()) else {
            continue;
        };
        if let Some(body) = heredoc_body_for(command, path) {
            bodies.push((body, false));
            continue;
        }
        if let Some(text) = read_script_body(path, MAX_SCRIPT_BYTES) {
            bodies.push((text, true));
        }
    }
    // IL BUCO VERIFICATO IL 20/08/2026: `python3 script.py` negava, `cat
    // script.py | python3 -` passava intatto — la pipe bastava ad aggirare il
    // divieto per intero. `cat` e la redirezione leggono un file che deve
    // esistere per forza, quindi valgono la stessa lettura e la stessa
    // protezione (`require_parent`) dello script dato come argomento: non si
    // allarga il perimetro, si aggiungono due porte allo stesso corridoio.
    for c in cat_piped_to_interpreter()
        .captures_iter(command)
        .chain(redirected_to_interpreter().captures_iter(command))
    {
        let Some(path) = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()) else {
            continue;
        };
        if let Some(text) = read_script_body(path, MAX_SCRIPT_BYTES) {
            bodies.push((text, true));
        }
    }
    bodies
}

/// Risolve `~` e legge un file già sul disco, sotto lo stesso tetto di byte
/// per tutte le vie che portano un corpo a un interprete: un percorso preso da
/// una riga di comando può essere un log di centinaia di MB, e un gate che se
/// lo ingoia non risponde più.
fn read_script_body(path: &str, max_bytes: u64) -> Option<String> {
    let resolved = match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => path.to_string(),
        },
        None => path.to_string(),
    };
    let piccolo = std::fs::metadata(&resolved)
        .map(|m| m.len() <= max_bytes)
        .unwrap_or(false);
    if piccolo {
        std::fs::read_to_string(&resolved).ok()
    } else {
        None
    }
}

/// `cat script.py | python3 -` e `cat script.py | python3`: lo stesso corpo di
/// `python3 script.py`, con una pipe in mezzo. `cat` legge un file che deve
/// esistere sul disco per forza, quindi il bersaglio è leggibile quanto lo
/// script dato come argomento — non si prova a leggere ciò che sta a sinistra
/// di una pipe qualunque, solo questa forma nominata.
fn cat_piped_to_interpreter() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bcat\s+([^\s|]+)\s*\|\s*(?:python3?|node|bash|sh|zsh|ruby|perl)\b(?:\s*-)?")
            .unwrap()
    })
}

/// `< script.py python3` e `python3 < script.py`: il corpo arriva per
/// redirezione invece che per argomento o per pipe — la terza forma con cui
/// uno script già sul disco finisce dentro un interprete senza passare da
/// `Write`/`Edit`. Un `<` fra l'interprete e un altro argomento (`python3
/// script.py < input.txt`) non ha questa forma: lì lo script arriva come
/// argomento, e la redirezione riguarda l'input dello *script*, non
/// dell'interprete — la regex lo esclude perché richiede `<` subito dopo il
/// nome dell'interprete (o subito dopo un `-` solitario), senza un percorso
/// in mezzo.
///
/// IL BUCO VERIFICATO IL 20/08/2026: `python3 - < script.py` passava intatto
/// — il `-` fra l'interprete e `<` è la forma con cui si dice esplicitamente
/// «leggi da standard input», e prima bastava a rompere il confronto
/// letterale con l'interprete. Coperto insieme a `python3 < script.py`,
/// `node < script.js`, `bash < script.sh`, con spaziature diverse intorno a
/// `<`. NON coperto per scelta: `<<` e `<<<` sono un'altra sintassi (il primo
/// `<` non può essere seguito da un altro `<` dentro `[^\s<>|;&]+`, quindi
/// `(?:-\s*)?<` da solo li esclude), e una redirezione in uscita (`>`, `>>`)
/// è un'altra superficie, con un'altra decisione dietro.
fn redirected_to_interpreter() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"<\s*([^\s<>|;&]+)\s+(?:python3?|node|bash|sh|zsh|ruby|perl)\b",
            r"|\b(?:python3?|node|bash|sh|zsh|ruby|perl)\s*(?:-\s*)?<\s*([^\s<>|;&]+)",
        ))
        .unwrap()
    })
}

/// Il corpo dell'heredoc che, dentro questo stesso comando, crea quel file.
///
/// Si confronta la coda del percorso e non la stringa intera: `cat > /tmp/x.py`
/// e `python3 /tmp/x.py` combaciano, ma anche `cat > ./x.py` con `python3 x.py`,
/// che è la stessa cosa scritta due volte in modo diverso.
fn heredoc_body_for(command: &str, script: &str) -> Option<String> {
    let nome = script.rsplit('/').next().unwrap_or(script);
    for m in heredoc().captures_iter(command) {
        let target = m.get(1)?.as_str();
        if target.rsplit('/').next().unwrap_or(target) != nome {
            continue;
        }
        let delimiter = m.get(2)?.as_str().trim_matches(|c| c == '\'' || c == '"');
        if let Some(body) = heredoc_body(command, delimiter, m.get(0)?.end()) {
            return Some(body);
        }
    }
    None
}

/// `python3 script.py`, `bash script.sh`: l'interprete prende un file, e il
/// corpo che conta sta dentro quel file invece che nel comando.
fn interpreter_with_script() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:python3?|node|perl|ruby|bash|sh|zsh)\s+([A-Za-z0-9_./~-]+\.(?:py|js|mjs|cjs|pl|rb|sh|bash))\b",
        )
        .unwrap()
    })
}

/// `python3 - <<PY`, `node -e '…'`, `perl -e`: il corpo arriva dallo stesso
/// comando invece che da un file.
fn inline_interpreter() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:python3?|node|perl|ruby|osascript)\s+(?:-\s*<<|-[ec]\b|<<)").unwrap()
    })
}

/// Un gesto che mette testo su disco, nelle lingue che l'interprete può parlare.
fn writes_a_file() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // `open(` da solo prende anche chi **legge**: provato il 19/08/2026,
        // `python3 -c "print(open('x.py').read())"` finiva negato. La modalità
        // è l'unica cosa che distingue le due intenzioni, e va richiesta.
        Regex::new(
            r#"(?:write_text|writeFileSync|writeFile|\.write\s*\(|fs::write|open\s*\([^)]{0,200}['"][wax]\+?b?['"])"#,
        )
        .unwrap()
    })
}

/// Un percorso con un'estensione sorvegliata, ovunque nel comando.
///
/// Non si cercano gli apici: dentro un heredoc il testo attraversa due livelli
/// di citazione e le virgolette arrivano già mangiate. L'estensione basta a
/// proporre un candidato, e `family` decide poi se è sorvegliato davvero — la
/// fonte è lei, qui serve solo qualcosa da sottoporle.
fn quoted_path() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"([A-Za-z0-9_./-]+\.(?:rs|py|ts|tsx|js|mjs|cjs|sh|bash|yml|yaml))").unwrap()
    })
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
    fn a_write_through_an_inline_interpreter_is_flagged_as_unreadable() {
        // La via da cui è passato tutto il codice del 19/08/2026 senza che il
        // gate leggesse una riga: il contenuto nasce dentro l'interprete, e da
        // fuori non si vede.
        let command = "python3 - <<PY\np = pathlib.Path('/x/crates/a.rs')\n\
                       p.write_text(body)\nPY";
        assert_eq!(opaque_writes(command), ["/x/crates/a.rs"]);
    }

    #[test]
    fn a_script_written_and_run_in_one_command_is_not_readable() {
        // LA FORMA CHE HA ATTRAVERSATO IL GATE il 19/08/2026: lo script nasce da
        // un heredoc e parte nello stesso comando, quindi sul disco non c'è
        // ancora niente da leggere — il corpo però sta lì, due righe più su.
        let command = "cat > /tmp/patch.py <<'PY'\n\
                       import io\n\
                       io.open('/x/crates/a.rs', 'w').write(s)\n\
                       PY\n\
                       python3 /tmp/patch.py";
        assert_eq!(opaque_writes(command), ["/x/crates/a.rs"]);
    }

    #[test]
    fn a_script_already_on_disk_is_read_before_it_runs() {
        let dir = hook_io::testing::test_dir("gate-script-su-disco");
        // Il genitore del bersaglio deve esistere davvero, e stare sotto
        // `crates/` perché `family` lo riconosca — `parent_exists` scarterebbe
        // un percorso fittizio come un frammento di stringa.
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive.py");
        let target = crates_dir.join("a.rs");
        std::fs::write(
            &script,
            format!("import pathlib\npathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        let command = format!("python3 {}", script.display());
        assert_eq!(opaque_writes(&command), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn a_script_that_only_reads_is_left_alone() {
        // Il gate che rimprovera a torto viene spento: servono tutti e tre gli
        // indizi, e leggere non è scrivere.
        let command = "cat > /tmp/legge.py <<'PY'\n\
                       print(open('/x/crates/a.rs').read())\n\
                       PY\n\
                       python3 /tmp/legge.py";
        assert!(opaque_writes(command).is_empty());
    }

    #[test]
    fn naming_a_script_is_not_running_one() {
        // Un comando che *parla* di uno script non è quello script: è lo stesso
        // errore per cui il gate negò il `git commit` che descriveva la propria
        // correzione.
        assert!(opaque_writes("git add tools/patch.py && git commit -m 'x'").is_empty());
        assert!(opaque_writes("ls -l /tmp/patch.py").is_empty());
    }

    #[test]
    fn a_script_that_writes_something_unwatched_passes() {
        let command = "cat > /tmp/patch.py <<'PY'\n\
                       io.open('/tmp/appunti.txt', 'w').write(s)\n\
                       PY\n\
                       python3 /tmp/patch.py";
        assert!(opaque_writes(command).is_empty());
    }

    #[test]
    fn reading_a_watched_file_is_not_writing_it() {
        // Il falso positivo trovato al primo colpo: `open(` da solo prende
        // anche chi legge, e un gate che rimprovera a torto viene spento.
        let command = "python3 -c \"print(open('/x/scripts/a.py').read())\"";
        assert!(opaque_writes(command).is_empty());
    }

    #[test]
    fn talking_about_a_write_is_not_writing() {
        // Il caso vero: il gate ha negato il `git commit` che descriveva questa
        // stessa correzione, perché il messaggio citava la forma incriminata e
        // i percorsi dei file toccati. Gli indizi si cercano solo nel corpo che
        // l'interprete eseguirà.
        let command = "git commit -F - <<'MSG'\nThe hole: python3 - <<PY with \
                       write_text on crates/guards/src/a.rs went unseen.\nMSG";
        assert!(opaque_writes(command).is_empty());
    }

    #[test]
    fn an_opaque_write_to_an_unwatched_file_is_nobodys_business() {
        let command = "python3 - <<PY\npathlib.Path('/tmp/note.txt').write_text(x)\nPY";
        assert!(opaque_writes(command).is_empty());
    }

    #[test]
    fn a_readable_write_is_left_to_the_reader() {
        // `cat > file <<EOF` si legge: lo giudica `writes_from_bash`, e
        // segnalarlo anche qui darebbe due rimproveri per un solo gesto.
        let command = "cat > /x/crates/a.rs <<'EOF'\nfn collect() {}\nEOF";
        assert!(opaque_writes(command).is_empty());
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

    /// FALSO POSITIVO 1, riprodotto il 20/08/2026: `old_string`/`new_string`
    /// identici salvo `toBe(1)` → `toBe(2)`, con la descrizione italiana
    /// ricopiata in entrambi. Prima negava una scrittura che non la introduceva.
    #[test]
    fn an_edit_that_only_copies_context_is_not_reported() {
        let tool_input = serde_json::json!({
            "old_string": "it('restituisce il numero normalizzato', () => {\n  expect(f(1)).toBe(1);\n});",
            "new_string": "it('restituisce il numero normalizzato', () => {\n  expect(f(1)).toBe(2);\n});",
        });
        assert!(strings(&written_text(&tool_input), Family::Test).is_empty());
    }

    /// Il caso vero che non deve smettere di funzionare: chi modifica la riga
    /// italiana viene fermato lo stesso, perché quella riga non ha più un
    /// gemello identico nel testo vecchio.
    #[test]
    fn an_edit_that_changes_the_italian_line_is_still_caught() {
        let tool_input = serde_json::json!({
            "old_string": "it('rifiuta le date future', () => {})",
            "new_string": "it('rifiuta le date passate', () => {})",
        });
        assert_eq!(
            strings(&written_text(&tool_input), Family::Test),
            ["rifiuta le date passate"]
        );
    }

    /// `MultiEdit`: ogni coppia si filtra sul proprio `old_string`, non su
    /// quello delle altre.
    #[test]
    fn a_multi_edit_filters_each_pair_on_its_own_old_string() {
        let tool_input = serde_json::json!({
            "edits": [
                {"old_string": "it('rifiuta le date future', () => {})",
                 "new_string": "it('rifiuta le date future', () => {})"},
                {"old_string": "it('accetta input', () => {})",
                 "new_string": "it('accetta un valore nuovo', () => {})"},
            ]
        });
        assert_eq!(
            strings(&written_text(&tool_input), Family::Test),
            ["accetta un valore nuovo"]
        );
    }

    /// `Write` non ha `old_string`: tutto il contenuto resta giudicato, come
    /// prima — non cambia niente per questo strumento.
    #[test]
    fn a_write_still_judges_the_whole_content() {
        let tool_input =
            serde_json::json!({"content": "it('rifiuta le date future', () => {})"});
        assert_eq!(
            strings(&written_text(&tool_input), Family::Test),
            ["rifiuta le date future"]
        );
    }

    /// FALSO POSITIVO 2, riprodotto il 20/08/2026 su `sessions.py`: la regex
    /// scarta la tilde e lascia `/.claude/scripts/nome.sh`, un frammento senza
    /// `$HOME` davanti. La sua cartella non esiste: non è un file che sta per
    /// nascere, ed elencarlo nel rimprovero è anche incomprensibile a chi lo
    /// riceve — il comando non lo tocca affatto.
    #[test]
    fn a_path_fragment_without_a_real_parent_is_not_a_target() {
        let dir = hook_io::testing::test_dir("gate-frammento-percorso");
        let _ = std::fs::create_dir_all(&dir);
        let script = dir.join("misura.py");
        std::fs::write(
            &script,
            "print('to unmount: bash ~/.claude/scripts/smonta-finite-2026-08-18.sh')\n\
             (STATE / 'x').write_text('y')\n",
        )
        .unwrap();
        let command = format!("python3 {}", script.display());
        assert!(opaque_writes(&command).is_empty());
        let _ = std::fs::remove_file(&script);
    }

    /// IL BUCO VERIFICATO IL 20/08/2026: `python3 script.py` negava,
    /// `cat script.py | python3 -` passava con `exit=0`. `cat` legge un file
    /// che deve esistere per forza: vale la stessa lettura dello script dato
    /// come argomento, con o senza il `-` finale.
    #[test]
    fn a_script_piped_through_cat_is_read_like_one_given_as_an_argument() {
        let dir = hook_io::testing::test_dir("gate-script-per-pipe");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive3.py");
        let target = crates_dir.join("a.rs");
        std::fs::write(
            &script,
            format!("import pathlib\npathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        let with_dash = format!("cat {} | python3 -", script.display());
        assert_eq!(opaque_writes(&with_dash), [target.display().to_string()]);
        let without_dash = format!("cat {} | python3", script.display());
        assert_eq!(opaque_writes(&without_dash), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    /// La stessa forma con `node`, l'altro interprete del mandato: prova che
    /// l'elenco degli interpreti dentro la regex, non solo `python3`, sia
    /// coperto — con `writeFileSync`, il gesto che `writes_a_file` riconosce.
    #[test]
    fn a_node_script_piped_through_cat_is_read_too() {
        let dir = hook_io::testing::test_dir("gate-script-node-per-pipe");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive.js");
        let target = crates_dir.join("a.rs");
        std::fs::write(
            &script,
            format!("fs.writeFileSync('{}', s)\n", target.display()),
        )
        .unwrap();
        let command = format!("cat {} | node -", script.display());
        assert_eq!(opaque_writes(&command), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn a_script_redirected_into_an_interpreter_is_read_too() {
        let dir = hook_io::testing::test_dir("gate-script-per-redirezione");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive4.py");
        let target = crates_dir.join("a.rs");
        std::fs::write(
            &script,
            format!("import pathlib\npathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        let forward = format!("python3 < {}", script.display());
        assert_eq!(opaque_writes(&forward), [target.display().to_string()]);
        let backward = format!("< {} python3", script.display());
        assert_eq!(opaque_writes(&backward), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    /// IL BUCO CHE HA MOTIVATO QUESTA CORREZIONE, riprodotto il 20/08/2026:
    /// `python3 - < script.py` passava intatto mentre `python3 script.py` e
    /// `cat script.py | python3 -` erano già negati — lo stesso corpo, una
    /// terza porta. Il `-` è la forma esplicita per «leggi da standard input»,
    /// tanto per `python3` quanto per `node`/`bash`.
    #[test]
    fn a_dash_before_the_redirection_does_not_hide_the_script() {
        let dir = hook_io::testing::test_dir("gate-script-per-redirezione-trattino");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive5.py");
        let target = crates_dir.join("a.rs");
        std::fs::write(
            &script,
            format!("import pathlib\npathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        let with_dash = format!("python3 - < {}", script.display());
        assert_eq!(opaque_writes(&with_dash), [target.display().to_string()]);
        // spaziature diverse intorno a `<`: nessuna, doppia, senza spazio dopo il trattino
        let tight = format!("python3 -<{}", script.display());
        assert_eq!(opaque_writes(&tight), [target.display().to_string()]);
        let loose = format!("python3 -  <  {}", script.display());
        assert_eq!(opaque_writes(&loose), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    /// Differenziale: due comandi che differiscono solo per `<<` invece di `<`
    /// devono dare esiti opposti. `python3 - <<PY` è l'interprete in linea, già
    /// coperto da `inline_interpreter`; la regex della redirezione da file non
    /// deve confondersi e agganciare lo stesso comando una seconda volta con un
    /// bersaglio inventato.
    #[test]
    fn a_double_redirection_is_not_mistaken_for_a_single_one() {
        let single = "python3 - < /tmp/scrive.py";
        let double = "python3 - <<PY\npathlib.Path('/x/crates/a.rs').write_text(x)\nPY";
        assert!(redirected_to_interpreter().captures_iter(single).next().is_some());
        assert!(redirected_to_interpreter().captures_iter(double).next().is_none());
    }

    /// Un flag come `-c` non è il trattino solitario che introduce la
    /// redirezione da standard input: `-c` è seguito dal codice, non da `<`.
    #[test]
    fn a_flag_that_is_not_a_lone_dash_does_not_match() {
        let command = "python3 -c 'print(1)'";
        assert!(redirected_to_interpreter().captures_iter(command).next().is_none());
    }

    /// Non deve generalizzare «ogni pipe dopo `cat` è sospetta»: `grep` non è
    /// un interprete, e questa forma è la più comune di tutte.
    #[test]
    fn an_innocuous_pipe_through_cat_is_left_alone() {
        // Un file vero, non un percorso inventato: se `grep` finisse per
        // sbaglio nell'elenco degli interpreti, il file esisterebbe comunque
        // e la prova lo scoprirebbe — un percorso assente lo nasconderebbe.
        let dir = hook_io::testing::test_dir("gate-pipe-innocua");
        let _ = std::fs::create_dir_all(&dir);
        let log = dir.join("qualcosa.log");
        std::fs::write(&log, "errore: connessione rifiutata\n").unwrap();
        let command = format!("cat {} | grep errore", log.display());
        assert!(cat_piped_to_interpreter().captures_iter(&command).next().is_none());
        assert!(opaque_writes(&command).is_empty());
        let _ = std::fs::remove_file(&log);
    }

    /// Non deve prendere lo script dato come argomento quando la redirezione
    /// riguarda l'*input dello script*, non il corpo dell'interprete: qui
    /// `python3 script.py` è già letto da `interpreter_with_script`, e
    /// `< input.txt` non deve aggiungere un secondo bersaglio fantasma.
    #[test]
    fn a_redirection_after_the_script_argument_is_not_a_second_source() {
        let command = "python3 /tmp/analizza.py < /tmp/dati.csv";
        assert!(redirected_to_interpreter().captures_iter(command).next().is_none());
    }

    /// Deciso il 20/08/2026: il corpo che arriva da `curl` o da una
    /// sostituzione di comando non sta né sul disco né nel comando — non c'è
    /// niente da leggere, e negare sul solo pattern «pipe verso un
    /// interprete» prenderebbe anche `curl https://sh.rustup.rs | sh`. Si
    /// tace: un buco dichiarato costa meno di un rimprovero a torto.
    #[test]
    fn a_body_arriving_from_an_unreadable_source_is_left_alone() {
        // Verifica sulla regex, non solo sull'esito finale: un file inesistente
        // renderebbe `opaque_writes` vuoto comunque, mascherando una regex
        // allargata per sbaglio a «qualunque cosa prima di una pipe».
        let command = "curl https://example.com/setup.py | python3 -";
        assert!(cat_piped_to_interpreter().captures_iter(command).next().is_none());
        assert!(opaque_writes(command).is_empty());
        assert!(opaque_writes("python3 -c \"$(cat /tmp/a.py)\"").is_empty());
    }

    /// Non allenta il caso vero: se il bersaglio ha una cartella reale dietro
    /// — anche se il file stesso è nuovo — il gate nega ancora.
    #[test]
    fn a_real_target_with_an_existing_parent_is_still_caught() {
        let dir = hook_io::testing::test_dir("gate-bersaglio-vero");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("scrive2.py");
        let target = crates_dir.join("nuovo.rs");
        std::fs::write(
            &script,
            format!("pathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        let command = format!("python3 {}", script.display());
        assert_eq!(opaque_writes(&command), [target.display().to_string()]);
        let _ = std::fs::remove_file(&script);
    }

    /// IL CASO RIPRODOTTO IL 21/08/2026: `python3 <script> --help` negato perché
    /// la documentazione dello script **nomina** un altro programma. Chiedere
    /// l'aiuto di un comando è una lettura, e un controllo che impedisce di
    /// guardare costa più del difetto che sorveglia.
    ///
    /// DIFFERENZIALE — i due bracci cambiano solo per la riga di codice che
    /// scrive. Il primo pretende il silenzio, il secondo pretende il bersaglio
    /// vero: togliere le stringhe insieme ai commenti farebbe cadere il secondo,
    /// togliere niente farebbe cadere il primo. Nessuna delle due mutazioni
    /// passa.
    #[test]
    fn a_path_named_in_the_documentation_is_not_a_path_written() {
        let dir = hook_io::testing::test_dir("gate-percorso-nominato");
        let crates_dir = dir.join("crates");
        let _ = std::fs::create_dir_all(&crates_dir);
        let script = dir.join("nomina.py");
        let target = crates_dir.join("a.rs");
        let doc = "\"\"\"Chiude le copie finite.\n\n\
                   NON DUPLICA gyver/work/.claude/scripts/close-finished.py, e il\n\
                   confine e' netto: quello sceglie da solo chi smontare.\n\"\"\"\n\
                   # vedi anche tools/altro.py\n";

        std::fs::write(&script, format!("{doc}import shutil\nshutil.copy2(a, b)\n")).unwrap();
        let command = format!("python3 {} --help", script.display());
        assert!(
            opaque_writes(&command).is_empty(),
            "un percorso citato nella documentazione non è un bersaglio: {:?}",
            opaque_writes(&command)
        );

        std::fs::write(
            &script,
            format!("{doc}pathlib.Path('{}').write_text(s)\n", target.display()),
        )
        .unwrap();
        assert_eq!(
            opaque_writes(&command),
            [target.display().to_string()],
            "il bersaglio scritto in codice si vede ancora, e i due citati no"
        );
        let _ = std::fs::remove_file(&script);
    }

    /// Il rimprovero diceva «Non leggibili» di file che si aprono benissimo, e
    /// il 21/08/2026 ha mandato il capitano a cercare un file mancante che non
    /// mancava. Il gate non apre niente: quello che non legge è il testo futuro.
    ///
    /// MUTANTE: rimettere la parola «Non leggibili» al posto di «Nominati qui
    /// dentro» fa cadere questo caso e nient'altro.
    #[test]
    fn the_opaque_report_does_not_claim_the_files_cannot_be_opened() {
        let m = report_opaque(&["/Users/x/.claude/scripts/a.sh".to_string()]);
        assert!(m.contains("/Users/x/.claude/scripts/a.sh"), "{m}");
        assert!(
            !m.contains("Non leggibili"),
            "il file si apre: il messaggio non deve dire il contrario:\n{m}"
        );
        assert!(
            m.contains("TESTO FUTURO"),
            "deve dire cosa non riesce a leggere davvero:\n{m}"
        );
        // E che l'elenco sono nomi raccolti dal codice, non bersagli accertati:
        // un percorso che compare come dato ci finisce accanto a quello vero.
        assert!(m.contains("dato"), "{m}");
    }

    #[test]
    fn a_comment_marker_inside_a_string_does_not_cut_the_line() {
        // Il rischio della pulizia: `#` e `//` vivono anche dentro le stringhe —
        // uno shebang citato, un frammento di URL, un colore. Se tagliassero lì,
        // sparirebbe proprio la riga che porta il bersaglio.
        let body = "header = \"#!/bin/sh # x\"\nopen('/x/crates/a.rs', 'w')\n";
        let code = code_without_comments(body);
        assert!(code.contains("/x/crates/a.rs"), "{code}");
        assert!(code.contains("#!/bin/sh"), "{code}");
    }

    #[test]
    fn comments_and_docstrings_leave_the_code_behind() {
        let body = "\"\"\"doc con a/b/uno.py\"\"\"\n\
                    x = 1  # coda con due.py\n\
                    /* blocco con tre.py */\n\
                    // riga con quattro.py\n\
                    open('cinque.py', 'w')\n";
        let code = code_without_comments(body);
        for named in ["uno.py", "due.py", "tre.py", "quattro.py"] {
            assert!(!code.contains(named), "{named} doveva sparire:\n{code}");
        }
        assert!(code.contains("cinque.py"), "{code}");
        // Le righe non si saldano fra loro: senza l'a capo, il codice rimasto a
        // sinistra di un commento e la riga dopo genererebbero un percorso che
        // nel file non esiste.
        assert!(code.lines().any(|l| l.trim() == "x = 1"), "{code}");
    }
}
