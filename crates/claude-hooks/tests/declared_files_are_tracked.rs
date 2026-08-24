//! Ogni file che un sorgente dichiara deve esistere **per git**, non solo sul disco.
//!
//! IL GUASTO CHE QUESTA PROVA ESISTE PER PRENDERE, capitato due volte il
//! 21/08/2026 e la seconda ha tenuto rotto il ramo principale per un'ora e
//! mezza: quando cinque sessioni scrivono nello stesso deposito, `git add
//! <file>` prende anche i pezzi non finiti degli altri. Cosi' `main.rs` e'
//! arrivato su `main` dichiarando `mod marker_sweep;` mentre `marker_sweep.rs`
//! esisteva **soltanto nell'albero di lavoro**. Qui compilava; da un clone
//! pulito no.
//!
//! PERCHE' NESSUNO SE N'E' ACCORTO, ed e' il punto che decide come si scrive
//! questa prova: il file c'era. Chi guarda il disco vede tutto a posto — la
//! macchina che sbaglia e' l'unica che non puo' accorgersene. **L'unica fonte
//! che risponde davvero e' l'indice di git**, ed e' per questo che qui si chiama
//! `git ls-files` invece di `Path::exists`. Il mutante di questa prova e'
//! esattamente quello: farle guardare il disco, e vederla restare verde.
//!
//! PERCHE' UNA PROVA E NON UN GANCIO. Un gancio suo vuole una riga nel file
//! delle impostazioni, e quella riga la scrive solo Theo: un controllo che per
//! nascere dipende dalla sua mano arriva giorni dopo il guasto che deve
//! impedire. La batteria invece gira gia' prima di ogni commit, e questo
//! controllo costa due comandi.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// La radice del deposito, risalendo dal manifesto di questa cassa.
///
/// `CLAUDE_HOOKS_REPO_ROOT` la sostituisce, e serve a un caso solo: quando i
/// sorgenti vengono compilati fuori dal deposito — `cargo mutants` li copia in
/// una cartella temporanea — `git rev-parse` non ha niente da dire e questa
/// prova cade portandosi dietro l'intera base, che è la condizione per cui
/// nessun mutante viene misurato. Puntata al deposito vero, la domanda resta
/// quella di sempre: risponde l'indice di git di `~/.claude`, non il disco
/// della copia. Senza la variabile non cambia niente.
fn repo_root() -> PathBuf {
    if let Some(root) = std::env::var_os("CLAUDE_HOOKS_REPO_ROOT") {
        let root = PathBuf::from(root);
        // Una radice sbagliata farebbe tacere la prova invece di romperla: senza
        // `.git` non c'è nessun indice da interrogare, e `git ls-files` direbbe
        // «non lo conosco» per ogni file.
        assert!(
            root.join(".git").exists(),
            "CLAUDE_HOOKS_REPO_ROOT points at {}, which is not a git repository",
            root.display()
        );
        return root;
    }
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git rev-parse must run");
    assert!(out.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8_lossy(&out.stdout).trim())
}

/// L'albero dei sorgenti Rust: tutte le casse, non solo quella che ospita la
/// prova. Il guasto non ha niente di specifico a questa cassa.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.join("rust").join("crates")];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // `target` e' prodotto, non sorgente.
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// La cartella in cui un file cerca i propri sottomoduli, secondo le regole di
/// Rust: `main.rs`, `lib.rs` e `mod.rs` guardano accanto a se', ogni altro file
/// guarda in una cartella che porta il proprio nome.
fn module_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    match file.file_stem().and_then(|s| s.to_str()) {
        Some("main") | Some("lib") | Some("mod") => parent,
        Some(stem) => parent.join(stem),
        None => parent,
    }
}

