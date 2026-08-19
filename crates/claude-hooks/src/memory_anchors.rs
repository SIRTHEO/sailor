//! Il braccio dell'ancoraggio: dove stanno le memorie, dove sta il codice, e
//! chi scrive il timbro.
//!
//! La logica — impronta, estrazione del simbolo, verdetto — sta in
//! `guards::memory_anchor`, che non tocca il disco. Qui c'è solo il mondo:
//! cartelle, risoluzione dei percorsi, lettura, scrittura, uscite.
//!
//! I parenti già in casa, cercati prima di scrivere: `live-rules` verifica che i
//! rimandi **dentro le regole** puntino a file esistenti, e `comment-refs` vieta
//! i rimandi a documenti nei commenti. Tutti e due si fermano all'esistenza del
//! file. Qui la domanda è un'altra — «il codice sotto è ancora quello?» — e
//! nessuno dei due la sa porre.
//!
//! Uso:
//!     claude-hooks memory-anchors                 verifica tutti gli ancoraggi
//!     claude-hooks memory-anchors --json          lo stesso, da dare in pasto
//!     claude-hooks memory-anchors --suggest       cosa si potrebbe ancorare
//!     claude-hooks memory-anchors --suggest --write   e lo scrive
//!     claude-hooks memory-anchors --restamp       riallinea le derive già viste
//!     claude-hooks memory-anchors --dead          i file citati che non esistono
//!     claude-hooks memory-anchors --file <path>   una memoria sola
//!
//! Uscita: 0 quando nessun ancoraggio è in allarme, 1 quando almeno uno lo è.
//! Serve per metterlo in un servizio senza leggere l'uscita a occhio.
//!
//! `--restamp` è separato di proposito: aggiornare l'impronta **spegne**
//! l'allarme, e spegnere un allarme è una decisione, non un effetto collaterale
//! della verifica.

use guards::memory_anchor::{
    anchors_in, citations, declaration_line, extract_symbol, fingerprint, identifiers, judge,
    normalise, symbol_at_line, with_anchors, Anchor, Citation, Lang, Verdict,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Le cartelle di memorie da percorrere.
///
/// `MEMORY_ANCHOR_DIR` (più cartelle separate da `:`) le sostituisce: è la
/// valvola che rende provabile lo strumento senza toccare le memorie vere.
fn memory_dirs() -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("MEMORY_ANCHOR_DIR") {
        return std::env::split_paths(&v).collect();
    }
    let projects = home().join(".claude/projects");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&projects) {
        for e in entries.flatten() {
            let dir = e.path().join("memory");
            if dir.is_dir() {
                out.push(dir);
            }
        }
    }
    out.sort();
    out
}

/// Le radici sotto cui si cerca un percorso relativo citato da una memoria.
///
/// Una memoria scrive `relay.rs:1050` o `crates/guards/src/chain.rs`, non il
/// percorso assoluto: senza radici quei riferimenti resterebbero tutti «file
/// mancante», che è la risposta sbagliata al 43% delle citazioni misurate.
fn roots() -> Vec<PathBuf> {
    if let Some(v) = test_roots() {
        return v;
    }
    if let Some(v) = std::env::var_os("MEMORY_ANCHOR_ROOTS") {
        return std::env::split_paths(&v).collect();
    }
    let h = home();
    let mut out = vec![
        h.join(".claude"),
        h.join(".claude/rust"),
        h.join(".claude/scripts"),
        h.join("orca/general"),
        h.join("gyver/work"),
    ];
    // I repo sotto l'ombrello: `suite/src/...` si risolve dall'ombrello, ma
    // `src/...` no, e le memorie scrivono in tutti e due i modi.
    if let Ok(entries) = std::fs::read_dir(h.join("gyver/work")) {
        for e in entries.flatten() {
            if e.path().join(".git").exists() {
                out.push(e.path());
            }
        }
    }
    out
}

/// Le radici imposte da una prova, **per thread**.
///
/// Una variabile d'ambiente e' del processo, e i test Rust girano in parallelo
/// dentro lo stesso: due prove che dichiarano radici diverse se le rubano a
/// vicenda, e l'esito diventa una moneta — provato due volte, con la stessa
/// prova che rispondeva «FILE MANCANTE» su un file che c'era. Un mutex fra le
/// mie prove non e' bastato, perche' l'ambiente lo tocca anche chi non lo sa.
/// Qui l'override e' del thread, quindi nessuno puo' toccarlo per sbaglio.
#[cfg(test)]
fn test_roots() -> Option<Vec<PathBuf>> {
    TEST_ROOTS.with(|r| r.borrow().clone())
}

