//! Le righe che una lavorazione ha toccato, lette dal diff.
//!
//! È la parte che toglie la scelta di mano a chi ha scritto il codice: il
//! perimetro non è «i punti che mi sembrano delicati», è l'insieme delle righe
//! che il diff dichiara nuove o cambiate. Si legge `git diff -U0`, che di
//! contorno non ne dà: ogni riga marcata `+` è una riga da guastare.

/// Un file toccato e le sue righe nuove, 1-based sul contenuto attuale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedFile {
    pub path: String,
    pub lines: Vec<usize>,
}

/// Legge un diff unificato e restituisce le righe del lato nuovo.
///
/// I file cancellati non compaiono: non c'è più niente da guastare. Il
/// percorso è quello del lato `+++`, cioè il nome dopo un'eventuale rinomina.
pub fn parse_unified_diff(text: &str) -> Vec<TouchedFile> {
    let mut files: Vec<TouchedFile> = Vec::new();
    let mut current: Option<TouchedFile> = None;
    let mut next_new_line: usize = 0;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            if let Some(file) = current.take() {
                if !file.lines.is_empty() {
                    files.push(file);
                }
            }
            let path = rest.trim();
            if path == "/dev/null" {
                continue;
            }
            // `+++ b/src/x.ts` — il prefisso `b/` è di git, non del progetto.
            let path = path.strip_prefix("b/").unwrap_or(path);
            current = Some(TouchedFile {
                path: path.to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            // `@@ -12,0 +13,4 @@ coda che non conta`
            if let Some(plus) = rest.split('+').nth(1) {
                let span = plus.split(' ').next().unwrap_or("");
                let mut parts = span.split(',');
                let start: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
                next_new_line = start;
            }
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if let Some(file) = current.as_mut() {
            if let Some(added) = line.strip_prefix('+') {
                let _ = added;
                file.lines.push(next_new_line);
                next_new_line += 1;
            } else if line.starts_with(' ') {
                next_new_line += 1;
            }
            // Una riga tolta non muove il conto del lato nuovo.
        }
    }
    if let Some(file) = current.take() {
        if !file.lines.is_empty() {
            files.push(file);
        }
    }
    files
}

/// Se il percorso è un file di prova: guastare una prova non dice niente sui
/// controlli, dice solo che la prova era là.
pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.contains("/test/")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name == "tests.rs"
        || name == "conftest.py"
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/src/a.ts b/src/a.ts\n\
index 111..222 100644\n\
--- a/src/a.ts\n\
+++ b/src/a.ts\n\
@@ -10 +10 @@ export function f() {\n\
-  const old = 1;\n\
+  const fresh = 1;\n\
@@ -20,0 +21,2 @@\n\
+  const other = 2;\n\
+  const third = 3;\n\
diff --git a/src/b.ts b/src/b.ts\n\
--- a/src/b.ts\n\
+++ b/src/b.ts\n\
@@ -5,0 +6 @@\n\
+  return null;\n";

    #[test]
    fn the_new_side_lines_come_out_per_file() {
        let files = parse_unified_diff(SAMPLE);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.ts");
        assert_eq!(files[0].lines, vec![10, 21, 22]);
        assert_eq!(files[1].path, "src/b.ts");
        assert_eq!(files[1].lines, vec![6]);
    }

    /// Un file cancellato non ha righe nuove: se comparisse, il giro
    /// proverebbe a guastare un file che non c'è.
    #[test]
    fn a_deleted_file_does_not_appear() {
        let diff = "diff --git a/src/gone.ts b/src/gone.ts\n\
--- a/src/gone.ts\n\
+++ /dev/null\n\
@@ -1,2 +0,0 @@\n\
-a\n\
-b\n";
        assert!(parse_unified_diff(diff).is_empty());
    }

    /// Una riga tolta non fa avanzare il conto del lato nuovo: contarla
    /// sposterebbe di uno ogni guasto del pezzo successivo.
    #[test]
    fn removed_lines_do_not_move_the_new_side_counter() {
        let diff = "--- a/x.ts\n+++ b/x.ts\n@@ -1,3 +1,2 @@\n a\n-b\n-c\n+d\n";
        let files = parse_unified_diff(diff);
        assert_eq!(files[0].lines, vec![2]);
    }

    #[test]
    fn a_file_with_no_added_lines_is_dropped() {
        let diff = "--- a/x.ts\n+++ b/x.ts\n@@ -1,2 +1,1 @@\n a\n-b\n";
        assert!(parse_unified_diff(diff).is_empty());
    }

    #[test]
    fn test_files_are_recognised() {
        assert!(is_test_path("tests/unit/graph.test.ts"));
        assert!(is_test_path("src/services/graph.spec.ts"));
        assert!(is_test_path("crates/guards/tests/mutants.rs"));
        assert!(is_test_path("crates/ledger/src/tests.rs"));
        assert!(!is_test_path("src/services/graph-resolution.ts"));
        assert!(!is_test_path("crates/guasti/src/mutations.rs"));
    }
}
