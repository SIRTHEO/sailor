//! La parte con stato del rilevatore di copie: il registro degli avvisi dati.
//!
//! Il giudizio sta in `guards::duplication`. Qui c'è solo ciò che ricorda fra
//! una chiamata e l'altra, e il flusso delle due fasi.

use guards::duplication as dup;
use hook_io::HookInput;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Vero se dell'elenco dei fratelli di questo file si è già parlato.
///
/// Segna il percorso e risponde com'era **prima**: la prima chiamata dice no e
/// lascia parlare, la seconda dice sì e lascia scrivere. Un avviso che non
/// ricorda di averlo già dato blocca all'infinito — difetto trovato all'uso il
/// 12/08/2026, non nelle prove.
fn already_announced(path: &str) -> bool {
    let dir = dup::state_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return true; // se non si può ricordare, non si blocca
    }
    let registry = dir.join("annunciati.txt");
    let key = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(path).to_path_buf())
        .to_string_lossy()
        .into_owned();

    let seen: Vec<String> = std::fs::read_to_string(&registry)
        .map(|t| t.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();
    if seen.contains(&key) {
        return true;
    }
    // Si tiene corta: è memoria di lavoro, non un archivio. Le ultime 400 più
    // quella nuova, come l'originale.
    let keep = seen.len().saturating_sub(400);
    let mut lines: Vec<&str> = seen[keep..].iter().map(|s| s.as_str()).collect();
    lines.push(&key);
    if std::fs::write(&registry, format!("{}\n", lines.join("\n"))).is_err() {
        return true;
    }
    false
}

/// Il gancio intero. `phase` è `pre` o `post`.
pub fn run(input: &HookInput, phase: &str) -> i32 {
    let empty = serde_json::json!({});
    let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
    let path = tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if path.is_empty() || !dup::is_watched(path) {
        return 0;
    }
    let target = Path::new(path);
    let root = dup::root_of(target);
    let minimum = std::env::var("DUPLICAZIONE_RIGHE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(dup::MIN_LINES);

    if phase == "pre" {
        // Solo alla nascita di un file: su un file che esiste già il modello ha
        // davanti il contenuto, e l'elenco dei fratelli è rumore.
        if input.tool_name.as_deref() != Some("Write") || target.exists() {
            return 0;
        }
        let Some(message) = dup::pre_text(target, &root) else {
            return 0;
        };
        if already_announced(path) {
            return 0;
        }
        eprintln!("{message}");
        return 2;
    }

    let known = dup::load_baseline(&root);
    let (covered, findings) = dup::report(target, &root, minimum, &known, false);
    if findings.is_empty() {
        return 0;
    }
    eprintln!("{}", dup::post_text(target, covered, &findings));
    2
}

/// Congela il debito di copie che esiste **adesso** sotto una radice.
///
/// PERCHE' ESISTE. Il rilevatore parla della duplicazione nuova, non di quella
/// che ha trovato: senza una linea di base, in un albero di lavoro appena creato
/// rimprovera per codice che chi ci scrive non ha mai toccato. La regola
/// `rules/duplicazione.md` la prescrive da sempre — ma citava
/// `duplication.py --congela`, e quel Python e' stato portato in Rust il
/// 17/08/2026 senza questo verbo. Dal 19/08 nessuno poteva piu' congelare
/// niente, e il gancio parlava di debito preesistente in ogni copia nuova: il
/// caso e' arrivato da una sessione che ci stava lavorando dentro.
///
/// `report(..., full = true)` esisteva gia' per questo — il suo commento dice
/// «serve a congelare» — e nessuno la chiamava.
pub fn freeze(root: Option<&str>) -> i32 {
    let start = match root {
        Some(r) => PathBuf::from(r),
        None => std::env::current_dir().unwrap_or_default(),
    };
    if !start.exists() {
        eprintln!("congelamento: la radice {} non esiste", start.display());
        return 1;
    }
    let root = if start.is_dir() {
        dup::root_from_dir(&start)
    } else {
        dup::root_of(&start)
    };
    let minimum = min_lines();

    let files = watched_files(&root);
    let empty: HashSet<String> = HashSet::new();
    let mut pairs: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for file in &files {
        // `full = true`: un riscontro tagliato via qui resterebbe a suonare per
        // sempre, perche' non entrerebbe mai nella linea di base.
        let (_, findings) = dup::report(file, &root, minimum, &empty, true);
        for f in findings {
            if seen.insert(f.signature.clone()) {
                pairs.push(f.signature);
            }
        }
    }
    pairs.sort();

    let dir = dup::state_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("congelamento: non posso scrivere in {}: {e}", dir.display());
        return 1;
    }
    let path = dup::baseline_path(&root);
    let payload = serde_json::json!({
        "radice": root.display().to_string(),
        "file": files.len(),
        "coppie": pairs,
    });
    if let Err(e) = std::fs::write(&path, format!("{payload:#}\n")) {
        eprintln!("congelamento: non posso scrivere {}: {e}", path.display());
        return 1;
    }
    println!(
        "congelate {} coppie su {} file sorvegliati di {}",
        pairs.len(),
        files.len(),
        root.display()
    );
    println!("linea di base: {}", path.display());
    0
}

