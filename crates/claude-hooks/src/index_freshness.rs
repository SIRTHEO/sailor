//! `SessionStart`: legge i repo che SocratiCode ha indicizzato e git vero, e
//! passa i numeri al giudice puro di `guards::index_freshness`.
//!
//! DA DOVE VIENE L'ELENCO DEI REPO INDICIZZATI. `codebase_list_projects` è
//! uno strumento MCP: solo una sessione con quello strumento caricato può
//! interrogarlo, non un binario lanciato da un gancio. La sua risposta però
//! arriva già su disco — `~/.claude/state/socraticode-progetti.txt`, la
//! stessa lista che `guards::socraticode_gate::declared_projects_list` legge
//! per decidere se una cartella è indicizzata — quindi qui si legge quella,
//! invece di duplicare la domanda.
//!
//! QUALE RAMO RIFLETTE L'INDICE SI DEDUCE, NON SI LEGGE. Nessun campo dice
//! «questo indice è stato scritto sul ramo X»: SocratiCode indicizza i file
//! sul disco del checkout canonico, punto. L'unica verità disponibile è il
//! ramo che quel checkout ha in HEAD *adesso* — vero finché nessuno cambia
//! ramo lì in mezzo, ed è esattamente il buco che questo strumento misura.

use guards::index_freshness::{format_line, judge, Observation};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Il ramo su cui questi repo integrano il lavoro. **Non** dedotto da
/// `origin/HEAD`: quel simbolico punta altrove a seconda del repo — misurato
/// il 20/08/2026, `origin/HEAD` risolve a `staging` per whatsapp e
/// matching-engine, a `main` per packages, e solo per suite a `develop`.
/// Leggerlo da lì capovolgerebbe il verdetto in tre casi su quattro.
const INTEGRATION_BRANCH: &str = "develop";

/// I ripieghi, provati in quest'ordine quando il preferito non esiste.
///
/// MISURATO ALLA PRIMA ESECUZIONE VERA, il 20/08/2026: con `develop` scritto
/// fisso, tre repo su quattro campionati rispondevano «non verificabile» — fra
/// questi `~/.claude`, cioè il repo dove questo controllo vive. Un controllo
/// che non sa giudicare la casa propria non è prudente, è cieco.
const INTEGRATION_FALLBACKS: &[&str] = &["main", "master"];

/// Il primo ramo d'integrazione che esiste davvero su `origin`, fra il
/// preferito e i ripieghi. Nessuno esiste → si torna al preferito, così il
/// verdetto resta «non verificabile» e dice quale ramo ha cercato.
fn integration_for(toplevel: &Path, preferred: &str) -> String {
    for branch in std::iter::once(preferred).chain(INTEGRATION_FALLBACKS.iter().copied()) {
        if has_remote_branch(toplevel, branch) {
            return branch.to_string();
        }
    }
    preferred.to_string()
}

