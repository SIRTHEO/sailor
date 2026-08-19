//! Quando si tocca una regola, verifica che i suoi riferimenti esistano ancora.
//!
//! L'originale è `skills/hooks/live-rules.py`, 480 righe. Il suo costo misurato
//! il 17/08/2026 è **62,7 ms per invocazione anche quando non ha niente da
//! dire**: gira su ogni `Write|Edit|MultiEdit` e la prima cosa che fa è uscire
//! zero se il percorso non è una regola. Quei 62 ms sono avvio dell'interprete,
//! non lavoro — ed è il muro di `PostToolUse`.
//!
//! Qui sta il giudizio puro. Le due cose che hanno bisogno del mondo — il
//! verificatore Node e il registro dei guasti — stanno nel binario, come per
//! ogni altro gancio portato.
//!
//! Il perimetro difeso è duplice, e le due metà hanno destinatari diversi:
//!
//! - `always_loaded` guarda **la regola appena scritta**: senza `paths:` entra
//!   in ogni sessione, e il costo non si vede da dentro perché il contesto non
//!   elenca ciò che ha caricato;
//! - `dead_globs` e `dead_home_paths` guardano **i riferimenti** di una regola
//!   globale, che il verificatore Node non sa misurare: `~/.claude/rules/` non
//!   appartiene a un repo, quindi i suoi `paths:` vanno provati contro i repo
//!   veri e i percorsi citati con una `stat`.

use std::path::{Path, PathBuf};

/// I marcatori con cui il verificatore Node etichetta un reperto. Sono in
/// italiano perché li scrive lui: cambiarli qui non cambia il suo output, fa
/// solo smettere di riconoscerlo.
pub const MARKERS: [&str; 4] = ["[percorso]", "[env]", "[ramo]", "[paths]"];

/// Il frontmatter YAML in testa al file, senza i delimitatori. `None` se il
/// file non ne ha uno — e non averne è già un verdetto, non un caso limite.
fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some(&rest[..end])
}