#[cfg(not(test))]
fn test_roots() -> Option<Vec<PathBuf>> {
    None
}

#[cfg(test)]
thread_local! {
    static TEST_ROOTS: std::cell::RefCell<Option<Vec<PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

/// Da come è scritto nella memoria al file sul disco.
fn resolve(path: &str) -> Option<PathBuf> {
    resolve_all(path).into_iter().next()
}

/// Tutti i file che quel percorso può nominare, radice per radice.
///
/// Serve perché `src/lib/api/base.ts` esiste in tre repo: prendere il primo che
/// si trova ancorerebbe la memoria a un file che non è quello di cui parla, e
/// l'errore sarebbe invisibile — l'impronta tornerebbe «fermo» per sempre sul
/// file sbagliato. Quando le corrispondenze sono più d'una, il timbro si astiene.
/// Alberi rigenerati da un comando: ancorarci una memoria e' ancorarla a un
/// artefatto, che cambia a ogni build e non e' mai stato scritto da nessuno.
const GENERATED: &[&str] = &[
    "/node_modules/",
    "/dist/",
    "/build/",
    "/out/",
    "/.next/",
    "/.mastra/",
    "/generated/",
    "/coverage/",
    "/target/",
];

fn is_generated(path: &Path) -> bool {
    let s = path.to_string_lossy();
    GENERATED.iter().any(|g| s.contains(g))
}

/// Vero se una **cartella** con questo nome è un albero rigenerato.
///
/// Serve perché `GENERATED` confronta segmenti fra due barre (`/dist/`), e la
/// cartella `…/dist` mentre la si visita non ne ha una in coda: l'indice per
/// nome ci entrava dentro e trovava `index.mjs` di una build.
fn is_generated_dir(name: &str) -> bool {
    GENERATED
        .iter()
        .any(|g| g.trim_matches('/') == name)
        || name == "node_modules"
        || name == "target"
}

fn resolve_all(path: &str) -> Vec<PathBuf> {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(path)
    };
    if expanded.is_absolute() {
        return if expanded.is_file() {
            vec![expanded]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    for root in roots() {
        let cand = root.join(&expanded);
        if cand.is_file() && !is_generated(&cand) && !out.contains(&cand) {
            out.push(cand);
        }
    }
    if out.is_empty() {
        // Le memorie citano un file col solo nome — `relay.rs`, `plancia.py` —
        // molto piu' spesso che col percorso, e quando scrivono un percorso lo
        // scrivono accorciato: `guards/handoff.rs` per il file che vive tre
        // cartelle piu' in basso. Cercare solo il join con le radici dichiarava
        // morto tutto cio' che sta in una sottocartella: 23 delle 272
        // «citazioni morte» del 19/08/2026 erano moduli Rust vivissimi.
        let written = expanded.to_string_lossy().to_string();
        let name = written.rsplit('/').next().unwrap_or(&written).to_string();
        let mut found = by_name(&name);
        // I segmenti scritti devono comparire nel candidato, in ordine: senza
        // questo vincolo un percorso accorciato accetterebbe anche il gemello
        // in un altro crate, e l'indicazione di chi cita andrebbe persa.
        let segments: Vec<String> = written
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(|s| s.to_string())
            .collect();
        if segments.len() > 1 {
            found.retain(|cand| contains_in_order(&cand.to_string_lossy(), &segments));
        }
        out = found;
    }
    out
}

/// Vero se i pezzi compaiono nel percorso uno dopo l'altro.
fn contains_in_order(haystack: &str, segments: &[String]) -> bool {
    let mut rest = haystack;
    for seg in segments {
        match rest.find(seg.as_str()) {
            Some(i) => rest = &rest[i + seg.len()..],
            None => return false,
        }
    }
    true
}

/// L'indice nome-di-file → percorsi, costruito una volta per esecuzione.
///
/// Camminare le radici a ogni citazione costerebbe un secondo per riga; qui si
/// paga una volta sola e si tiene in memoria. Gli alberi rigenerati e le
/// cartelle col punto non si visitano: il primo giro senza quel taglio ha letto
/// 190.000 file e non e' finito.
fn by_name(name: &str) -> Vec<PathBuf> {
    // Sotto prova l'indice si ricostruisce ogni volta: e' minuscolo, e una cache
    // globale farebbe ereditare a una prova l'albero di un'altra.
    if test_roots().is_some() {
        return build_index().get(name).cloned().unwrap_or_default();
    }
    static INDEX: OnceLock<BTreeMap<String, Vec<PathBuf>>> = OnceLock::new();
    let index = INDEX.get_or_init(build_index);
    index.get(name).cloned().unwrap_or_default()
}

fn build_index() -> BTreeMap<String, Vec<PathBuf>> {
    {
        let mut map: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for root in roots() {
            let mut stack = vec![root];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for e in entries.flatten() {
                    let p = e.path();
                    let file_name = e.file_name();
                    let file_name = file_name.to_string_lossy().to_string();
                    if p.is_dir() {
                        // Le cartelle col punto si saltano, **tranne `.claude`**:
                        // è lì che vivono gli script e i ganci che le memorie
                        // citano di più, e saltarla dichiarava morto
                        // `plancia.py` mentre sta in `gyver/work/.claude/scripts/`.
                        // È la stessa eccezione che SocratiCode fa indicizzando
                        // `.claude` come progetto a sé.
                        if (file_name.starts_with('.') && file_name != ".claude")
                            || is_generated_dir(&file_name)
                            || is_generated(&p)
                        {
                            continue;
                        }
                        stack.push(p);
                    } else {
                        let list = map.entry(file_name).or_default();
                        if !list.contains(&p) {
                            list.push(p);
                        }
                    }
                }
            }
        }
        map
    }
}

/// Il percorso assoluto riscritto con la tilde, che è come si citano i file
/// fuori dal repo corrente in tutta la configurazione.
fn tilde(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

// ─── Il rendiconto ───────────────────────────────────────────────────────────

struct Row {
    memory: String,
    anchor: Anchor,
    verdict: Verdict,
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.iter().any(|a| a == "--session-start") {
        return session_start();
    }
    let json = args.iter().any(|a| a == "--json");
    let suggest = args.iter().any(|a| a == "--suggest");
    let write = args.iter().any(|a| a == "--write");
    let restamp = args.iter().any(|a| a == "--restamp");
    let dead = args.iter().any(|a| a == "--dead");
    let only = args
        .iter()
        .position(|a| a == "--file")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let files = match &only {
        Some(f) => vec![f.clone()],
        None => memory_files(),
    };

    if suggest {
        return suggest_anchors(&files, write, json);
    }
    if restamp {
        return restamp_anchors(&files, json);
    }
    if dead {
        return dead_citations(&files, json);
    }
    check(&files, json)
}

/// All'apertura di una sessione: quali memorie di **questo** progetto parlano di
/// codice che nel frattempo e' cambiato.
///
/// E' il punto in cui serve saperlo. Una sessione che riparte legge `MEMORY.md`
/// e agisce su quello che c'e' scritto: se una memoria descrive una funzione che
/// non e' piu' quella, l'errore e' gia' dentro la prima decisione del turno.
///
/// Esce sempre 0 e non blocca niente: un gancio di avvio che fallisce sarebbe un
/// guasto in piu', non un avviso.
fn session_start() -> i32 {
    let mut raw = String::new();
    use std::io::Read as _;
    let _ = std::io::stdin().read_to_string(&mut raw);
    let Some(dir) = project_memory_dir(&raw) else {
        return 0;
    };
    if !dir.is_dir() {
        return 0;
    }
    let files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect(),
        Err(_) => return 0,
    };
    let rows = rows_for(&files);
    let alarming: Vec<&Row> = rows.iter().filter(|r| r.verdict.alarming()).collect();
    if alarming.is_empty() {
        return 0;
    }
    println!(
        "MEMORIE IN DERIVA: {} ancoraggi su {} non descrivono piu' il codice di adesso.",
        alarming.len(),
        rows.len()
    );
    for r in alarming.iter().take(MAX_LINES_AT_STARTUP) {
        println!("  [{}] {} — {}", r.verdict.label(), r.memory, r.anchor.render());
    }
    if alarming.len() > MAX_LINES_AT_STARTUP {
        println!("  … e altri {}", alarming.len() - MAX_LINES_AT_STARTUP);
    }
    println!(
        "Prima di agire su una di queste, rileggi il codice: \
         `claude-hooks memory-anchors` per l'elenco intero."
    );
    0
}

/// Quante righe si stampano all'avvio prima di riassumere.
///
/// Il messaggio finisce nel contesto del modello a ogni apertura: un elenco
/// lungo costa token a ogni sessione e non lo legge nessuno.
const MAX_LINES_AT_STARTUP: usize = 5;

/// La cartella delle memorie del progetto da cui parte la sessione.
///
/// Il nome del progetto e' il `cwd` con le barre trasformate in trattini —
/// `/Users/theo/orca/general` diventa `-Users-theo-orca-general`. E' la
/// convenzione dell'harness, non una nostra scelta.
fn project_memory_dir(hook_input: &str) -> Option<PathBuf> {
    let cwd = json_field(hook_input, "cwd")?;
    let slug: String = cwd
        .chars()
        .map(|c| if c == '/' { '-' } else { c })
        .collect();
    Some(home().join(".claude/projects").join(slug).join("memory"))
}

/// Il valore di una chiave di stringa nel JSON del gancio.
///
/// Un parser intero per leggere un campo sarebbe una dipendenza in piu' su un
/// binario che deve partire in millisecondi.
fn json_field(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = raw.find(&needle)? + needle.len();
    let rest = raw[start..].trim_start().strip_prefix(':')?.trim_start();
    let body = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

fn memory_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in memory_dirs() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "md") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

fn label_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn rows_for(files: &[PathBuf]) -> Vec<Row> {
    let mut rows = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for anchor in anchors_in(&text) {
            let source = resolve(&anchor.path).and_then(|p| std::fs::read_to_string(p).ok());
            let verdict = judge(&anchor, source.as_deref());
            rows.push(Row {
                memory: label_of(file),
                anchor,
                verdict,
            });
        }
    }
    rows
}

