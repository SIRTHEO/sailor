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
    all_claims, categorize, is_scanned, is_test_file, skip_dir, stale_claims, strip_test_bodies,
    Category, Date, DEFAULT_MONTHS,
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

/// L'ultima data in cui git registra un cambiamento del file: parla del
/// contenuto, non di un tocco che lo sposta senza cambiarlo (una copia, un
/// `touch`). `None` se il file non è tracciato, o non c'è repository.
fn git_last_modified(path: &std::path::Path) -> Option<Date> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_str()?;
    let out = std::process::Command::new("git")
        .args(["log", "-1", "--format=%cs", "--", name])
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stamp = String::from_utf8_lossy(&out.stdout);
    let mut parts = stamp.trim().split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    Date::new(y, m, d)
}

/// Il ripiego per i file che git non traccia: la data del filesystem, che
/// cambia per motivi che non c'entrano con la misura (una copia, un
/// checkout) — meno fidata, ma è l'unica che resta.
fn fs_modified(path: &std::path::Path) -> Option<Date> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let secs = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(Date::from_days(secs.div_euclid(86_400)))
}

/// La data di modifica del file, e se viene da git o dal ripiego sul
/// filesystem — il chiamante lo dice nel rapporto, perché le due fonti non
/// sono ugualmente fidate.
fn file_modified(path: &std::path::Path) -> (Option<Date>, bool) {
    if let Some(d) = git_last_modified(path) {
        return (Some(d), true);
    }
    (fs_modified(path), false)
}

