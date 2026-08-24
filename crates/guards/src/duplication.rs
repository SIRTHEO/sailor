//! Il codice ricopiato, visto mentre viene scritto e non in revisione.
//!
//! Porta di `skills/hooks/duplication.py`. È il muro della catena `Write`/`Edit`
//! — 30,5 ms — e a differenza degli otto ganci portati prima il tempo non è
//! avvio dell'interprete: è camminare l'albero, leggere fino a 300 file e farci
//! sopra una sottosequenza comune. Qui il porting guadagna davvero.
//!
//! PERCHÉ ESISTE. Misura sulla suite (12/08/2026, 104 file «sorelle»): l'11%
//! delle righe sta verbatim in un file fratello. Il meccanismo è sempre lo
//! stesso — si consegna un'area nuova copiando quella più simile, e resta un
//! commento che chiede di «tenere allineati» i due file. Quel commento è
//! sopravvissuto tre settimane.
//!
//! DOVE STA LA SOGLIA, e perché lì. Tarata su un caso vero: la lettura dei
//! parametri di ricerca stava identica in dodici shim di route. Sono cinque
//! righe di logica, quattro quando la copia è locale e l'originale è esportato.
//! Una soglia a otto righe non l'avrebbe vista nemmeno una volta su dodici: la
//! duplicazione che conta è corta, si copia una funzione, non un modulo. Alla
//! lunghezza si affianca un minimo di sostanza in caratteri, che separa quattro
//! righe di logica da quattro di intelaiatura.
//!
//! LA LINEA DI BASE è ciò che lo tiene acceso. 122 file su 1160 contengono già
//! un blocco ricopiato: senza memoria, toccarne uno farebbe scattare un
//! rimprovero per una copia scritta mesi fa da un altro, e un controllo che
//! accusa del debito altrui viene spento entro il pomeriggio. Le coppie note si
//! congelano una volta e da lì tacciono; parla solo la duplicazione **nuova**.
//! L'impronta è (file, file, contenuto) senza i numeri di riga: aggiungere un
//! import sopra non deve far ricomparire un debito congelato, ma la tredicesima
//! copia in un file diverso è una coppia nuova, e parla.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Lunghezza minima, in righe significative, di un blocco che vale la pena
/// segnalare. La valvola `DUPLICAZIONE_RIGHE` la cambia.
///
/// Il valore è 4, mentre il riepilogo in testa all'originale dice 5: è il
/// riepilogo a essere rimasto indietro, e il numero vero è quello del codice.
pub const MIN_LINES: usize = 4;

/// Un blocco corto passa solo se ha sostanza: cinque righe di intelaiatura JSX
/// stanno sotto, cinque righe di logica stanno sopra.
const MIN_CHARS: usize = 180;

const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "py", "mjs"];

/// Oltre questo numero di candidati il confronto costa più di quanto renda: si
/// tengono i più vicini per percorso, che sono anche i più probabili donatori.
const MAX_SIBLINGS: usize = 300;

/// I punti di riuso: dove il codice condiviso *deve* stare. Vanno confrontati
/// sempre, non solo coi pari — senza di loro il rilevatore non vede il caso più
/// prezioso, riscrivere a mano una funzione che esiste già in `lib/`.
const REUSE_DIRS: &[&str] = &["/lib/", "/hooks/", "/utils/", "/helpers/", "/shared/"];

/// Cartelle che non sono codice nostro: confrontarcisi non insegna niente.
const EXCLUDED: &[&str] = &[
    "node_modules", "/dist/", "/.git/", "/build/", "routeTree.gen", "/animate-ui/",
    "/components/ui/", "/.claude/",
];

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*")
}

fn is_trivial(bare: &str) -> bool {
    bare.chars().all(|c| " \t{}()[];,<>".contains(c))
}

/// Solo import e ri-export puri. `export function` ed `export const` sono
/// dichiarazioni, cioè logica: scartarli aveva reso invisibile proprio il caso
/// su cui la soglia è tarata — la copia locale contro l'originale esportato.
fn is_import(line: &str) -> bool {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("import") {
        return rest.starts_with(char::is_whitespace);
    }
    let Some(rest) = t.strip_prefix("export") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    rest.starts_with('*')
        || rest.starts_with('{')
        || rest
            .strip_prefix("type")
            .map(|r| r.starts_with(char::is_whitespace) && r.trim_start().starts_with('{'))
            .unwrap_or(false)
}

/// Le righe che sono logica e non forma, col loro numero (a partire da 1).
///
/// La normalizzazione è volutamente povera: spazi collassati e niente altro.
/// Normalizzare anche gli identificatori troverebbe più copie (il 14% invece
/// dell'11%) ma segnalerebbe come uguali due funzioni che fanno cose diverse
/// sullo stesso schema — e un rapporto che grida al lupo viene spento.
pub fn significant_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (n, line) in text.lines().enumerate() {
        let bare = line.trim();
        if in_block {
            if bare.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if bare.starts_with("/*") {
            in_block = !bare.contains("*/");
            continue;
        }
        if bare.is_empty() || is_comment(line) || is_import(line) {
            continue;
        }
        if is_trivial(bare) || bare.chars().count() < 12 {
            continue;
        }
        out.push((n + 1, collapse_spaces(bare)));
    }
    out
}