fn has_remote_branch(toplevel: &Path, branch: &str) -> bool {
    git_output(
        toplevel,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
    .is_some()
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Il checkout canonico di cui `dir` fa parte — risolve un percorso dichiarato
/// che è una sottocartella di un repo (`gyver/work/.claude` è dentro il repo
/// `gyver/work`) alla stessa radice, cosicché due radici dichiarate per lo
/// stesso repo producano un solo verdetto invece di due identici.
fn toplevel_of(dir: &Path) -> Option<PathBuf> {
    git_output(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

fn branch_of(dir: &Path) -> String {
    git_output(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "?".to_string())
}

fn behind_ahead(dir: &Path, integration: &str) -> Option<(u32, u32)> {
    let spec = format!("origin/{integration}...HEAD");
    let raw = git_output(dir, &["rev-list", "--left-right", "--count", &spec])?;
    let mut parts = raw.split_whitespace();
    let behind: u32 = parts.next()?.parse().ok()?;
    let ahead: u32 = parts.next()?.parse().ok()?;
    Some((behind, ahead))
}

/// L'età del fetch più recente, dalla data di modifica di `FETCH_HEAD` — lo
/// stesso file che git stesso aggiorna a ogni `fetch`/`pull`. Non prova a
/// seguire un `.git` che è un file di worktree: i repo dichiarati sono
/// canonici per costruzione (`declared_projects_list` li elenca così), e un
/// checkout che non lo fosse riporterebbe semplicemente età sconosciuta.
fn fetch_age_secs(dir: &Path) -> Option<u64> {
    let marker = dir.join(".git").join("FETCH_HEAD");
    let modified = std::fs::metadata(&marker).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok().map(|d| d.as_secs())
}

fn observe(toplevel: &Path, integration: &str) -> Observation {
    let name = toplevel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| toplevel.to_string_lossy().into_owned());
    let branch = branch_of(toplevel);
    let has_integration_ref = git_ok(
        toplevel,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{integration}"),
        ],
    );
    let (behind, ahead) = if has_integration_ref {
        match behind_ahead(toplevel, integration) {
            Some((b, a)) => (Some(b), Some(a)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    Observation {
        name,
        branch,
        integration: integration.to_string(),
        has_integration_ref,
        behind,
        ahead,
        fetch_age_secs: fetch_age_secs(toplevel),
    }
}

/// Una riga per repo, deduplicata per checkout canonico. Presa fuori da
/// `run()` perché è la parte che vale la pena provare: dato un elenco di
/// radici (vere o di prova), quali righe escono.
///
/// `integration` è il ramo **preferito**, non un obbligo: ogni repo prende il
/// primo che esiste fra quello e i ripieghi, perché la suite integra su
/// `develop` mentre `~/.claude` e `sailor` stanno su `main`.
pub fn report(roots: &[PathBuf], integration: &str) -> Vec<String> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut lines = Vec::new();
    for root in roots {
        let Some(top) = toplevel_of(root) else {
            continue; // non è dentro un repo git: niente da dire
        };
        if seen.contains(&top) {
            continue; // stesso repo dichiarato da più radici
        }
        seen.push(top.clone());
        let obs = observe(&top, &integration_for(&top, integration));
        lines.push(format_line(&obs, &judge(&obs)));
    }
    lines
}

pub fn run() -> i32 {
    let roots = guards::socraticode_gate::declared_projects_list(&home());
    let lines = report(&roots, INTEGRATION_BRANCH);
    if lines.is_empty() {
        return 0; // nessun repo indicizzato: niente da dire
    }
    // Il ramo lo nomina ogni riga, perché non è più uno solo per tutti.
    println!("SocratiCode: affidabilità degli indici");
    for line in &lines {
        println!("  {line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::test_root;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git deve essere sul PATH per questa prova");
        assert!(status.success(), "git {args:?} è fallito in {}", dir.display());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = test_root().join("index_freshness").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Un repo vero, isolato in una cartella di prova: un bare remoto e un
    /// checkout locale già configurato e già sul ramo `develop`. Le prove
    /// costruiscono da qui gli scarti che vogliono osservare, invece di
    /// inventare i numeri: è l'unico modo per provare la colla che parla con
    /// git davvero, non solo il giudizio puro (già coperto in `guards`).
    fn repo_on_develop(base: &Path) -> PathBuf {
        let bare = base.join("origin.git");
        let work = base.join("work");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "-q"]);
        // Il ramo di default del remoto è `develop`, come sui repo veri. Senza
        // questo il bare resta con HEAD su un ramo mai nato e un clone non
        // trova niente da estrarre.
        git(&bare, &["symbolic-ref", "HEAD", "refs/heads/develop"]);
        fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "t"]);
        fs::write(work.join("a.txt"), "1").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "c1"]);
        git(&work, &["branch", "-M", "develop"]);
        git(&work, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&work, &["push", "-q", "-u", "origin", "develop"]);
        work
    }

    /// Un repo che integra su `main`, come `~/.claude` e `sailor`: nessun
    /// `develop` da nessuna parte.
    fn repo_on_main(base: &Path) -> PathBuf {
        let bare = base.join("origin.git");
        let work = base.join("work");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "-q"]);
        git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "t"]);
        fs::write(work.join("a.txt"), "1").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "c1"]);
        git(&work, &["branch", "-M", "main"]);
        git(&work, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&work, &["push", "-q", "-u", "origin", "main"]);
        work
    }

    #[test]
    fn a_repo_that_integrates_on_main_is_judged_on_main() {
        // MISURATO IL 20/08/2026: col ramo scritto fisso, `~/.claude` — il repo
        // dove questo controllo vive — rispondeva «non verificabile».
        // MUTANTE: rimettere `observe(&top, integration)` senza `integration_for`.
        let base = scratch("su_main");
        let work = repo_on_main(&base);
        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("AFFIDABILE"), "line={}", lines[0]);
        assert!(
            !lines[0].contains("NON VERIFICABILE"),
            "ha cercato develop su un repo che sta su main: {}",
            lines[0]
        );
    }

    #[test]
    fn develop_wins_over_main_when_both_exist() {
        // La suite ha entrambi: il preferito deve battere il ripiego, altrimenti
        // il verdetto si sposta sul ramo sbagliato proprio dove conta.
        let base = scratch("develop_e_main");
        let work = repo_on_develop(&base);
        git(&work, &["push", "-q", "origin", "develop:main"]);
        git(&work, &["fetch", "-q"]);
        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("develop"), "line={}", lines[0]);
    }

    #[test]
    fn a_checkout_on_develop_and_caught_up_is_reliable() {
        let base = scratch("caught_up");
        let work = repo_on_develop(&base);
        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("AFFIDABILE"), "line={}", lines[0]);
        assert!(lines[0].contains("0 indietro"), "line={}", lines[0]);
    }

    #[test]
    fn a_checkout_behind_origin_develop_is_flagged() {
        let base = scratch("behind");
        let work = repo_on_develop(&base);
        let bare = base.join("origin.git");
        // Un secondo checkout spinge un commit che `work` non ha ancora
        // preso: da qui in poi `work` è indietro rispetto a `origin/develop`.
        let other = base.join("other");
        git(&base, &["clone", "-q", bare.to_string_lossy().as_ref(), "other"]);
        git(&other, &["config", "user.email", "t2@example.com"]);
        git(&other, &["config", "user.name", "t2"]);
        fs::write(other.join("b.txt"), "2").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "c2"]);
        git(&other, &["push", "-q", "origin", "develop"]);
        git(&work, &["fetch", "-q"]);

        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("1 indietro"), "line={}", lines[0]);
    }

    #[test]
    fn a_checkout_on_a_feature_branch_is_wrong() {
        let base = scratch("wrong_branch");
        let work = repo_on_develop(&base);
        git(&work, &["checkout", "-q", "-b", "feat/altro"]);

        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("SBAGLIATO"), "line={}", lines[0]);
        assert!(lines[0].contains("feat/altro"), "line={}", lines[0]);
    }

    #[test]
    fn a_repo_without_an_integration_branch_on_the_remote_is_unknown() {
        let base = scratch("no_ref");
        let bare = base.join("origin.git");
        let work = base.join("work");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "-q"]);
        fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "t"]);
        fs::write(work.join("a.txt"), "1").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "c1"]);
        // Il ramo si chiama "develop" ma non è mai stato spinto: sul
        // telecomando non esiste `origin/develop`.
        git(&work, &["branch", "-M", "develop"]);
        git(&work, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);

        let lines = report(&[work], "develop");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("NON VERIFICABILE"), "line={}", lines[0]);
    }

    #[test]
    fn two_declared_roots_in_the_same_repo_produce_one_line() {
        let base = scratch("dedup");
        let work = repo_on_develop(&base);
        let sub = work.join("src");
        fs::create_dir_all(&sub).unwrap();

        let lines = report(&[work.clone(), sub], "develop");
        assert_eq!(lines.len(), 1, "due radici, un solo checkout canonico");
    }

    #[test]
    fn a_declared_root_that_is_not_a_git_repo_is_silently_skipped() {
        let base = scratch("not_a_repo");
        fs::create_dir_all(&base).unwrap();
        let lines = report(&[base], "develop");
        assert!(lines.is_empty());
    }

    #[test]
    fn fetch_head_age_is_read_from_its_mtime() {
        let base = scratch("fetch_age");
        let work = repo_on_develop(&base);
        // `push` non tocca FETCH_HEAD: solo un `fetch` lo fa, come in
        // produzione.
        git(&work, &["fetch", "-q"]);
        let age = fetch_age_secs(&work);
        assert!(age.is_some(), "un fetch appena fatto deve avere un'età");
        assert!(age.unwrap() < 60, "age={:?}", age);
    }
}