fn check(files: &[PathBuf], json: bool) -> i32 {
    let rows = rows_for(files);
    let alarms = rows.iter().filter(|r| r.verdict.alarming()).count();

    if json {
        let items: Vec<String> = rows
            .iter()
            .map(|r| {
                format!(
                    "{{\"memory\":{},\"anchor\":{},\"verdict\":{},\"alarming\":{}}}",
                    quote(&r.memory),
                    quote(&r.anchor.render()),
                    quote(r.verdict.label()),
                    r.verdict.alarming()
                )
            })
            .collect();
        println!(
            "{{\"anchors\":{},\"alarming\":{},\"items\":[{}]}}",
            rows.len(),
            alarms,
            items.join(",")
        );
        return i32::from(alarms > 0);
    }

    if rows.is_empty() {
        println!("Nessuna memoria ancorata. `--suggest` dice da dove cominciare.");
        return 0;
    }

    let mut by_memory: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        by_memory.entry(&r.memory).or_default().push(r);
    }
    for (memory, items) in &by_memory {
        if !items.iter().any(|r| r.verdict.alarming()) {
            continue;
        }
        println!("{memory}");
        for r in items {
            let extra = match &r.verdict {
                Verdict::Drifted { current } => format!("  (adesso @{current})"),
                _ => String::new(),
            };
            println!("  [{}] {}{}", r.verdict.label(), r.anchor.render(), extra);
        }
    }
    println!(
        "\n{} ancoraggi su {} memorie · {} in allarme",
        rows.len(),
        by_memory.len(),
        alarms
    );
    if alarms > 0 {
        println!("Le memorie qui sopra vanno rilette prima di agirci: il codice sotto è cambiato.");
        println!("Riallineate le impronte dopo aver corretto il testo: memory-anchors --restamp");
    }
    i32::from(alarms > 0)
}