/// I nomi dichiarati con `mod X;` — solo la forma col punto e virgola, che e'
/// quella che promette un file. `mod tests { … }` ha il corpo dentro e non
/// promette niente.
fn declared_mods(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        let rest = line
            .strip_prefix("pub mod ")
            .or_else(|| line.strip_prefix("mod "))
            .or_else(|| line.strip_prefix("pub(crate) mod "));
        if let Some(rest) = rest {
            if let Some(name) = rest.strip_suffix(';') {
                let name = name.trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

/// I percorsi inclusi alla lettera, relativi al file che li cita.
///
/// LE RIGHE DI COMMENTO SI SALTANO, e non e' una raffinatezza: questo stesso
/// file nomina quella macro nel proprio commento d'intestazione per spiegare a
/// cosa serve, e senza questa riga la prova accusava se stessa di includere un
/// file inesistente. L'ha trovata il mutante, non io.
fn included_paths(source: &str) -> BTreeSet<String> {
    const OPEN: &str = "include_str!(\"";
    let mut paths = BTreeSet::new();
    for line in source.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for (idx, _) in line.match_indices(OPEN) {
            let after = &line[idx + OPEN.len()..];
            if let Some(end) = after.find('"') {
                paths.insert(after[..end].to_string());
            }
        }
    }
    paths
}

/// LA DOMANDA E' «GIT LO CONOSCE?», non «esiste?». Vedi il commento in testa:
/// invertire queste due rende la prova cieca esattamente al caso che la motiva.
fn git_knows(root: &Path, path: &Path) -> bool {
    Command::new("git")
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg(path)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Il contenuto del file come sta **in indice**: quello che il prossimo commit
/// porterebbe. `None` quando l'indice non ha quel percorso, e chi chiama lo
/// tratta come «non c'è niente di promesso qui».
fn staged_text(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let out = Command::new("git")
        .arg("show")
        .arg(format!(":{}", relative.display()))
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Le dichiarazioni che il prossimo commit porterebbe senza il file dietro.
///
/// Sta fuori dal caso di prova perché la scena del guasto si possa costruire in
/// un deposito usa-e-getta: provare questo controllo sull'albero vero
/// vorrebbe dire sporcare l'indice che tutte le sessioni condividono.
fn broken_declarations(root: &Path, sources: &[PathBuf]) -> Vec<String> {
    let mut untracked: Vec<String> = Vec::new();

    for file in sources {
        // Un sorgente che git non conosce non puo' pretendere niente dagli
        // altri: e' lavoro in corso di qualcuno, e le sue dichiarazioni non
        // sono ancora una promessa fatta al ramo.
        if !git_knows(root, file) {
            continue;
        }
        // IL TESTO SI LEGGE DALL'INDICE, NON DAL DISCO — dal 24/08/2026. Una
        // dichiarazione che vive solo nell'albero di lavoro non è una promessa
        // fatta a nessuno: è lavoro in corso, e finché non entra in un commit
        // il ramo pubblicato non ne sa niente. Leggendo il disco, questa prova
        // diventava rossa ogni volta che un'altra sessione stava scrivendo un
        // modulo nuovo — due volte su tre corse il 24/08 — e un rosso che non
        // è tuo è il modo più rapido per far spegnere una prova. `git show :`
        // dà la versione **in indice**, cioè quello che il prossimo commit
        // porterà: così il controllo morde ancora prima del commit, che è il
        // momento in cui serve, senza contare ciò che nessuno ha promesso.
        let text = match staged_text(root, file) {
            Some(t) => t,
            None => continue,
        };
        let dir = module_dir(file);

        for name in declared_mods(&text) {
            let flat = dir.join(format!("{name}.rs"));
            let nested = dir.join(&name).join("mod.rs");
            if !git_knows(root, &flat) && !git_knows(root, &nested) {
                untracked.push(format!(
                    "{} declares `mod {};` but git knows neither {} nor {}",
                    file.strip_prefix(root).unwrap_or(file).display(),
                    name,
                    flat.strip_prefix(root).unwrap_or(&flat).display(),
                    nested.strip_prefix(root).unwrap_or(&nested).display()
                ));
            }
        }

        for rel in included_paths(&text) {
            let target = file.parent().unwrap_or(Path::new(".")).join(&rel);
            if !git_knows(root, &target) {
                untracked.push(format!(
                    "{} includes \"{}\" but git does not know {}",
                    file.strip_prefix(root).unwrap_or(file).display(),
                    rel,
                    target.strip_prefix(root).unwrap_or(&target).display()
                ));
            }
        }
    }

    untracked
}

#[test]
fn every_declared_module_and_included_file_is_tracked_by_git() {
    let root = repo_root();
    let sources = rust_sources(&root);
    assert!(
        sources.len() > 10,
        "found only {} rust sources under {}: the walk is broken, not the tree",
        sources.len(),
        root.display()
    );

    let untracked = broken_declarations(&root, &sources);
    assert!(
        untracked.is_empty(),
        "the branch does not build from a clean clone:\n  {}",
        untracked.join("\n  ")
    );
}

/// Il deposito usa-e-getta con un `lib.rs` che dichiara `mod <name>;`.
/// `staged` dice se la dichiarazione entra in indice o resta solo sul disco.
///
/// La scena sta dove il sistema dice, non in `/tmp` scritto per esteso: dentro
/// il perimetro delle sessioni quel percorso non è scrivibile e i due casi che
/// usano questa funzione morivano di `PermissionDenied`, cioè con un rosso che
/// non distingue un ramo rotto da una scrittura negata.
fn scene(name: &str, declares: &str, staged: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "claude-hooks-prove-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("rust").join("crates").join("uno").join("src");
    std::fs::create_dir_all(&src).expect("the scene directory");
    let run = |args: &[&str]| {
        Command::new("git").args(args).current_dir(&dir).output().expect("git must run");
    };
    run(&["init", "--quiet"]);
    run(&["config", "user.email", "prova@example.com"]);
    run(&["config", "user.name", "prova"]);
    let lib = src.join("lib.rs");
    std::fs::write(&lib, "// niente\n").expect("the first lib.rs");
    run(&["add", "."]);
    run(&["commit", "--quiet", "-m", "base"]);
    std::fs::write(&lib, declares).expect("the declaring lib.rs");
    if staged {
        run(&["add", "rust/crates/uno/src/lib.rs"]);
    }
    dir
}

/// Il guasto vero: la dichiarazione è in indice, il file che promette no.
/// MUTANTE (rotto così → rosso): far leggere di nuovo il disco a `staged_text`
/// — questo caso resterebbe verde, perché su disco e in indice il testo è lo
/// stesso, ma la prova non distinguerebbe più i due mondi.
#[test]
fn a_declaration_staged_without_its_file_is_caught() {
    let root = scene("promessa-in-indice", "pub mod mancante;\n", true);
    let sources = rust_sources(&root);
    let broken = broken_declarations(&root, &sources);
    assert_eq!(broken.len(), 1, "{broken:?}");
    assert!(broken[0].contains("mod mancante"), "{broken:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// E il lavoro in corso di un altro non è un guasto: la stessa identica
/// dichiarazione, lasciata solo nell'albero di lavoro, non promette niente a
/// nessuno. È il falso rosso che il 24/08/2026 ha colpito due corse su tre.
/// MUTANTE (rotto così → rosso): rimettere `read_to_string` al posto di
/// `staged_text` — questo caso torna rosso, e con lui ogni sessione che sta
/// scrivendo un modulo nuovo accanto alla tua.
#[test]
fn a_declaration_left_in_the_working_tree_is_nobodys_promise() {
    let root = scene("promessa-solo-sul-disco", "pub mod mancante;\n", false);
    let sources = rust_sources(&root);
    let broken = broken_declarations(&root, &sources);
    assert!(broken.is_empty(), "{broken:?}");
    let _ = std::fs::remove_dir_all(&root);
}