/// `re.sub(r'\s+', ' ', s)` su una stringa già ripulita ai bordi, senza montare
/// un motore di regex per questo.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// L'ultimo segmento del nome, che dice a quale famiglia appartiene il file.
///
/// `external-users-bulk-bar.tsx` e `candidates-bulk-bar.tsx` sono la stessa cosa
/// per due entità diverse: è esattamente la coppia che si copia.
pub fn family_tag(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.rsplit(['-', '.']).next().unwrap_or(&stem).to_string()
}

fn is_test(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.to_string_lossy().contains("__tests__") || name.contains(".test.")
}

fn has_extension(path: &Path) -> bool {
    path.extension()
        .map(|e| EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
        .unwrap_or(false)
}

/// Tutti i file sotto una radice, **in ordine di percorso**.
///
/// L'ordinamento è deliberato. Il `rglob` di Python segue l'ordine di `readdir`,
/// che su APFS non è alfabetico: con più di `MAX_SIBLINGS` candidati il taglio
/// cadeva su file diversi a seconda di come il filesystem aveva disposto le
/// voci, e lo stesso file poteva produrre due rapporti diversi su due macchine.
/// L'ordinamento è stato aggiunto anche all'originale il 17/08/2026, così le due
/// implementazioni sono confrontabili e nessuna delle due dipende più dal
/// filesystem.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

/// I file con cui ha senso confrontarsi: la famiglia, la zona, il riuso.
pub fn siblings(target: &Path, root: &Path) -> Vec<PathBuf> {
    let base = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let family = family_tag(&base);
    let target_is_test = is_test(&base);
    let parent = base.parent().map(|p| p.to_path_buf());

    let mut found: Vec<PathBuf> = walk(root)
        .into_iter()
        .filter(|p| {
            if !has_extension(p) {
                return false;
            }
            let s = p.to_string_lossy().into_owned();
            if EXCLUDED.iter().any(|x| s.contains(x)) {
                return false;
            }
            if p.canonicalize().map(|c| c == base).unwrap_or(false) {
                return false;
            }
            if is_test(p) != target_is_test {
                return false;
            }
            family_tag(p) == family
                || p.parent().map(|d| Some(d.to_path_buf()) == parent).unwrap_or(false)
                || (!target_is_test && REUSE_DIRS.iter().any(|x| s.contains(x)))
        })
        .collect();

    // I più vicini per percorso: un fratello nella cartella accanto è un
    // donatore più probabile di uno all'altro capo dell'albero. Il confronto è
    // per **carattere**, come `os.path.commonprefix`, non per componente, e
    // l'ordinamento è stabile — a parità di prefisso resta l'ordine alfabetico.
    let base_string = base.to_string_lossy().into_owned();
    found.sort_by_key(|p| {
        std::cmp::Reverse(common_prefix_len(&p.to_string_lossy(), &base_string))
    });
    found.truncate(MAX_SIBLINGS);
    found
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Un blocco contiguo identico fra due file.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Block {
    pub line_here: usize,
    pub line_there: usize,
    pub count: usize,
    pub fingerprint: String,
}

/// I blocchi contigui identici fra due elenchi di righe significative.
///
/// Programmazione dinamica sulla sottosequenza comune più lunga, in versione a
/// una riga di memoria: i file qui sono di poche centinaia di righe, e la forma
/// esplicita costa meno di una dipendenza esterna.
pub fn shared_blocks(
    lines_a: &[(usize, String)],
    lines_b: &[(usize, String)],
    minimum: usize,
) -> Vec<Block> {
    let a: Vec<&str> = lines_a.iter().map(|(_, t)| t.as_str()).collect();
    let b: Vec<&str> = lines_b.iter().map(|(_, t)| t.as_str()).collect();
    // Scorciatoia, non guardia: con un elenco vuoto i cicli qui sotto non
    // girerebbero comunque. Nessuna prova può distinguerla, ed è giusto così.
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut previous = vec![0usize; b.len() + 1];
    // Per riga d'inizio, come il dizionario dell'originale: a parità di inizio
    // vince l'ultimo trovato.
    let mut results: HashMap<usize, Block> = HashMap::new();

    for i in 1..=a.len() {
        let mut current = vec![0usize; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                current[j] = previous[j - 1] + 1;
                // Fine corsa: la prossima coppia non prosegue il blocco.
                let ends = i == a.len() || j == b.len() || a[i] != b[j];
                // Aspettare la fine corsa è un risparmio, non una condizione:
                // un blocco ancora aperto tornerebbe qui più lungo, con lo
                // stesso inizio, e riscriverebbe la voce. Anche questa nessuna
                // prova può distinguerla.
                if ends && current[j] >= minimum {
                    let body = &a[i - current[j]..i];
                    if body.iter().map(|x| x.chars().count()).sum::<usize>() >= MIN_CHARS {
                        let start_a = lines_a[i - current[j]].0;
                        results.insert(
                            start_a,
                            Block {
                                line_here: start_a,
                                line_there: lines_b[j - current[j]].0,
                                count: current[j],
                                fingerprint: fingerprint(body),
                            },
                        );
                    }
                }
            }
        }
        previous = current;
    }

    let mut out: Vec<Block> = results.into_values().collect();
    // Per lunghezza decrescente; a parità, per riga d'inizio, così l'ordine non
    // dipende da come la mappa è stata percorsa.
    out.sort_by_key(|r| (std::cmp::Reverse(r.count), r.line_here));
    out
}

/// Le prime sedici cifre esadecimali di SHA-1 sul corpo del blocco.
fn fingerprint(body: &[&str]) -> String {
    let digest = sha1(body.join("\n").as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

/// SHA-1 (RFC 3174), scritto a mano invece che con una dipendenza.
///
/// Non serve per sicurezza — dà un nome stabile a un blocco di righe — ma deve
/// produrre **le stesse** cifre di `hashlib.sha1`: la linea di base congelata è
/// un file solo, e le due implementazioni la leggono entrambe. Se le impronte
/// divergessero, tutto il debito congelato tornerebbe a parlare in una volta.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    let bit_length = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        // Le due funzioni qui sotto si scrivono anche con `^` al posto di `|`,
        // e danno lo stesso identico risultato: nella prima i due termini non
        // hanno bit in comune, nella seconda vale l'identità della maggioranza.
        // Non c'è dato che possa distinguere le due scritture.
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// L'identità di un blocco duplicato, stabile allo spostarsi delle righe.
///
/// Contiene i due percorsi — ordinati, così la coppia è la stessa da entrambi i
/// lati — e il contenuto. Non i numeri di riga: aggiungere un import sopra non
/// deve far ricomparire un debito già congelato.
pub fn pair_signature(root: &Path, a: &Path, b: &Path, fingerprint: &str) -> String {
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let relative = |p: &Path| -> Option<String> {
        let full = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        full.strip_prefix(&base)
            .ok()
            .map(|r| r.to_string_lossy().into_owned())
    };
    // Come l'originale: se **uno dei due** è fuori dalla radice si ricade sui
    // percorsi non risolti per entrambi, perché lì `relative_to` solleva.
    let (mut pa, mut pb) = match (relative(a), relative(b)) {
        (Some(x), Some(y)) => (x, y),
        _ => (
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ),
    };
    if pa > pb {
        std::mem::swap(&mut pa, &mut pb);
    }
    format!("{pa}|{pb}|{fingerprint}")
}

/// Il file di linea di base per questa radice, distinto per percorso.
///
/// Un worktree è una radice diversa dal checkout canonico e ha la sua: il debito
/// congelato là non vale qui, e mescolarli darebbe silenzi falsi.
pub fn baseline_path(root: &Path) -> PathBuf {
    let resolved = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let digest = sha1(resolved.to_string_lossy().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    state_dir().join(format!("{}.json", &hex[..12]))
}

pub fn state_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".claude")
        .join("state")
        .join("duplicazione")
}

/// Le coppie già congelate per questa radice. Un file mancante o illeggibile
/// vale come «niente di congelato»: il rilevatore parlerà di più, non di meno.
pub fn load_baseline(root: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(baseline_path(root)) else {
        return HashSet::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashSet::new();
    };
    value
        .get("coppie")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Un riscontro: il fratello, dove sta qui, dove sta lì, quanto è lungo.
#[derive(Debug, Clone)]
pub struct Finding {
    pub sibling: PathBuf,
    pub line_here: usize,
    pub line_there: usize,
    pub count: usize,
    pub signature: String,
}

/// Quante righe del file sono coperte da una copia, e i riscontri.
///
/// `known` sono le firme congelate: quei blocchi non si segnalano. `full` toglie
/// il taglio ai primi sei — serve a congelare, dove un riscontro dimenticato
/// resterebbe a suonare per sempre.
pub fn report(
    target: &Path,
    root: &Path,
    minimum: usize,
    known: &HashSet<String>,
    full: bool,
) -> (usize, Vec<Finding>) {
    let Ok(text) = read_lossy(target) else {
        return (0, Vec::new());
    };
    let mine = significant_lines(&text);
    if mine.len() < minimum {
        return (0, Vec::new());
    }
    let mut findings = Vec::new();
    let mut covered: HashSet<usize> = HashSet::new();
    for file in siblings(target, root) {
        let Ok(other) = read_lossy(&file) else { continue };
        let theirs = significant_lines(&other);
        for block in shared_blocks(&mine, &theirs, minimum) {
            let signature = pair_signature(root, target, &file, &block.fingerprint);
            if known.contains(&signature) {
                continue;
            }
            covered.extend(block.line_here..block.line_here + block.count);
            findings.push(Finding {
                sibling: file.clone(),
                line_here: block.line_here,
                line_there: block.line_there,
                count: block.count,
                signature,
            });
        }
    }
    // Stabile, come `list.sort` di Python: a parità di lunghezza resta l'ordine
    // in cui i fratelli sono stati percorsi.
    findings.sort_by_key(|r| std::cmp::Reverse(r.count));
    if !full {
        findings.truncate(6);
    }
    (covered.len(), findings)
}

/// `read_text(errors='replace')`: un file con byte non validi si legge lo
/// stesso, perché il rilevatore non deve fermarsi davanti a un carattere strano.
fn read_lossy(path: &Path) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// La radice del repo, o la cartella del file se non è sotto git.
pub fn root_of(path: &Path) -> PathBuf {
    let start = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));
    root_from_dir(&start)
}

/// La stessa risalita, ma partendo da una **cartella**.
///
/// `root_of` toglie un livello perche' riceve il percorso di un file: passandole
/// una cartella si perde proprio quella che si voleva usare. Chi congela la
/// linea di base riceve una cartella, e senza questa porta d'ingresso finiva a
/// camminare l'albero di sopra — provato: da una radice di prova sotto `/tmp`
/// ha censito 290 file estranei.
pub fn root_from_dir(dir: &Path) -> PathBuf {
    let start = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut current = Some(start.clone());
    while let Some(d) = current {
        if d.join(".git").exists() {
            let src = d.join("src");
            return if src.is_dir() { src } else { d };
        }
        current = d.parent().map(|p| p.to_path_buf());
    }
    start
}

pub fn post_text(target: &Path, covered: usize, findings: &[Finding]) -> String {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut lines = vec![format!(
        "codice ricopiato in {name}: {covered} righe stanno già, identiche, in un file fratello."
    )];
    for f in findings {
        lines.push(format!(
            "  · righe {}-{} = {}:{} ({} righe)",
            f.line_here,
            f.line_here + f.count - 1,
            f.sibling.display(),
            f.line_there,
            f.count
        ));
    }
    lines.push(
        "  Prima di proseguire: estrai la parte comune nel punto di riuso della \
         sua famiglia e falla usare a entrambi, oppure — se le due devono \
         davvero divergere — scrivi nel codice perché. Un commento che chiede \
         di «tenere allineati» due file non è una soluzione: è il difetto."
            .to_string(),
    );
    lines.join("\n")
}

/// Il testo dell'avviso alla nascita di un file, o `None` se non c'è famiglia.
pub fn pre_text(target: &Path, root: &Path) -> Option<String> {
    // Solo i veri omonimi: i punti di riuso servono al confronto in `post`, ma
    // in un elenco «ecco la tua famiglia» sono rumore.
    let family = family_tag(target);
    let neighbors: Vec<PathBuf> = siblings(target, root)
        .into_iter()
        .filter(|v| family_tag(v) == family)
        .take(8)
        .collect();
    if neighbors.is_empty() {
        return None;
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut lines = vec![format!(
        "stai creando {name}; la sua famiglia esiste già:"
    )];
    for v in &neighbors {
        let count = read_lossy(v).map(|t| t.lines().count()).unwrap_or(0);
        lines.push(format!("  · {}  ({count} righe)", v.display()));
    }
    lines.push(
        "  Aprine almeno uno prima di scrivere: se il file nuovo gli somiglia, \
         la parte comune va estratta ora — dopo la quinta copia costa una \
         passata su tutte."
            .to_string(),
    );
    Some(lines.join("\n"))
}

/// Vero se il file ha un'estensione che il rilevatore guarda e non sta in una
/// cartella esclusa.
pub fn is_watched(path: &str) -> bool {
    let p = Path::new(path);
    has_extension(p) && !EXCLUDED.iter().any(|x| path.contains(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: [u8; 20]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// L'oracolo di `hashlib.sha1`: senza queste cifre la linea di base
    /// congelata dal Python sarebbe illeggibile da qui, e tutto il debito
    /// vecchio tornerebbe a parlare in una volta.
    /// Le cifre vengono da `hashlib.sha1`, non dalla memoria: le lunghezze 55,
    /// 56, 57 e 64 sono i casi limite del riempimento, dove un byte in più
    /// costringe a un blocco in più ed è lì che un'implementazione a mano
    /// sbaglia.
    #[test]
    fn the_digest_matches_the_one_python_computes() {
        for (data, expected) in [
            (vec![], "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            (b"abc".to_vec(), "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (b"a".repeat(55), "c1c8bbdc22796e28c0e15163d20899b65621d65a"),
            (b"a".repeat(56), "c2db330f6083854c99d4b5bfb6e8f29f201be699"),
            (b"a".repeat(57), "f08f24908d682555111be7ff6f004e78283d989a"),
            (b"a".repeat(64), "0098ba824b5c16427bd7a1122a5a442a25ec644d"),
            (b"a".repeat(119), "ee971065aaa017e0632a8ca6c77bb3bf8b1dfc56"),
            (b"a".repeat(120), "f34c1488385346a55709ba056ddd08280dd4c6d6"),
            (b"a".repeat(1000), "291e9a6c66994949b57ba5e650361e98fc36b1ba"),
        ] {
            assert_eq!(hex(sha1(&data)), expected, "su {} byte", data.len());
        }
    }

    #[test]
    fn it_keeps_only_the_lines_that_are_logic() {
        let text = "import x from 'y'\n\
                    // un commento\n\
                    const valoreNumero = calcolaQualcosa(1, true)\n\
                    }\n\
                    short\n";
        let lines = significant_lines(text);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, 3);
    }

    #[test]
    fn a_block_comment_does_not_swallow_the_file() {
        let text = "/* apre\n   continua */\nconst valoreNumero = calcolaQualcosa(1, true)\n";
        assert_eq!(significant_lines(text).len(), 1);
        // e quello che apre e chiude sulla stessa riga non apre niente
        let inline = "/* tutto qui */\nconst valoreNumero = calcolaQualcosa(1, true)\n";
        assert_eq!(significant_lines(inline).len(), 1);
    }

    /// `export function` è una dichiarazione, cioè logica: scartarla rendeva
    /// invisibile il caso su cui la soglia è tarata.
    #[test]
    fn an_export_of_a_declaration_is_not_an_import() {
        assert!(is_import("import { a } from 'b'"));
        assert!(is_import("export * from './x'"));
        assert!(is_import("export { a } from './x'"));
        assert!(is_import("export type { A } from './x'"));
        assert!(!is_import("export function toStringArray(v) {"));
        assert!(!is_import("export const useThing = () => {}"));
        // Un alias di tipo è una dichiarazione: dopo `type` c'è uno spazio ma
        // non una graffa, e servono tutte e due perché sia un ri-export.
        assert!(!is_import("export type Alias = Record<string, number>"));
        assert!(!is_import("important('this is not an import')"));
        assert!(!is_import("exported = 1"));
    }

    #[test]
    fn the_family_is_the_last_segment_of_the_name() {
        assert_eq!(family_tag(Path::new("/x/candidates-bulk-bar.tsx")), "bar");
        assert_eq!(family_tag(Path::new("/x/alfa-columns.ts")), "columns");
        assert_eq!(family_tag(Path::new("/x/a.test.ts")), "test");
    }

    #[test]
    fn a_block_shorter_than_the_threshold_is_not_a_finding() {
        let a: Vec<(usize, String)> = (1..=10)
            .map(|i| (i, format!("const valoreNumero{i} = calcolaQualcosa({i}, true)")))
            .collect();
        let blocks = shared_blocks(&a, &a, 4);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].count, 10);
        assert!(shared_blocks(&a[..2], &a[..2], 4).is_empty());
    }

    /// Il minimo di sostanza in caratteri: quattro righe di intelaiatura non
    /// sono una copia, quattro righe di logica sì.
    #[test]
    fn four_lines_of_scaffolding_are_not_a_copy() {
        let thin: Vec<(usize, String)> =
            (1..=6).map(|i| (i, "const a = 1 //".to_string())).collect();
        assert!(shared_blocks(&thin, &thin, 4).is_empty());
    }

    #[test]
    fn the_signature_does_not_move_with_the_lines() {
        let root = Path::new("/tmp");
        let one = pair_signature(root, Path::new("/tmp/a.ts"), Path::new("/tmp/b.ts"), "ff");
        let other = pair_signature(root, Path::new("/tmp/b.ts"), Path::new("/tmp/a.ts"), "ff");
        assert_eq!(one, other, "la coppia è la stessa da entrambi i lati");
        assert!(one.ends_with("|ff"));
    }

    #[test]
    fn it_only_watches_the_extensions_it_can_read() {
        assert!(is_watched("/x/a.ts"));
        assert!(is_watched("/x/a.py"));
        assert!(!is_watched("/x/a.md"));
        assert!(!is_watched("/x/node_modules/a.ts"));
        assert!(!is_watched("/x/.claude/a.py"));
    }

    /// Ogni marcatore di commento vale da solo. Incrociarli a due a due — la
    /// mutazione che qui sopravviveva — spegne il riconoscimento di `#` e di
    /// `*` senza che il rapporto finale cambi di una riga.
    #[test]
    fn each_comment_marker_stands_on_its_own() {
        assert!(is_comment("// una riga di commento"));
        assert!(is_comment("# un commento di Python"));
        assert!(is_comment(" * la riga di mezzo di un blocco"));
        assert!(is_comment("/* apre un blocco */"));
        assert!(!is_comment("const value = 1 // in coda"));
    }

    /// I due filtri che si somigliano ma non coincidono: la sola punteggiatura,
    /// e la riga troppo corta. Il taglio è a **meno di** dodici caratteri, e
    /// dodici esatti restano.
    #[test]
    fn punctuation_is_not_logic_and_twelve_characters_are_enough() {
        assert!(is_trivial("  });  "));
        assert!(!is_trivial("const a = 1"));
        let text = "let x = 1234\nlet y = 123\n});});});});\n";
        assert_eq!(
            significant_lines(text),
            vec![(1, "let x = 1234".to_string())]
        );
    }

    /// La normalizzazione degli spazi è ciò che rende confrontabili due righe
    /// indentate diversamente: se cade, due copie identiche smettono di
    /// sembrarlo e il rilevatore tace.
    #[test]
    fn it_squeezes_every_run_of_whitespace_into_one_space() {
        assert_eq!(collapse_spaces("const a   =\t1"), "const a = 1");
        assert_eq!(collapse_spaces("uno"), "uno");
        assert_eq!(collapse_spaces(""), "");
    }

    /// Cartella o nome: bastano da soli. Chiederli tutti e due lascerebbe
    /// confrontare le prove col codice, che è il paragone che non insegna
    /// niente.
    #[test]
    fn a_test_file_is_recognised_by_the_folder_or_by_the_name() {
        assert!(is_test(Path::new("/x/__tests__/bulk-bar.tsx")));
        assert!(is_test(Path::new("/x/app/bulk-bar.test.tsx")));
        assert!(!is_test(Path::new("/x/app/bulk-bar.tsx")));
    }

    /// L'ordine è deliberato: senza, il taglio a `MAX_SIBLINGS` cade su file
    /// diversi a ogni esecuzione, perché `read_dir` non è alfabetico.
    #[test]
    fn it_walks_the_whole_tree_in_path_order() {
        let root = fixture_root("duplicazione-albero");
        write_file(&root.join("b/second.ts"), "uno");
        write_file(&root.join("a/first.ts"), "uno");
        write_file(&root.join("a/deep/third.md"), "uno");
        assert_eq!(
            walk(&root),
            vec![
                root.join("a/deep/third.md"),
                root.join("a/first.ts"),
                root.join("b/second.ts"),
            ]
        );
    }

    /// Le tre porte d'ingresso — famiglia, cartella, punto di riuso — e i
    /// quattro rifiuti. Ognuna entra da sola: un caso che le chieda tutte
    /// insieme non vede quale delle tre si è spenta.
    #[test]
    fn the_siblings_are_the_family_the_folder_and_the_reuse_points() {
        let root = fixture_root("duplicazione-fratelli");
        let target = root.join("app/candidates-bulk-bar.tsx");
        write_file(&target, "uno");
        write_file(&root.join("altro/external-users-bulk-bar.tsx"), "uno");
        write_file(&root.join("app/list-view.tsx"), "uno");
        write_file(&root.join("lib/format-money.ts"), "uno");
        write_file(&root.join("altro/user-profile-card.tsx"), "uno");
        write_file(&root.join("app/appunti.md"), "uno");
        write_file(&root.join("app/candidates-bulk-bar.test.tsx"), "uno");
        write_file(&root.join("node_modules/candidates-bulk-bar.tsx"), "uno");

        let mut found: Vec<String> = siblings(&target, &root)
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![
                "altro/external-users-bulk-bar.tsx",
                "app/list-view.tsx",
                "lib/format-money.ts",
            ],
            "il bersaglio, le prove, l'estensione muta e node_modules restano fuori"
        );
    }

    /// Il confronto è per carattere, non per componente: è così che si sceglie
    /// il donatore più probabile quando i candidati sono più di trecento.
    #[test]
    fn the_nearest_sibling_shares_the_longest_prefix() {
        assert_eq!(common_prefix_len("/a/b/uno.ts", "/a/b/due.ts"), 5);
        assert_eq!(common_prefix_len("/a/b/uno.ts", "/z/uno.ts"), 1);
        assert_eq!(common_prefix_len("", "/a"), 0);
    }

    /// I passi interni del confronto: dove comincia il blocco qui, dove
    /// comincia là, su quale corpo si calcola l'impronta e quando la corsa
    /// finisce. Un caso che guardi solo la lunghezza non ne distingue nessuno
    /// dei quattro — i due elenchi hanno code diverse apposta, perché
    /// altrimenti il blocco finirebbe insieme ai file e la condizione di fine
    /// non verrebbe mai esercitata.
    #[test]
    fn a_block_carries_both_starting_lines_and_the_body_it_hashed() {
        let body = shared_body(5);
        let shared: Vec<&str> = body.lines().collect();
        let mut here: Vec<(usize, String)> = vec![
            (10, "const firstLineOnlyHere = prepareTheContext(1)".to_string()),
            (11, "const thirdLineOnlyHere = prepareTheContext(2)".to_string()),
        ];
        let mut there: Vec<(usize, String)> = vec![
            (20, "const alphaOnlyThere = prepareOtherContext(1)".to_string()),
            (21, "const betaOnlyThere = prepareOtherContext(2)".to_string()),
            (22, "const gammaOnlyThere = prepareOtherContext(3)".to_string()),
        ];
        for (i, line) in shared.iter().enumerate() {
            here.push((12 + i, (*line).to_string()));
            there.push((23 + i, (*line).to_string()));
        }
        here.push((17, "const tailOnlyHere = closeTheContext(9)".to_string()));
        there.push((28, "const tailOnlyThere = closeOtherContext(9)".to_string()));

        let blocks = shared_blocks(&here, &there, MIN_LINES);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].count, 5);
        assert_eq!(blocks[0].line_here, 12);
        assert_eq!(blocks[0].line_there, 23);
        assert_eq!(blocks[0].fingerprint, fingerprint(&shared));
    }

    /// L'impronta deve restare quella di `hashlib.sha1`: la linea di base
    /// congelata è un file solo e la leggono in due. Oracolo:
    /// `python3 -c "import hashlib; print(hashlib.sha1(b'uno\ndue').hexdigest())"`.
    #[test]
    fn the_fingerprint_is_the_first_sixteen_digits_of_the_joined_body() {
        assert_eq!(fingerprint(&["uno", "due"]), "e9697974cd37f503");
    }

    /// Dentro la radice la firma è fatta di percorsi relativi, in ordine
    /// alfabetico. Sono i due passi che il caso a percorsi inesistenti non
    /// tocca: là `strip_prefix` fallisce e si ricade sui percorsi interi.
    #[test]
    fn the_signature_is_relative_to_the_root_and_alphabetical() {
        let root = fixture_root("duplicazione-firma");
        write_file(&root.join("app/uno.ts"), "uno");
        write_file(&root.join("lib/due.ts"), "due");
        let signature = pair_signature(
            &root,
            &root.join("lib/due.ts"),
            &root.join("app/uno.ts"),
            "ff00",
        );
        assert_eq!(signature, "app/uno.ts|lib/due.ts|ff00");
    }

    /// Due radici non possono condividere la linea di base: il debito congelato
    /// in un albero di lavoro non vale nell'altro, e mescolarli darebbe silenzi
    /// falsi.
    #[test]
    fn each_root_gets_its_own_baseline_under_the_state_folder() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        assert_eq!(state_dir(), home.join(".claude/state/duplicazione"));

        let path = baseline_path(&fixture_root("duplicazione-linea-uno"));
        assert_eq!(path.parent().unwrap(), state_dir());
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name.len(), 12 + ".json".len());
        assert!(name[..12].chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(path, baseline_path(&fixture_root("duplicazione-linea-due")));
    }

    /// Le coppie congelate si leggono dal file di **quella** radice; un file
    /// mancante vale «niente di congelato», e il rilevatore parla di più
    /// invece che di meno.
    #[test]
    fn the_frozen_pairs_come_from_the_file_of_that_root() {
        let root = fixture_root("duplicazione-congelate");
        assert!(load_baseline(&root).is_empty());

        // Il file nasce nella cartella di stato vera: l'impronta viene da una
        // radice temporanea, quindi non può collidere con quella di un repo.
        // Ed è per la stessa ragione che dentro il perimetro non si misura: la
        // sede vera è proprio quella che il perimetro nega.
        let path = baseline_path(&root);
        if let Some(why) = hook_io::testing::writes_denied_in(path.parent().unwrap()) {
            eprintln!("{why}");
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _cleanup = RemoveOnDrop(path.clone());
        std::fs::write(&path, r#"{"coppie": ["app/uno.ts|lib/due.ts|ff00", "a|b|11"]}"#).unwrap();

        let frozen = load_baseline(&root);
        assert_eq!(frozen.len(), 2);
        assert!(frozen.contains("app/uno.ts|lib/due.ts|ff00"));
        assert!(frozen.contains("a|b|11"));
    }

    /// La catena intera, dal file al riscontro. È il caso che manca a un gancio
    /// che non trova mai niente: senza, tacere passerebbe per star bene.
    #[test]
    fn it_reports_the_copied_block_and_the_lines_it_covers() {
        let root = fixture_root("duplicazione-rapporto");
        let body = shared_body(5);
        let target = root.join("app/candidates-bulk-bar.tsx");
        write_file(
            &target,
            &format!(
                "import {{ x }} from './y'\n\
                 const firstLineOnlyHere = prepareTheContext(1)\n\
                 const thirdLineOnlyHere = prepareTheContext(2)\n\
                 {body}\n"
            ),
        );
        let sibling = root.join("app/external-users-bulk-bar.tsx");
        write_file(
            &sibling,
            &format!("const alphaOnlyThere = prepareOtherContext(1)\n{body}\n"),
        );

        let (covered, findings) = report(&target, &root, MIN_LINES, &HashSet::new(), false);
        assert_eq!(covered, 5, "cinque righe del bersaglio stanno già altrove");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].sibling, sibling);
        assert_eq!(findings[0].line_here, 4);
        assert_eq!(findings[0].line_there, 2);
        assert_eq!(findings[0].count, 5);

        // La firma congelata zittisce quel riscontro, e con lui le righe che
        // risultavano coperte.
        let known: HashSet<String> = [findings[0].signature.clone()].into_iter().collect();
        let (covered, findings) = report(&target, &root, MIN_LINES, &known, false);
        assert_eq!(covered, 0);
        assert!(findings.is_empty());
    }

    /// Il taglio è «meno di `minimum` righe», non «al più»: un file di
    /// esattamente quattro righe significative si guarda ancora.
    #[test]
    fn a_file_with_exactly_the_minimum_number_of_lines_is_still_examined() {
        let root = fixture_root("duplicazione-minimo");
        let body = shared_body(MIN_LINES);
        let target = root.join("app/candidates-bulk-bar.tsx");
        write_file(&target, &format!("{body}\n"));
        write_file(
            &root.join("app/external-users-bulk-bar.tsx"),
            &format!("{body}\n"),
        );
        let (covered, findings) = report(&target, &root, MIN_LINES, &HashSet::new(), false);
        assert_eq!(covered, MIN_LINES);
        assert_eq!(findings.len(), 1);
    }

    /// Il taglio ai primi sei vale per l'avviso, non per chi congela: là un
    /// riscontro dimenticato resterebbe a suonare per sempre.
    #[test]
    fn the_notice_stops_at_six_findings_and_the_freeze_sees_them_all() {
        let root = fixture_root("duplicazione-taglio");
        let body = shared_body(5);
        let target = root.join("app/candidates-bulk-bar.tsx");
        write_file(&target, &format!("{body}\n"));
        for i in 1..=7 {
            write_file(
                &root.join(format!("app/copy{i}-bulk-bar.tsx")),
                &format!("{body}\n"),
            );
        }
        let (_, short) = report(&target, &root, MIN_LINES, &HashSet::new(), false);
        assert_eq!(short.len(), 6);
        let (_, full) = report(&target, &root, MIN_LINES, &HashSet::new(), true);
        assert_eq!(full.len(), 7);
    }

    /// Un byte non valido non ferma il rilevatore: il file si legge lo stesso,
    /// col carattere di sostituzione al posto del byte.
    #[test]
    fn a_file_with_invalid_bytes_is_read_anyway() {
        let root = fixture_root("duplicazione-byte");
        let path = root.join("strano.ts");
        std::fs::write(&path, b"const a = 1\n\xff\n").unwrap();
        assert_eq!(read_lossy(&path).unwrap(), "const a = 1\n\u{fffd}\n");
        assert!(read_lossy(&root.join("non-esiste.ts")).is_err());
    }

    /// La risalita al deposito: da un file si parte dalla sua cartella, da una
    /// cartella da sé stessa. Dove c'è `src` la radice è quella, perché è lì
    /// che sta il codice con cui confrontarsi.
    #[test]
    fn the_root_is_the_repository_or_its_source_folder() {
        let root = fixture_root("duplicazione-radice");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        write_file(&root.join("app/uno.ts"), "uno");
        assert_eq!(root_of(&root.join("app/uno.ts")), root);
        assert_eq!(root_from_dir(&root.join("app")), root);

        std::fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(root_of(&root.join("app/uno.ts")), root.join("src"));
        assert_eq!(root_from_dir(&root.join("app")), root.join("src"));
    }

    /// L'avviso dice il file, le righe da entrambi i lati e la via d'uscita.
    #[test]
    fn the_notice_names_the_file_the_lines_and_the_way_out() {
        let findings = [Finding {
            sibling: PathBuf::from("/repo/app/external-users-bulk-bar.tsx"),
            line_here: 12,
            line_there: 23,
            count: 5,
            signature: "app/uno.ts|app/due.ts|ff00".to_string(),
        }];
        let text = post_text(Path::new("/repo/app/candidates-bulk-bar.tsx"), 5, &findings);
        assert!(text.starts_with("codice ricopiato in candidates-bulk-bar.tsx: 5 righe"));
        assert!(
            text.contains("· righe 12-16 = /repo/app/external-users-bulk-bar.tsx:23 (5 righe)"),
            "{text}"
        );
        assert!(text.contains("estrai la parte comune"));
    }

    /// L'avviso alla nascita elenca solo i veri omonimi: i vicini di cartella
    /// servono al confronto in `post`, ma in un elenco «ecco la tua famiglia»
    /// sono rumore.
    #[test]
    fn the_birth_notice_lists_only_the_namesakes() {
        let root = fixture_root("duplicazione-nascita");
        write_file(
            &root.join("app/external-users-bulk-bar.tsx"),
            "const a = 1\nconst b = 2\n",
        );
        write_file(&root.join("app/list-view.tsx"), "const c = 3\n");

        let text = pre_text(&root.join("app/candidates-bulk-bar.tsx"), &root)
            .expect("la famiglia esiste");
        assert!(
            text.starts_with("stai creando candidates-bulk-bar.tsx; la sua famiglia esiste già:"),
            "{text}"
        );
        assert!(text.contains("external-users-bulk-bar.tsx  (2 righe)"), "{text}");
        assert!(!text.contains("list-view.tsx"), "{text}");
        assert_eq!(pre_text(&root.join("app/orphan-alone-widget.tsx"), &root), None);
    }

    /// Una radice di prova **già risolta**: sotto `/tmp` il percorso vero passa
    /// da `/private`, e `siblings` confronta cartelle risolte con cartelle
    /// camminate — con una radice non risolta nessun fratello risulta vicino.
    fn fixture_root(tag: &str) -> PathBuf {
        hook_io::testing::test_dir(tag).canonicalize().unwrap()
    }

    fn write_file(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Righe come quelle che si ricopiano davvero: abbastanza lunghe da
    /// superare il minimo di sostanza, così i casi provano la soglia in righe e
    /// non quella in caratteri.
    fn shared_body(count: usize) -> String {
        (1..=count)
            .map(|i| format!("const rowTotalNumber{i} = computeRowAmountFor({i}, true)"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// La linea di base di prova vive nella cartella di stato vera: va tolta
    /// anche quando il caso cade a metà.
    struct RemoveOnDrop(PathBuf);

    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