/// Riscrive le impronte di ciò che è in deriva o non è mai stato timbrato.
///
/// Non tocca gli ancoraggi spariti o senza file: quelli vogliono una mano, non
/// un timbro nuovo — e riscriverli in silenzio cancellerebbe l'unica traccia
/// che qualcosa non torna.
fn restamp_anchors(files: &[PathBuf], json: bool) -> i32 {
    let mut touched = 0;
    let mut changed_files = 0;
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let anchors = anchors_in(&text);
        if anchors.is_empty() {
            continue;
        }
        let mut fresh = Vec::new();
        let mut dirty = false;
        for a in anchors {
            let source = resolve(&a.path).and_then(|p| std::fs::read_to_string(p).ok());
            let new_print = match judge(&a, source.as_deref()) {
                Verdict::Drifted { current } => Some(current),
                Verdict::Unstamped { current } => current,
                _ => None,
            };
            match new_print {
                Some(p) if Some(&p) != a.print.as_ref() => {
                    dirty = true;
                    touched += 1;
                    fresh.push(Anchor {
                        print: Some(p),
                        ..a
                    });
                }
                _ => fresh.push(a),
            }
        }
        if dirty {
            let updated = with_anchors(&text, &fresh);
            if std::fs::write(file, updated).is_ok() {
                changed_files += 1;
            }
        }
    }
    if json {
        println!("{{\"restamped\":{touched},\"files\":{changed_files}}}");
    } else {
        println!("{touched} impronte riallineate in {changed_files} memorie.");
    }
    0
}