/// Vero se la riga è il riassunto di un commit nell'uscita `--oneline`
/// (`<hash> <soggetto>`), falso se è una riga di diff (`+`, `-`, `@@`,
/// `diff --git`, `index …`). Basta il primo token: solo l'hash è fatto di
/// soli caratteri esadecimali, ed è lungo almeno sette cifre.
fn is_commit_summary(line: &str) -> bool {
    let Some(hash) = line.split_whitespace().next() else { return false };
    hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// Quanto è cambiato **il punto preciso della misura** da allora: commit che
/// hanno toccato quella riga (o le sue vicine, per un'affermazione scritta su
/// più righe) e quante ne hanno mosse.
///
/// Il primo tentativo contava il file intero con `--numstat`, ed era cieco
/// alla differenza fra «questa frase è stata riscritta» e «qualcuno ha
/// aggiunto mille righe altrove nello stesso file» — misurato su
/// `docs/libro-di-bordo.md`, un registro che cresce per aggiunta: la riga
/// dove nasce «un permesso che Theo aveva scelto…» non è mai stata toccata,
/// eppure il file intero segnava 5.180 righe mosse e la metteva in cima.
/// `git log -L` segue la riga nella cronologia, non il file: qui pesa solo
/// chi ha davvero scritto sopra quella frase.
fn change_near(path: &std::path::Path, date: Date, line_no: usize) -> (usize, usize) {
    let Some(dir) = path.parent() else { return (0, 0) };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return (0, 0) };
    // La finestra parte dal giorno **dopo** la misura, non da quel giorno: un
    // commit dello stesso giorno può essere proprio quello che ha scritto la
    // misura. L'ora esplicita non è un vezzo: `--since=2026-08-22` da solo,
    // quando «2026-08-22» è oggi, git lo legge come «da adesso», non «da
    // mezzanotte» — misurato sul repository vero, un commit delle 01:52
    // spariva da un `--since` lanciato alle 02:04 dello stesso giorno.
    let since = format!("{} 00:00:00", Date::from_days(date.days() + 1).iso());
    // Tre righe di margine: la frase che dichiara la misura si scrive spesso
    // su più righe, e la parola-chiave non cade sempre sulla prima.
    let start = line_no.saturating_sub(3).max(1);
    let end = line_no + 3;
    let Ok(out) = std::process::Command::new("git")
        .args([
            "log",
            "--oneline",
            &format!("--since={since}"),
            &format!("-L{start},{end}:{name}"),
        ])
        .current_dir(dir)
        .output()
    else {
        return (0, 0);
    };
    if !out.status.success() {
        return (0, 0);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut commits = 0usize;
    let mut lines = 0usize;
    for line in text.lines() {
        if is_commit_summary(line) {
            commits += 1;
        } else if (line.starts_with('+') && !line.starts_with("+++"))
            || (line.starts_with('-') && !line.starts_with("---"))
        {
            lines += 1;
        }
    }
    (commits, lines)
}

/// Una misura sospetta, col peso che dice quanto guardarla per prima: più
/// righe mosse da allora, più commit — non solo *se* il file è cambiato.
struct Suspect {
    shown: String,
    claim: guards::stale_facts::Claim,
    commits: usize,
    lines: usize,
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut months = DEFAULT_MONTHS;
    let mut days: Option<i64> = None;
    let mut root: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--months" => {
                if let Some(v) = it.next().and_then(|v| v.parse::<i64>().ok()) {
                    months = v;
                }
            }
            "--days" => {
                days = it.next().and_then(|v| v.parse::<i64>().ok());
            }
            _ if a.starts_with('-') => {}
            _ => root = Some(a.clone()),
        }
    }
    let root = root.unwrap_or_else(|| {
        std::env::var("HOME").map(|h| format!("{h}/.claude")).unwrap_or_default()
    });
    // Soglia del segnale nuovo, in giorni: `--days` la fissa a mano, altrimenti
    // segue `--months` — un progetto che cambia ogni giorno non si misura bene
    // in mesi.
    let threshold_days = days.unwrap_or(months * 30);

    let today = today();
    let mut files = Vec::new();
    walk(std::path::Path::new(&root), &mut files);
    files.sort();

    let mut suspects: Vec<Suspect> = Vec::new();
    let mut stale_only: Vec<(String, guards::stale_facts::Claim)> = Vec::new();
    // Il totale storico, invariato: stesso criterio, stessa riga finale. È il
    // contratto che legge `accettazione.py`, e questo giro non lo tocca — lo
    // arricchisce con le categorie qui sotto, senza sostituirlo.
    let mut legacy_total = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let stripped = strip_test_bodies(&text);
        legacy_total += stale_claims(&stripped, today, months).len();

        let all = all_claims(&stripped, today);
        if all.is_empty() {
            continue;
        }
        let (modified, from_git) = file_modified(path);
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        for c in all {
            match categorize(&c, modified, threshold_days) {
                Some(Category::Suspect) => {
                    let (commits, lines) =
                        if from_git { change_near(path, c.date, c.line_no) } else { (0, 0) };
                    suspects.push(Suspect { shown: shown.clone(), claim: c, commits, lines });
                }
                Some(Category::StaleOnly) => stale_only.push((shown.clone(), c)),
                None => {}
            }
        }
    }

    // Più peso a chi si è mosso di più da allora: prima le righe mosse, poi i
    // commit — un file riscritto tre volte batte un file toccato da un
    // commit di due righe, anche se quel commit è più recente.
    suspects.sort_by(|a, b| (b.lines, b.commits).cmp(&(a.lines, a.commits)));
    let total_suspects = suspects.len();
    let shown_suspects = total_suspects.min(20);
    println!("\nLe {shown_suspects} sospette con più peso, su {total_suspects} in tutto:");
    for s in suspects.iter().take(20) {
        println!(
            "\n{} — line {}, misura del {} ({} days ago)",
            s.shown, s.claim.line_no, s.claim.date.iso(), s.claim.days
        );
        println!("  peso: {} righe mosse in {} commit dopo la misura", s.lines, s.commits);
        println!("    {}", s.claim.line);
    }
    if total_suspects > 20 {
        println!("\n... altre {} sospette omesse, ordinate per peso decrescente", total_suspects - 20);
    }

    println!("\n{} solo vecchie: oltre {threshold_days} giorni, file fermo da prima", stale_only.len());
    for (shown, c) in &stale_only {
        println!("  {shown} — line {}: {} ({} days ago)", c.line_no, c.date.iso(), c.days);
        println!("    {}", c.line);
    }

    println!("\n{legacy_total} measurements older than {months} months");
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

    /// Un repository minimo con un commit a data fissa, per provare
    /// `git_last_modified` senza dipendere dall'orologio di chi esegue.
    fn commit_at(base: &std::path::Path, file_name: &str, iso_date: &str) {
        std::fs::write(base.join(file_name), "x").unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(base)
                .env("GIT_AUTHOR_NAME", "prova")
                .env("GIT_AUTHOR_EMAIL", "prova@example.com")
                .env("GIT_COMMITTER_NAME", "prova")
                .env("GIT_COMMITTER_EMAIL", "prova@example.com")
                .env("GIT_AUTHOR_DATE", format!("{iso_date}T10:00:00"))
                .env("GIT_COMMITTER_DATE", format!("{iso_date}T10:00:00"))
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["init", "-q"]);
        run(&["add", file_name]);
        run(&["commit", "-q", "-m", "prova"]);
    }

    #[test]
    fn git_last_modified_reads_the_committer_date() {
        let base = std::env::temp_dir().join(format!("stale-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        commit_at(&base, "nota.md", "2026-03-05");

        let found = git_last_modified(&base.join("nota.md"));
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(found, Date::new(2026, 3, 5));
    }

    #[test]
    fn git_last_modified_is_none_outside_a_repository() {
        let base = std::env::temp_dir().join(format!("stale-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("nota.md"), "x").unwrap();

        let found = git_last_modified(&base.join("nota.md"));
        let _ = std::fs::remove_dir_all(&base);
        assert!(found.is_none(), "trovato {found:?} fuori da un repository");
    }

    #[test]
    fn fs_modified_reads_a_date_close_to_now() {
        let base = std::env::temp_dir().join(format!("stale-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("nota.md"), "x").unwrap();

        let found = fs_modified(&base.join("nota.md"));
        let _ = std::fs::remove_dir_all(&base);
        assert!(found.is_some(), "nessuna data dal filesystem");
    }

    #[test]
    fn file_modified_prefers_git_over_the_filesystem() {
        // Il file lo scrive questo processo, ora: il filesystem direbbe oggi,
        // ma il commit porta marzo — deve vincere git.
        let base = std::env::temp_dir().join(format!("stale-pref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        commit_at(&base, "nota.md", "2026-03-05");

        let (date, from_git) = file_modified(&base.join("nota.md"));
        let _ = std::fs::remove_dir_all(&base);
        assert!(from_git, "doveva venire da git");
        assert_eq!(date, Date::new(2026, 3, 5));
    }

    /// Un commit successivo al primo, con contenuto e data a scelta — per
    /// costruire una cronologia dove alcuni commit devono contare nel peso e
    /// altri no.
    fn commit_content_at(base: &std::path::Path, file_name: &str, content: &str, iso_date: &str) {
        std::fs::write(base.join(file_name), content).unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(base)
                .env("GIT_AUTHOR_NAME", "prova")
                .env("GIT_AUTHOR_EMAIL", "prova@example.com")
                .env("GIT_COMMITTER_NAME", "prova")
                .env("GIT_COMMITTER_EMAIL", "prova@example.com")
                .env("GIT_AUTHOR_DATE", format!("{iso_date}T10:00:00"))
                .env("GIT_COMMITTER_DATE", format!("{iso_date}T10:00:00"))
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["add", file_name]);
        run(&["commit", "-q", "-m", "prova"]);
    }

    /// Nove righe, con la misura sulla quinta: base comune per i test che
    /// verificano se `change_near` segue la riga giusta, non il file intero.
    fn nine_lines(line5: &str) -> String {
        format!("l1\nl2\nl3\nl4\n{line5}\nl6\nl7\nl8\nl9\n")
    }

    fn init_repo(base: &std::path::Path) {
        let out = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(base)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn change_near_ignores_a_same_day_edit_and_counts_the_next_one() {
        let base = std::env::temp_dir().join(format!("stale-near-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        init_repo(&base);
        commit_content_at(&base, "nota.md", &nine_lines("misurato qui"), "2026-03-05");
        // Stesso giorno della misura: potrebbe essere il commit che l'ha
        // scritta, non deve contare come cambiamento successivo.
        commit_content_at(&base, "nota.md", &nine_lines("misurato qui, rivisto"), "2026-03-05");
        // Il giorno dopo, sulla stessa riga: questo conta.
        commit_content_at(&base, "nota.md", &nine_lines("smentito il giorno dopo"), "2026-03-06");

        let (commits, lines) = change_near(&base.join("nota.md"), Date::new(2026, 3, 5).unwrap(), 5);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(commits, 1, "solo il commit del 6 marzo deve contare");
        assert!(lines >= 1, "la riga 5 è cambiata: doveva vedersi almeno una riga mossa");
    }

    #[test]
    fn change_near_ignores_an_edit_far_from_the_claim_line() {
        // Stesso file di sopra, ma il commit del giorno dopo tocca la riga
        // 9, fuori dalla finestra di tre righe attorno alla misura (riga 5):
        // il file è cambiato, ma non dove viveva la misura.
        let base = std::env::temp_dir().join(format!("stale-far-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        init_repo(&base);
        commit_content_at(&base, "nota.md", &nine_lines("misurato qui"), "2026-03-05");
        let far = "l1\nl2\nl3\nl4\nmisurato qui\nl6\nl7\nl8\nriscritta lontano\n";
        commit_content_at(&base, "nota.md", far, "2026-03-06");

        let (commits, lines) = change_near(&base.join("nota.md"), Date::new(2026, 3, 5).unwrap(), 5);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!((commits, lines), (0, 0), "la riga 9 non è nella finestra della riga 5");
    }

    #[test]
    fn change_near_is_zero_without_a_git_history() {
        let base = std::env::temp_dir().join(format!("stale-notrack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("nota.md"), "x").unwrap();

        let out = change_near(&base.join("nota.md"), Date::new(2026, 1, 1).unwrap(), 1);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(out, (0, 0));
    }
}
