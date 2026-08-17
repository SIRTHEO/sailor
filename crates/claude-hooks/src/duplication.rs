//! La parte con stato del rilevatore di copie: il registro degli avvisi dati.
//!
//! Il giudizio sta in `guards::duplication`. Qui c'è solo ciò che ricorda fra
//! una chiamata e l'altra, e il flusso delle due fasi.

use guards::duplication as dup;
use hook_io::HookInput;
use std::path::Path;

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