// ─── Il suggerimento ─────────────────────────────────────────────────────────

/// Quanti simboli si ancorano al massimo per ogni file citato.
///
/// Una consegna nomina decine di funzioni: senza tetto una memoria sola
/// porterebbe quaranta ancoraggi, e il rendiconto diventerebbe illeggibile
/// proprio dove serve leggerlo.
const MAX_SYMBOLS_PER_FILE: usize = 3;

/// Trasforma le citazioni che la memoria già contiene in ancoraggi timbrati.
///
/// Le citazioni che non si risolvono **non** diventano ancoraggi: un ancoraggio
/// a un file che non c'è direbbe «file mancante» a ogni verifica, per sempre, e
/// un elenco che grida sempre non fa più guardare nessuno. Restano contate
/// nell'ultima riga, che è la misura del debito.
fn suggest_anchors(files: &[PathBuf], write: bool, json: bool) -> i32 {
    let mut proposals: BTreeMap<String, Vec<Anchor>> = BTreeMap::new();
    let mut unresolved = 0;
    let mut ambiguous = 0;
    let mut cited = 0;

    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let existing = anchors_in(&text);
        let already: Vec<String> = existing
            .iter()
            .filter(|a| a.symbol.is_some())
            .map(|a| a.path.clone())
            .collect();
        let names = identifiers(&text);
        let mut found: Vec<Anchor> = Vec::new();
        for cit in citations(&text) {
            cited += 1;
            let matches = resolve_all(&cit.path);
            let disk = match matches.len() {
                0 => {
                    unresolved += 1;
                    continue;
                }
                1 => matches[0].clone(),
                _ => {
                    ambiguous += 1;
                    continue;
                }
            };
            // Il percorso dell'ancoraggio è quello canonico, non quello scritto
            // nella memoria: è l'unico modo perché la verifica di domani guardi
            // lo stesso file di oggi.
            let canonical = tilde(&disk);
            if already.contains(&canonical) || already.contains(&cit.path) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&disk) else {
                continue;
            };
            // Prima i simboli che la memoria nomina e che in quel file esistono
            // davvero: sono gli ancoraggi che dicono qualcosa sul contenuto.
            let lang = Lang::from_path(&cit.path);
            let mut by_symbol = 0;
            for name in &names {
                if by_symbol >= MAX_SYMBOLS_PER_FILE {
                    break;
                }
                // Solo le definizioni di primo livello: un nome comune che
                // vive dentro una funzione (`hex`, `phase`, `role`) combacia
                // per caso, e l'ancoraggio che ne esce non dice niente.
                match declaration_line(&source, name, lang) {
                    Some(i) if source.lines().nth(i).is_some_and(|l| !l.starts_with(' ')) => {}
                    _ => continue,
                }
                let Some(body) = extract_symbol(&source, name, lang) else {
                    continue;
                };
                let anchor = Anchor {
                    path: canonical.clone(),
                    symbol: Some(name.clone()),
                    print: Some(fingerprint(&normalise(&body, lang))),
                };
                if !found.iter().any(|a| a.render() == anchor.render()) {
                    found.push(anchor);
                    by_symbol += 1;
                }
            }
            if by_symbol > 0 {
                continue;
            }
            if let Some(mut anchor) = anchor_for(&cit, &source) {
                anchor.path = canonical;
                if !found.iter().any(|a| a.render() == anchor.render()) {
                    found.push(anchor);
                }
            }
        }
        // Un file ancorato a un suo simbolo non ha piu bisogno dell'ancoraggio
        // nudo: la sparizione del file la segnala lo stesso.
        let with_symbol: Vec<String> = found
            .iter()
            .filter(|a| a.symbol.is_some())
            .map(|a| a.path.clone())
            .collect();
        found.retain(|a| a.symbol.is_some() || !with_symbol.contains(&a.path));
        // Anche una memoria senza proposte nuove entra nell'elenco se i suoi
        // ancoraggi hanno un doppione: la riscrittura li normalizza. E' la
        // riparazione dei duplicati gia' scritti da una versione precedente.
        let has_twins = {
            let rendered: Vec<String> = existing.iter().map(|a| a.render()).collect();
            let mut unique = rendered.clone();
            unique.sort();
            unique.dedup();
            unique.len() != rendered.len()
        };
        if !found.is_empty() || has_twins {
            proposals.insert(file.display().to_string(), found);
        }
    }

    let total: usize = proposals.values().map(Vec::len).sum();

    if write {
        let mut written = 0;
        for (file, fresh) in &proposals {
            let path = PathBuf::from(file);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let promoted: Vec<String> = fresh
                .iter()
                .filter(|a| a.symbol.is_some())
                .map(|a| a.path.clone())
                .collect();
            let mut all: Vec<Anchor> = anchors_in(&text)
                .into_iter()
                .filter(|a| a.symbol.is_some() || !promoted.contains(&a.path))
                .collect();
            all.extend(fresh.iter().cloned());
            // Due passate di `--suggest --write` scrivevano due volte lo stesso
            // ancoraggio nudo: il filtro sopra lascia passare i percorsi senza
            // simbolo, ed e' voluto (uno con simbolo deve poterli sostituire),
            // ma senza questa riga il frontmatter cresce a ogni esecuzione.
            let mut seen: Vec<String> = Vec::new();
            all.retain(|a| {
                let key = a.render();
                if seen.contains(&key) {
                    false
                } else {
                    seen.push(key);
                    true
                }
            });
            if std::fs::write(&path, with_anchors(&text, &all)).is_ok() {
                written += 1;
            }
        }
        if json {
            println!("{{\"written_files\":{written},\"anchors\":{total}}}");
        } else {
            println!("{total} ancoraggi scritti in {written} memorie.");
        }
        return 0;
    }

    if json {
        let items: Vec<String> = proposals
            .iter()
            .flat_map(|(f, list)| {
                list.iter().map(move |a| {
                    format!(
                        "{{\"memory\":{},\"anchor\":{}}}",
                        quote(f),
                        quote(&a.render())
                    )
                })
            })
            .collect();
        println!(
            "{{\"proposals\":{total},\"cited\":{cited},\"unresolved\":{unresolved},\"ambiguous\":{ambiguous},\"items\":[{}]}}",
            items.join(",")
        );
        return 0;
    }

    for (file, list) in &proposals {
        println!("{}", label_of(Path::new(file)));
        for a in list {
            println!("  + {}", a.render());
        }
    }
    println!(
        "\n{total} ancoraggi proponibili in {} memorie.",
        proposals.len()
    );
    println!(
        "{cited} citazioni lette · {unresolved} non si risolvono su nessuna radice · \
         {ambiguous} nominano un file che esiste in piu repo, e restano fuori."
    );
    println!("Per scriverli: memory-anchors --suggest --write");
    0
}