/// Le righe commentate via, come fa `re.sub(r'(?m)^[ \t]*#.*$', '', front)`.
///
/// Serve perché un `# paths: [...]` dentro una spiegazione non fa caricare
/// niente: contarlo come configurazione lascerebbe passare proprio le regole
/// che si vogliono trovare.
fn without_comments(front: &str) -> String {
    front
        .lines()
        .map(|l| {
            if l.trim_start_matches([' ', '\t']).starts_with('#') {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Perché questa regola entra in ogni sessione, o `None` se non lo fa.
///
/// Due modi di caricarsi sempre, e il secondo non si vede a occhio: nessun
/// `paths:`, oppure `paths: ["**"]`, che combacia con qualunque file e per
/// giunta rientra a ogni tocco e non solo in testa.
///
/// Le tre stringhe di ritorno finiscono dentro il messaggio che il modello
/// legge: sono contratto verso l'esterno, non testo di servizio.
pub fn always_loaded(text: &str) -> Option<&'static str> {
    let Some(front) = frontmatter(text) else {
        return Some("it has no frontmatter at all");
    };
    let front = without_comments(front);
    if !front.lines().any(|l| l.trim_start().starts_with("paths:")) {
        return Some("it has a frontmatter but no `paths:`");
    }
    if universal_paths(&front) {
        return Some("its `paths:` is `[\"**\"]`, which matches every file");
    }
    None
}

/// `paths: [ "**" ]` con qualunque spaziatura, che è il caso che non si vede a
/// occhio: sembra uno scope e invece è l'assenza di scope.
fn universal_paths(front: &str) -> bool {
    for line in front.lines() {
        let Some((_, after)) = line.split_once("paths:") else {
            continue;
        };
        let compact: String = after.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.starts_with("[\"**\"]") {
            return true;
        }
    }
    false
}

/// I glob dentro `paths:` nel frontmatter, in ordine. Vuoto se non c'è.
pub fn frontmatter_globs(text: &str) -> Vec<String> {
    let Some(front) = frontmatter(text) else {
        return Vec::new();
    };
    let front = without_comments(front);
    let Some(line) = front.lines().find(|l| l.trim_start().starts_with("paths:")) else {
        return Vec::new();
    };
    quoted(line)
}

/// Le stringhe fra virgolette doppie di una riga, nell'ordine in cui compaiono.
fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

/// La parte letterale iniziale del glob, o `""` se comincia con un jolly.
///
/// `src/**` dà `src`, `**/.claude/**` dà `""`. È quanto basta per sapere se il
/// glob può agganciare qualcosa: espanderlo davvero significherebbe attraversare
/// `node_modules` a ogni scrittura di regola.
pub fn glob_prefix(glob: &str) -> String {
    let mut parts = Vec::new();
    for part in glob.split('/') {
        if part.contains(['*', '?', '[']) {
            break;
        }
        parts.push(part);
    }
    parts.join("/")
}

/// I glob di `paths:` che non agganciano niente in **nessuno** dei repo.
///
/// Una regola globale non appartiene a un repo: i suoi `paths:` parlano dei
/// repo in cui verrà caricata. Misurarli sulla home — dove `src/` non esiste per
/// definizione — dichiarava morti tutti i glob di tutte le regole globali: 11 su
/// 16 il 14/08/2026, sempre.
///
/// Un glob che comincia con un jolly non ha prefisso da verificare e passa: il
/// falso negativo è il verso giusto in cui sbagliare, perché un gancio che grida
/// sul corretto viene spento.
pub fn dead_globs(text: &str, repos: &[PathBuf]) -> Vec<String> {
    if repos.is_empty() {
        return Vec::new();
    }
    let mut dead = Vec::new();
    for g in frontmatter_globs(text) {
        let prefix = glob_prefix(&g);
        if prefix.is_empty() {
            continue;
        }
        if !repos.iter().any(|repo| repo.join(&prefix).exists()) {
            dead.push(g);
        }
    }
    dead
}

/// I percorsi assoluti o `~/…` citati fra apici inversi e non più esistenti.
///
/// Il verificatore Node farebbe di più, ma su una regola globale non è usabile:
/// la fa cadere sulla home, e indicizzare l'intera home lo manda in timeout dopo
/// 60 secondi — con l'eccezione inghiottita e il gancio muto. Qui si guarda solo
/// ciò che una regola globale cita davvero, cioè i propri fratelli sotto
/// `~/.claude/`, e costa una `stat` a percorso.
pub fn dead_home_paths(text: &str, home: &Path) -> Vec<String> {
    let mut dead = Vec::new();
    for token in backticked(text) {
        // Un percorso citato porta spesso dietro una riga (`file.md:12`) o
        // un'ancora (`file.md#sezione`): la `stat` va fatta sul file.
        let t = token.trim();
        let t = t.split(':').next().unwrap_or("");
        let t = t.split('#').next().unwrap_or("");
        if !(t.starts_with("~/") || t.starts_with("/Users/")) {
            continue;
        }
        if !has_short_extension(t) {
            continue;
        }
        if !expand_user(t, home).exists() {
            dead.push(t.to_string());
        }
    }
    dead
}

/// L'equivalente di `re.search(r"\.[A-Za-z0-9]{1,5}$", t)`: senza un'estensione
/// corta il token è quasi sempre una cartella o una frase, e una cartella citata
/// che non esiste non è un reperto affidabile.
fn has_short_extension(t: &str) -> bool {
    let Some(dot) = t.rfind('.') else { return false };
    let ext = &t[dot + 1..];
    (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

fn expand_user(t: &str, home: &Path) -> PathBuf {
    match t.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(t),
    }
}

/// Il contenuto fra apici inversi, escluse le sequenze che scavalcano una riga.
fn backticked(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let inner = &after[..close];
        // `[^`\n]+`: niente a capo dentro, e niente coppie vuote.
        if !inner.is_empty() && !inner.contains('\n') {
            out.push(inner);
        }
        rest = &after[close + 1..];
    }
    out
}

/// I reperti su una regola globale: glob spenti e percorsi citati e spariti.
pub fn global_rule_lines(path: &Path, text: &str, repos: &[PathBuf], home: &Path) -> Vec<String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut lines: Vec<String> = dead_globs(text, repos)
        .into_iter()
        .map(|g| format!("{name}  [paths] il glob `{g}` non esiste in nessun repo in carico"))
        .collect();
    lines.extend(
        dead_home_paths(text, home)
            .into_iter()
            .map(|p| format!("{name}  [percorso] `{p}` non esiste")),
    );
    lines
}

/// `(radice, nome_repo, percorso_relativo)` per il file toccato, o `None`.
///
/// Regge le due forme in cui un repo si presenta su questo disco:
/// `~/gyver/work/suite/.claude/rules/x.md` e
/// `~/orca/workspaces/suite/mio-albero/.claude/rules/x.md`. In entrambi i casi
/// il repo è la cartella che contiene `.claude/rules/`, e la radice è il suo
/// genitore — che è esattamente ciò che il verificatore si aspetta nella carta.
pub fn repo_of(path: &str) -> Option<(PathBuf, String, String)> {
    let p = Path::new(path);
    let mut parent = p.parent();
    while let Some(dir) = parent {
        if dir.join(".claude").join("rules").is_dir() {
            let root = dir.parent()?.to_path_buf();
            let name = dir.file_name()?.to_string_lossy().into_owned();
            let relative = p.strip_prefix(&root).ok()?.to_string_lossy().into_owned();
            return Some((root, name, relative));
        }
        parent = dir.parent();
    }
    None
}

/// La regola sta in `~/.claude/rules/`, cioè non appartiene a un repo.
pub fn is_global(path: &str, home: &Path) -> bool {
    let target = home.join(".claude").join("rules");
    let Some(parent) = Path::new(path).parent() else {
        return false;
    };
    // Si scioglie **sempre**, da entrambe le parti, come fa `Path.resolve()`
    // dell'originale.
    //
    // La prima stesura provava prima un confronto letterale e cadeva sullo
    // scioglimento solo se falliva. Copriva il caso del percorso raggiunto
    // attraverso un link, e sbagliava quello opposto: se è `~/.claude/rules`
    // **stessa** a essere un collegamento verso un'altra cartella, il confronto
    // letterale riesce per primo e risponde «globale» dove l'originale, che
    // scioglie, risponde «no» — due percorsi di codice diversi e due messaggi
    // diversi per lo stesso file. Trovato da una revisione indipendente.
    match (parent.canonicalize(), target.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        // Se non si può sciogliere (cartella inesistente) resta il confronto
        // letterale: è tutto ciò che si può dire, e negare sempre farebbe
        // tacere il gancio su una regola appena creata in una cartella nuova.
        _ => parent == target,
    }
}

/// Le sole righe del verificatore che parlano del file appena scritto.
///
/// Senza questo filtro il gancio rimprovererebbe per il debito altrui — e un
/// controllo che accusa per cose che non hai toccato viene spento entro il
/// pomeriggio.
pub fn mine(output: &str, relative: &str) -> Vec<String> {
    output
        .lines()
        .filter(|r| MARKERS.iter().any(|m| r.contains(m)) && r.contains(relative))
        .map(|r| r.trim().to_string())
        .collect()
}

/// Il messaggio sul perimetro, parola per parola come lo scriveva l'originale:
/// è già citato nelle regole, e cambiarlo spezzerebbe i rimandi.
pub fn scope_message(why: &str) -> String {
    format!(
        "This rule loads into every session: {why}.\n\
         Give it a `paths:` scoped to the moment it applies, so it arrives\n\
         when it can still change a decision instead of riding along in the\n\
         preamble. If it really must always load, say so in a comment above\n\
         `paths:` — an unexplained wide scope reads as an oversight and the\n\
         next session will narrow it."
    )
}

/// Il messaggio sui riferimenti morti, anch'esso identico all'originale.
pub fn dead_refs_message(lines: &[String]) -> String {
    let body = lines
        .iter()
        .map(|r| format!("  {r}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Dead references in the rule you just wrote:\n{body}\n\n\
         Fix the reference or drop the line: a rule pointing at a file that\n\
         does not exist reads as true to whoever receives it as context."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_with_no_frontmatter_loads_always() {
        assert!(always_loaded("# a rule\n\nbody\n").is_some());
    }

    #[test]
    fn a_frontmatter_without_paths_loads_always() {
        assert!(always_loaded("---\ndescription: x\n---\n\n# a rule\n").is_some());
    }

    #[test]
    fn a_universal_paths_loads_always() {
        assert!(always_loaded("---\npaths: [\"**\"]\n---\n\n# a rule\n").is_some());
        assert!(
            always_loaded("---\npaths: [ \"**\" ]\n---\n\n# a rule\n").is_some(),
            "spacing must not hide a universal scope"
        );
    }

    #[test]
    fn mutant_a_scoped_rule_passes() {
        assert!(always_loaded("---\npaths: [\"src/**\"]\n---\n\n# a rule\n").is_none());
        assert!(
            always_loaded("---\npaths: [\"**/*.ts\", \"**/*.tsx\"]\n---\n\n# x\n").is_none(),
            "a wide but not universal scope is not the same as no scope"
        );
    }

    #[test]
    fn mutant_a_commented_out_paths_is_not_a_paths() {
        assert!(always_loaded("---\n# paths: [\"src/**\"]\ndescription: x\n---\n\n# x\n").is_some());
        assert!(
            always_loaded("---\n# why it is narrow\npaths: [\"src/**\"]\n---\n\n# x\n").is_none(),
            "a comment above a real paths must not hide it"
        );
    }

    #[test]
    fn the_literal_head_of_a_glob_is_what_can_be_checked() {
        assert_eq!(glob_prefix("src/**"), "src");
        assert_eq!(glob_prefix("packages/**/*.ts"), "packages");
        assert_eq!(glob_prefix("**/.claude/**"), "");
    }

    #[test]
    fn globs_are_read_in_order_from_the_frontmatter() {
        assert_eq!(
            frontmatter_globs("---\npaths: [\"a/**\", \"b/**\"]\n---\n\n# x\n"),
            vec!["a/**".to_string(), "b/**".to_string()]
        );
        assert!(frontmatter_globs("# no frontmatter\n").is_empty());
    }

    #[test]
    fn a_glob_alive_in_any_repo_is_not_reported() {
        let dir = tempdir("live-rules-globs");
        let repos = vec![dir.join("suite"), dir.join("whatsapp")];
        std::fs::create_dir_all(repos[0].join("src")).unwrap();
        std::fs::create_dir_all(repos[1].join("scripts")).unwrap();

        let rule = "---\npaths: [\"src/**\", \"scripts/**\"]\n---\n\n# x\n";
        assert!(dead_globs(rule, &repos).is_empty());

        let typo = "---\npaths: [\"srcs/**\"]\n---\n\n# x\n";
        assert_eq!(dead_globs(typo, &repos), vec!["srcs/**".to_string()]);

        // Il difetto vero: misurati su una radice senza repo questi sono TUTTI
        // morti, ed è per questo che il gancio taceva su 11 regole globali su 16.
        let home_like = vec![dir.join("home")];
        std::fs::create_dir_all(&home_like[0]).unwrap();
        assert_eq!(
            dead_globs(rule, &home_like),
            vec!["src/**".to_string(), "scripts/**".to_string()]
        );

        assert!(
            dead_globs("---\npaths: [\"**/.claude/**\"]\n---\n\n# x\n", &repos).is_empty(),
            "a glob starting with a wildcard has no head to check"
        );
        assert!(
            dead_globs(typo, &[]).is_empty(),
            "no carta means no verdict, never a false alarm"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_backticked_paths_with_a_short_extension_are_checked() {
        let dir = tempdir("live-rules-paths");
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/there.md"), "x").unwrap();

        let text = "vedi `~/.claude/there.md` e `~/.claude/gone.md`, \
                    la cartella `~/.claude/rules/` e la riga `~/.claude/gone.md:12`";
        let dead = dead_home_paths(text, &dir);
        assert_eq!(
            dead,
            vec![
                "~/.claude/gone.md".to_string(),
                "~/.claude/gone.md".to_string()
            ],
            "the anchor and line suffix are stripped before the stat"
        );
        assert!(
            !dead.iter().any(|d| d.ends_with('/')),
            "a directory has no short extension, so it is not a finding"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mutant_a_relative_path_is_not_a_home_path() {
        // `src/x.ts` esiste solo dentro un repo: giudicarlo qui produrrebbe un
        // reperto per ogni regola che cita un file del proprio progetto.
        assert!(dead_home_paths("vedi `src/x.ts`", Path::new("/nowhere")).is_empty());
    }

    #[test]
    fn a_backtick_run_across_lines_is_not_a_path() {
        assert!(dead_home_paths("`~/a\n/b.md`", Path::new("/nowhere")).is_empty());
    }

    #[test]
    fn it_finds_root_repo_name_and_relative_path() {
        let dir = tempdir("live-rules-repo");
        std::fs::create_dir_all(dir.join("suite/.claude/rules")).unwrap();
        let rule = dir.join("suite/.claude/rules/x.md");
        std::fs::write(&rule, "# a rule\n").unwrap();

        let (root, name, relative) = repo_of(rule.to_str().unwrap()).unwrap();
        assert_eq!(root, dir);
        assert_eq!(name, "suite");
        assert_eq!(relative, "suite/.claude/rules/x.md");

        // La forma dell'albero di lavoro Orca, che è quella che il gancio
        // sbagliava: due livelli sotto `workspaces`, non uno.
        std::fs::create_dir_all(dir.join("workspaces/suite/a-tree/.claude/rules")).unwrap();
        let wt = dir.join("workspaces/suite/a-tree/.claude/rules/y.md");
        std::fs::write(&wt, "# a rule\n").unwrap();
        let (root, name, _) = repo_of(wt.to_str().unwrap()).unwrap();
        assert_eq!(name, "a-tree");
        assert_eq!(root, dir.join("workspaces/suite"));

        // `repo_of` risponde per QUALUNQUE file dentro un repo, anche fuori da
        // `rules/`: è il chiamante a filtrare sul percorso.
        let other = dir.join("suite/other.md");
        assert_eq!(repo_of(other.to_str().unwrap()).unwrap().1, "suite");
        assert!(
            repo_of(dir.join("loose.md").to_str().unwrap()).is_none(),
            "MUTANT: a file under no repo at all resolves to nothing"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn it_reports_only_the_file_just_written() {
        let output = "suite\n  \
            suite/.claude/rules/mine.md:12  [percorso] `x.ts` does not exist\n  \
            suite/.claude/rules/theirs.md:3  [percorso] `y.ts` does not exist\n";
        assert_eq!(
            mine(output, "suite/.claude/rules/mine.md"),
            vec!["suite/.claude/rules/mine.md:12  [percorso] `x.ts` does not exist".to_string()]
        );
        assert_eq!(
            output
                .lines()
                .filter(|r| MARKERS.iter().any(|m| r.contains(m)))
                .count(),
            2,
            "MUTANT: without the filter someone else's debt would come through"
        );
        assert!(
            mine("suite\n\n1 riferimenti morti NUOVI.\n", "suite").is_empty(),
            "header lines do not pass the filter"
        );
    }

    #[test]
    fn a_rule_under_the_home_rules_dir_is_global() {
        let dir = tempdir("live-rules-global");
        std::fs::create_dir_all(dir.join(".claude/rules")).unwrap();
        assert!(is_global(
            dir.join(".claude/rules/x.md").to_str().unwrap(),
            &dir
        ));
        assert!(
            !is_global("/Users/theo/gyver/work/suite/.claude/rules/x.md", &dir),
            "MUTANT: a rule inside a repo is not global"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Una cartella temporanea senza dipendenze esterne, sotto la radice del
    /// processo: il pid la separa dalle batterie che girano in parallelo.
    fn tempdir(tag: &str) -> PathBuf {
        hook_io::testing::test_dir(tag)
    }
}