/// L'elenco di cio' che e' gia' ricopiato, senza congelare niente.
///
/// La regola lo prescrive come `--debito` da sempre; nel porto in Rust non
/// esisteva, come `--congela` e `--scan`. Chi legge la regola trovava tre
/// comandi su quattro che non rispondevano.
pub fn debt(root: Option<&str>) -> i32 {
    let start = match root {
        Some(r) => PathBuf::from(r),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = if start.is_dir() {
        dup::root_from_dir(&start)
    } else {
        dup::root_of(&start)
    };
    let minimum = min_lines();
    let known = dup::load_baseline(&root);
    let mut rows: Vec<(usize, String)> = Vec::new();
    let mut total = 0;
    for file in watched_files(&root) {
        let (covered, findings) = dup::report(&file, &root, minimum, &known, true);
        if findings.is_empty() {
            continue;
        }
        total += findings.len();
        let name = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
        let worst = findings.iter().map(|f| f.count).max().unwrap_or(0);
        rows.push((
            worst,
            format!("{name}: {covered} righe coperte, {} riscontri", findings.len()),
        ));
    }
    rows.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    for (_, line) in rows.iter().take(40) {
        println!("{line}");
    }
    println!(
        "\n{total} riscontri fuori dalla linea di base in {}",
        root.display()
    );
    if !known.is_empty() {
        println!("({} coppie gia' congelate non compaiono)", known.len());
    }
    i32::from(total > 0)
}

/// Il rapporto su un file solo, come lo vedrebbe il gancio.
pub fn scan(path: Option<&str>) -> i32 {
    let Some(path) = path else {
        eprintln!("uso: claude-hooks duplication --scan <file>");
        return 64;
    };
    let target = PathBuf::from(path);
    if !target.is_file() {
        eprintln!("{path} non e' un file");
        return 1;
    }
    let root = dup::root_of(&target);
    let known = dup::load_baseline(&root);
    let (covered, findings) = dup::report(&target, &root, min_lines(), &known, true);
    if findings.is_empty() {
        println!("{path}: nessuna copia fuori dalla linea di base");
        return 0;
    }
    println!("{}", dup::post_text(&target, covered, &findings));
    1
}

fn min_lines() -> usize {
    std::env::var("DUPLICAZIONE_RIGHE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(dup::MIN_LINES)
}

/// I file che il rilevatore guarda sotto una radice.
///
/// Si cammina a mano invece di chiedere a git: un albero di lavoro appena creato
/// ha file non ancora tracciati, e sono proprio quelli su cui il gancio parla.
fn watched_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                // Le cartelle col punto e gli alberi rigenerati non si visitano:
                // `is_watched` li scarterebbe file per file, ma dopo averli letti.
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                stack.push(p);
            } else if dup::is_watched(&p.to_string_lossy()) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Il caso di autoverifica: due file identici devono produrre un riscontro, due
/// file diversi no. Gira su una cartella temporanea, non sul disco vero.
pub fn self_check() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("duplication-check-{}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join(".git")).map_err(|e| e.to_string())?;

    let body: String = (0..12)
        .map(|i| format!("  const valoreNumero{i} = calcolaQualcosa({i}, true)\n"))
        .collect();
    std::fs::write(src.join("alfa-columns.ts"), format!("function alfa() {{\n{body}}}\n"))
        .map_err(|e| e.to_string())?;
    std::fs::write(src.join("beta-columns.ts"), format!("function beta() {{\n{body}}}\n"))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        src.join("solo-columns.ts"),
        "function solo() {\n  return unaCosaCompletamenteDiversa(42)\n}\n",
    )
    .map_err(|e| e.to_string())?;

    let known = Default::default();
    let (covered, findings) = dup::report(&src.join("beta-columns.ts"), &src, dup::MIN_LINES, &known, false);
    let (_, none) = dup::report(&src.join("solo-columns.ts"), &src, dup::MIN_LINES, &known, false);
    let _ = std::fs::remove_dir_all(&dir);

    if covered < 12 || findings.is_empty() {
        return Err(format!("non vede il blocco ricopiato ({covered} righe)"));
    }
    if !none.is_empty() {
        return Err("segnala un file che non somiglia a nessuno".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il congelamento si prova come si prova un freno: prima il gancio deve
    /// parlare, poi — congelato — deve tacere sullo stesso file.
    #[test]
    fn freezing_silences_the_debt_that_was_already_there() {
        let dir = std::env::temp_dir().join(format!("duplication-freeze-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();

        let body: String = (0..12)
            .map(|i| format!("  const valueNumber{i} = computeSomething({i}, true)\n"))
            .collect();
        std::fs::write(src.join("alfa-columns.ts"), format!("function alfa() {{\n{body}}}\n"))
            .unwrap();
        std::fs::write(src.join("beta-columns.ts"), format!("function beta() {{\n{body}}}\n"))
            .unwrap();

        let target = src.join("beta-columns.ts");
        let root = dup::root_of(&target);
        let empty: HashSet<String> = HashSet::new();
        let (_, before) = dup::report(&target, &root, dup::MIN_LINES, &empty, false);
        assert!(!before.is_empty(), "senza linea di base deve parlare");

        assert_eq!(freeze(Some(&dir.to_string_lossy())), 0);

        let known = dup::load_baseline(&root);
        assert!(!known.is_empty(), "la linea di base e' vuota");
        let (_, after) = dup::report(&target, &root, dup::MIN_LINES, &known, false);
        assert!(after.is_empty(), "congelato, deve tacere: {after:?}");

        let _ = std::fs::remove_file(dup::baseline_path(&root));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