/// I file che una memoria cita e che non esistono da nessuna parte.
///
/// Non diventano ancoraggi — un ancoraggio a un file che non c'e' griderebbe a
/// ogni verifica, per sempre — ma sono la prova piu' forte che una memoria e'
/// scaduta, ed e' l'unica che si legge senza aprire il codice.
/// Il caso che l'ha motivata: `staffetta-cancella-ogni-sessione` racconta
/// `relay.py:98-99` e `register-session.py`, portati in Rust il 17/08/2026. La
/// memoria descrive un sistema che non esiste piu', e nessun controllo lo diceva.
fn dead_citations(files: &[PathBuf], json: bool) -> i32 {
    let mut per_memory: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut total = 0;
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let mut dead: Vec<String> = Vec::new();
        for cit in citations(&text) {
            if !resolve_all(&cit.path).is_empty() {
                continue;
            }
            if !dead.contains(&cit.path) {
                dead.push(cit.path.clone());
                total += 1;
            }
        }
        if !dead.is_empty() {
            per_memory.insert(label_of(file), dead);
        }
    }
    if json {
        let items: Vec<String> = per_memory
            .iter()
            .map(|(m, list)| {
                let paths: Vec<String> = list.iter().map(|p| quote(p)).collect();
                format!("{{\"memory\":{},\"dead\":[{}]}}", quote(m), paths.join(","))
            })
            .collect();
        println!(
            "{{\"dead\":{total},\"memories\":{},\"items\":[{}]}}",
            per_memory.len(),
            items.join(",")
        );
        return i32::from(total > 0);
    }
    for (memory, list) in &per_memory {
        println!("{memory}");
        for p in list {
            println!("  ? {p}");
        }
    }
    println!(
        "\n{total} file citati e introvabili, in {} memorie.",
        per_memory.len()
    );
    println!(
        "Un file che non esiste piu' non rende falsa la memoria da solo: leggila \
         e decidi se il fatto vale ancora, poi correggi il percorso o archiviala."
    );
    i32::from(total > 0)
}

