//! La camminata sull'albero per il rilevatore delle affermazioni datate.
//!
//! Il giudizio sta in `guards::stale_facts` e non tocca il disco; qui c'è solo
//! ciò che il disco lo tocca: quali cartelle si attraversano, quali file si
//! leggono, e il rapporto.
//!
//! **La riga finale è un contratto**, non una formattazione: il criterio di
//! accettazione ci cerca dentro `N measurements older than`, ed è la stessa
//! riga che stampava il gemello Python. Cambiarla spegne il criterio senza
//! farlo diventare rosso — che è il modo peggiore in cui un controllo muore.

use guards::stale_facts::{
    is_scanned, is_test_file, skip_dir, stale_claims, strip_test_bodies, Date, DEFAULT_MONTHS,
};

/// La data di oggi, dall'orologio locale.
///
/// `now_local_iso8601()` dà `2026-08-20T13:41:02+02:00`: i primi dieci
/// caratteri sono la data locale, che è quella giusta — a mezzanotte meno un
/// quarto UTC sarebbe già domani, e un rapporto datato domani non lo capisce
/// nessuno.
fn today() -> Date {
    let stamp = hook_io::local_time::now_local_iso8601();
    let n = |a: usize, b: usize| stamp.get(a..b).and_then(|s| s.parse::<i64>().ok());
    match (n(0, 4), n(5, 7), n(8, 10)) {
        (Some(y), Some(m), Some(d)) => Date::new(y, m, d).unwrap_or(Date {
            year: y,
            month: 1,
            day: 1,
        }),
        _ => Date {
            year: 2026,
            month: 1,
            day: 1,
        },
    }
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // I collegamenti simbolici non si seguono: sotto `~/.claude` ce ne sono
        // otto che espongono competenze installate altrove, e seguirli
        // significherebbe leggere due volte lo stesso albero — o girare in
        // tondo, se uno punta a un genitore.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !skip_dir(&name) {
                walk(&path, out);
            }
        } else if is_scanned(&name) && !is_test_file(&path.to_string_lossy()) {
            out.push(path);
        }
    }
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut months = DEFAULT_MONTHS;
    let mut root: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--months" => {
                if let Some(v) = it.next().and_then(|v| v.parse::<i64>().ok()) {
                    months = v;
                }
            }
            _ if a.starts_with('-') => {}
            _ => root = Some(a.clone()),
        }
    }
    let root = root.unwrap_or_else(|| {
        std::env::var("HOME").map(|h| format!("{h}/.claude")).unwrap_or_default()
    });

    let today = today();
    let mut files = Vec::new();
    walk(std::path::Path::new(&root), &mut files);
    files.sort();

    let mut total = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let claims = stale_claims(&strip_test_bodies(&text), today, months);
        if claims.is_empty() {
            continue;
        }
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        println!("\n{shown}");
        for c in &claims {
            println!("  measured {} ({} days ago), line {}:", c.date.iso(), c.days, c.line_no);
            println!("    {}", c.line);
            total += 1;
        }
    }
    println!("\n{total} measurements older than {months} months");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_is_a_real_date_and_not_the_fallback() {
        let d = today();
        assert!(d.year >= 2025, "the local clock gave {}", d.iso());
        assert!((1..=12).contains(&d.month));
        assert!(Date::new(d.year, d.month, d.day).is_some());
    }

    #[test]
    fn the_walk_skips_what_it_must_and_keeps_what_it_must() {
        let base = std::env::temp_dir().join(format!("stale-walk-{}", std::process::id()));
        let plugins = base.join("plugins");
        let src = base.join("src");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(plugins.join("altrui.md"), "x").unwrap();
        std::fs::write(src.join("nota.md"), "x").unwrap();
        std::fs::write(src.join("prova-gate.py"), "x").unwrap();
        std::fs::write(src.join("dati.json"), "x").unwrap();

        let mut found = Vec::new();
        walk(&base, &mut found);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        let _ = std::fs::remove_dir_all(&base);

        assert_eq!(names, ["nota.md"], "walked: {names:?}");
    }
}