/// L'ancoraggio migliore ricavabile da una citazione.
///
/// L'ordine è quello della robustezza: il simbolo scritto a mano batte il
/// simbolo dedotto dalla riga, che batte il percorso nudo. Il percorso nudo non
/// porta impronta: dice «questo file esiste», e basta.
fn anchor_for(cit: &Citation, source: &str) -> Option<Anchor> {
    let lang = Lang::from_path(&cit.path);
    let symbol = match (&cit.symbol, cit.line) {
        (Some(s), _) => Some(s.clone()),
        (None, Some(line)) => symbol_at_line(source, line, lang),
        (None, None) => None,
    };
    let Some(name) = symbol else {
        // Nessun simbolo da agganciare: resta l'esistenza del file, che da sola
        // risponde al 43% di citazioni che non si risolvono piu.
        return Some(Anchor {
            path: cit.path.clone(),
            symbol: None,
            print: None,
        });
    };
    let body = guards::memory_anchor::extract_symbol(source, &name, lang)?;
    Some(Anchor {
        path: cit.path.clone(),
        symbol: Some(name),
        print: Some(fingerprint(&normalise(&body, lang))),
    })
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    /// Le radici di questa prova, viste solo dal suo thread.
    fn with_roots(dir: &Path) {
        TEST_ROOTS.with(|r| *r.borrow_mut() = Some(vec![dir.to_path_buf()]));
    }

    /// Una cartella usa-e-getta con una memoria e un sorgente dentro.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("memory-anchors-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_a_relative_path_under_a_declared_root() {
        let dir = scratch("resolve");
        fs::write(dir.join("codice.rs"), "fn a() {}\n").unwrap();
        with_roots(&dir);
        assert!(resolve("codice.rs").is_some());
        assert!(resolve("non-esiste.rs").is_none());
    }

    #[test]
    fn a_citation_with_a_line_becomes_an_anchor_with_a_symbol() {
        let source = "fn uno() {\n    1\n}\n\nfn due() {\n    2\n}\n";
        let cit = Citation {
            path: "x.rs".into(),
            line: Some(6),
            symbol: None,
        };
        let a = anchor_for(&cit, source).unwrap();
        assert_eq!(a.symbol.as_deref(), Some("due"));
        assert!(a.print.is_some());
    }

    #[test]
    fn a_citation_without_a_symbol_is_anchored_to_the_files_existence() {
        let source = "x\n".repeat(500);
        let cit = Citation {
            path: "x.rs".into(),
            line: None,
            symbol: None,
        };
        let a = anchor_for(&cit, &source).unwrap();
        assert_eq!(a.render(), "x.rs");
        // Il file c'e: nessun allarme, e nessuna impronta da riallineare.
        assert_eq!(judge(&a, Some(&source)), Verdict::Steady);
        assert_eq!(judge(&a, None), Verdict::FileGone);
    }

    #[test]
    fn the_project_folder_comes_from_the_working_directory() {
        let dir = project_memory_dir(r#"{"session_id":"x","cwd":"/Users/theo/orca/general"}"#)
            .unwrap();
        assert!(
            dir.ends_with("projects/-Users-theo-orca-general/memory"),
            "{dir:?}"
        );
        assert!(project_memory_dir("{}").is_none());
    }

    #[test]
    fn a_bare_file_name_is_found_in_a_subfolder() {
        let dir = scratch("byname");
        std::fs::create_dir_all(dir.join("crates/guards/src")).unwrap();
        fs::write(dir.join("crates/guards/src/chain.rs"), "fn a() {}\n").unwrap();
        with_roots(&dir);
        assert!(resolve("chain.rs").is_some(), "il nome nudo non si trova");
    }

    #[test]
    fn a_shortened_path_finds_the_file_it_names() {
        let dir = scratch("shortened");
        let one = dir.join("crates/guards/src");
        let two = dir.join("crates/arms/src");
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        fs::write(one.join("handoff.rs"), "fn a() {}\n").unwrap();
        fs::write(two.join("handoff.rs"), "fn b() {}\n").unwrap();
        with_roots(&dir);
        // Il nome nudo è ambiguo: due file, nessuna scelta.
        assert_eq!(resolve_all("handoff.rs").len(), 2);
        // Col pezzo di percorso che la memoria ha scritto, uno solo.
        let picked = resolve_all("guards/handoff.rs");
        assert_eq!(picked.len(), 1, "{picked:?}");
        assert!(picked[0].starts_with(&one));
    }

    #[test]
    fn a_generated_tree_is_never_a_candidate() {
        let dir = scratch("generated");
        fs::create_dir_all(dir.join("dist")).unwrap();
        fs::write(dir.join("dist/index.mjs"), "const a = 1;\n").unwrap();
        with_roots(&dir);
        assert!(resolve("dist/index.mjs").is_none());
    }

    #[test]
    fn running_the_suggestion_twice_does_not_duplicate_anchors() {
        let dir = scratch("twice");
        fs::write(dir.join("codice.rs"), "fn outer() {\n    1\n}\n").unwrap();
        let memory = dir.join("m.md");
        fs::write(&memory, "---\nname: m\n---\n\nSta in `codice.rs`.\n").unwrap();
        with_roots(&dir);

        suggest_anchors(&[memory.clone()], true, true);
        suggest_anchors(&[memory.clone()], true, true);

        let after = fs::read_to_string(&memory).unwrap();
        assert_eq!(after.matches("codice.rs").count(), 2, "{after}"); // frontmatter + corpo
    }

    #[test]
    fn only_top_level_symbols_become_anchors() {
        let dir = scratch("toplevel");
        fs::write(
            dir.join("codice.rs"),
            "fn outer() {\n    let hidden = 1;\n    hidden\n}\n",
        )
        .unwrap();
        let memory = dir.join("m.md");
        // La memoria nomina tutti e due: solo `outer` e' un appiglio.
        fs::write(
            &memory,
            "---\nname: m\n---\n\nIn `codice.rs` la funzione `outer` usa `hidden`.\n",
        )
        .unwrap();
        with_roots(&dir);

        suggest_anchors(&[memory.clone()], true, true);

        let after = fs::read_to_string(&memory).unwrap();
        assert!(after.contains("codice.rs#outer@"), "{after}");
        assert!(!after.contains("#hidden"), "{after}");
    }

    #[test]
    fn restamp_only_touches_drift_and_never_a_missing_file() {
        let dir = scratch("restamp");
        fs::write(dir.join("codice.rs"), "fn uno() {\n    1\n}\n").unwrap();
        let memory = dir.join("m.md");
        fs::write(
            &memory,
            "---\nname: m\nanchors:\n  - codice.rs#uno@000000000000\n  - sparito.rs#x@111111111111\n---\n\ncorpo\n",
        )
        .unwrap();
        with_roots(&dir);

        restamp_anchors(&[memory.clone()], true);

        let after = fs::read_to_string(&memory).unwrap();
        assert!(!after.contains("codice.rs#uno@000000000000"), "{after}");
        assert!(after.contains("sparito.rs#x@111111111111"), "{after}");
    }

    #[test]
    fn check_reports_the_drift_it_finds() {
        let dir = scratch("check");
        fs::write(dir.join("codice.rs"), "fn uno() {\n    1\n}\n").unwrap();
        let memory = dir.join("m.md");
        fs::write(
            &memory,
            "---\nname: m\nanchors:\n  - codice.rs#uno@000000000000\n---\n\ncorpo\n",
        )
        .unwrap();
        with_roots(&dir);
        assert_eq!(check(&[memory.clone()], true), 1);

        // Timbrato bene, l'allarme si spegne.
        restamp_anchors(&[memory.clone()], true);
        assert_eq!(check(&[memory], true), 0);
    }
}
