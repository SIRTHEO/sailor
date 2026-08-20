//! `PostToolUse`: chi ha appena fuso con schiacciamento vede cosa è rimasto fuori.
//!
//! IL PROBLEMA (mandato di Theo, 20/08/2026): dopo `gh pr merge --squash` il
//! ramo di partenza resta vivo e divergente, e ogni commit scritto **dopo**
//! l'istante della fusione non è più antenato di nessun tronco — lo
//! schiacciamento lo ha reso orfano — senza che nessun conflitto lo segnali.
//! Il confine è il TEMPO, non la parentela: prima della fusione i commit sono
//! dentro lo schiacciamento e non vanno mai segnalati, o ogni fusione altrui
//! diventerebbe rumore (il guasto che questo gancio deve evitare più di ogni
//! altro).
//!
//! Il riconoscimento del gesto e i due giudizi di contenuto sono presi da
//! `work_status.rs` (`merges_request`, `resolve_repo_slug`, `pull_by_number`,
//! `content_landed`): qui c'è solo il confine temporale e il messaggio.

use crate::work_status::{self, Py};
use serde_json::Value;
use std::io::Read;

/// Un commit rimasto fuori dal ramo appena fuso.
struct Orphan {
    sha: String,
    subject: String,
}

/// Il ref da guardare: il ramo remoto se esiste (è la verità condivisa),
/// altrimenti quello locale. `None` = questa copia non lo conosce affatto.
fn branch_ref(repo_path: &str, branch: &str) -> Option<String> {
    let remote = format!("origin/{branch}");
    if work_status::read_git(repo_path, &["rev-parse", "--verify", &remote]).is_some() {
        return Some(remote);
    }
    if work_status::read_git(repo_path, &["rev-parse", "--verify", branch]).is_some() {
        return Some(branch.to_string());
    }
    None
}

/// I commit del ramo fuso scritti dopo `merged_at` (ISO 8601, come lo scrive
/// GitHub) e il cui contenuto non è ancora arrivato su un tronco.
///
/// `--since` lascia a git il confronto delle date: sa già leggere
/// `2026-08-10T15:12:48Z` insieme al fuso orario di ogni commit, e riscriverlo
/// a mano vorrebbe dire portarsi dietro l'aritmetica del calendario per un
/// guadagno che git dà già gratis.
fn orphans_after(repo_path: &str, branch_ref: &str, merged_at: &str) -> Vec<Orphan> {
    // `%x01` come separatore: un oggetto di commit può contenere quasi
    // qualunque byte nell'oggetto messaggio tranne questo, e uno spazio o un
    // due-punti nel soggetto romperebbe uno split più ingenuo.
    // `--reverse`: dal più vecchio al più recente, l'ordine in cui i commit
    // sono stati davvero scritti — è quello che il messaggio deve raccontare.
    let log = work_status::read_git(
        repo_path,
        &[
            "log",
            branch_ref,
            &format!("--since={merged_at}"),
            "--reverse",
            "--format=%H%x01%s",
        ],
    )
    .unwrap_or_default();
    let mut found = Vec::new();
    for line in log.lines() {
        let Some((sha, subject)) = line.split_once('\u{1}') else {
            continue;
        };
        // IL VERDETTO È SUL CONTENUTO: un commit già arrivato altrove — per
        // esempio con un cherry-pick indipendente — non è un orfano, anche se
        // è cronologicamente dopo la fusione.
        if work_status::content_landed(repo_path, sha) == Some(true) {
            continue;
        }
        found.push(Orphan {
            sha: sha.to_string(),
            subject: subject.to_string(),
        });
    }
    found
}

/// Il messaggio per l'agente: cosa è rimasto fuori, e perché nessuno se ne
/// sarebbe accorto da solo.
fn report(branch: &str, orphans: &[Orphan]) -> String {
    let list = orphans
        .iter()
        .map(|o| format!("{} {}", &o.sha[..o.sha.len().min(8)], o.subject))
        .collect::<Vec<_>>()
        .join("; ");
    let quanti = orphans.len();
    let plural = if quanti == 1 { "commit" } else { "commits" };
    format!(
        "Orca: the squash merge landed '{branch}', but {quanti} {plural} written after the \
         merge never made it to trunk (checked by content, not ancestry — no conflict warns \
         about this on its own): {list}."
    )
}

/// `PostToolUse` su `Bash`: riconosciuto un gesto di fusione, cosa è rimasto
/// fuori dal ramo appena fuso.
///
/// RICHIEDE UN NUMERO ESPLICITO nel comando (`gh pr merge <n> --squash`, con o
/// senza `-R`/`--repo` in mezzo). `gh pr merge` senza numero fonde la
/// richiesta del ramo corrente, e senza un numero non c'è modo di chiedere a
/// GitHub l'istante preciso della fusione: senza quell'istante il confine
/// temporale non esiste, e segnalare a tempo indovinato sarebbe il rumore che
/// questo gancio deve evitare.
///
/// **È un buco noto che non sanguina**, e il numero serve a non farlo chiudere
/// per scrupolo da chi lo rileggerà. Misurato il 20/08/2026 su 3.363
/// trascrizioni, contando i soli comandi davvero eseguiti — quelli dentro un
/// `"command"` di uno strumento Bash, non le citazioni nelle regole e nei
/// prompt, che erano oltre duemila e davano la risposta opposta:
///
///     gh pr merge <numero>     402
///     gh pr merge --<opzione>    3   (due `--help`, uno con `--repo` e il numero dopo)
///
/// Zero fusioni vere nella forma cieca. Se un giorno comparisse, la misura si
/// rifà con lo stesso criterio: cercare la forma dentro i comandi eseguiti,
/// mai nel testo dei prompt.
fn after_merge(payload: &Value) -> Py<Option<String>> {
    if !payload.is_object() {
        return Err("il payload non è un dizionario".into());
    }
    if payload.get("tool_name") != Some(&Value::String("Bash".into())) {
        return Ok(None);
    }
    let command = work_status::command_of(payload)?;
    if !work_status::merges_request(&command) {
        return Ok(None);
    }
    if let Some(Value::Object(o)) = payload.get("tool_response") {
        let rotto = o.get("is_error").map(work_status::truthy).unwrap_or(false)
            || o.get("interrupted").map(work_status::truthy).unwrap_or(false);
        if rotto {
            return Ok(None);
        }
    }

    let number = work_status::pull_number(&command);
    if number.is_empty() {
        return Ok(None);
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let listing = work_status::orca_json(&["worktree", "list"]).unwrap_or(serde_json::json!({}));
    let worktrees = work_status::worktrees_of(&listing)?;
    let slug = work_status::resolve_repo_slug(&command, &cwd, &worktrees)?;
    if slug.is_empty() {
        return Ok(None);
    }

    let (branch, state, merged_at) = work_status::pull_by_number(&slug, &number);
    if state != "MERGED" || branch.is_empty() || merged_at.is_empty() {
        return Ok(None);
    }

    // La copia da cui leggere il ramo: quella da cui il comando è partito, se
    // riconosciuta; il `cwd` del gancio altrimenti — stessa scelta di
    // `dopo_fusione` in `work_status.rs`.
    let path_ = match work_status::worktree_of_command(&command, &cwd, &worktrees)? {
        Some(w) => w
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(&cwd)
            .to_string(),
        None => cwd.clone(),
    };
    let Some(reference) = branch_ref(&path_, &branch) else {
        return Ok(None);
    };

    let orphans = orphans_after(&path_, &reference, &merged_at);
    if orphans.is_empty() {
        return Ok(None);
    }
    Ok(Some(report(&branch, &orphans)))
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    match after_merge(&payload) {
        Err(e) => {
            eprintln!("squash-orphans non ha potuto decidere: {e}");
            0
        }
        Ok(None) => 0,
        Ok(Some(said)) => {
            println!(
                "{}",
                hook_io::python_json::dumps(&serde_json::json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PostToolUse",
                        "additionalContext": said,
                    }
                }))
            );
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Un repo git vero e isolato: solo così il confronto di contenuto
    /// (`merge-tree`) e il filtro temporale (`--since`) si provano su dati
    /// reali invece che su una tabella di stringhe.
    ///
    /// Sotto `hook_io::testing::test_root()`, la stessa radice usa-e-getta di
    /// `test_home.rs`: un nome per caso di prova la tiene distinta dalle
    /// altre, e la raccolta a inizio processo la libera senza che nessun test
    /// debba occuparsene.
    struct TestRepo {
        dir: std::path::PathBuf,
    }

    impl TestRepo {
        fn new(name: &str) -> Self {
            let dir = hook_io::testing::test_root().join(name);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let run = |args: &[&str]| {
                let out = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(args)
                    .output()
                    .unwrap();
                assert!(out.status.success(), "git {args:?}: {out:?}");
            };
            run(&["init", "-q"]);
            run(&["config", "user.email", "prova@example.com"]);
            run(&["config", "user.name", "Prova"]);
            // Un "remoto" finto: una cartella nuda da cui questo repo fa
            // `fetch`, così `origin/<ramo>` esiste davvero.
            let remote = dir.with_file_name(format!("{name}-remote.git"));
            let _ = std::fs::remove_dir_all(&remote);
            Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&remote)
                .output()
                .unwrap();
            run(&["remote", "add", "origin", remote.to_str().unwrap()]);
            Self { dir }
        }

        fn path(&self) -> &str {
            self.dir.to_str().unwrap()
        }

        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.dir)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        /// Un commit con una data precisa, per controllare il confine
        /// temporale senza dipendere dall'orologio della macchina che prova.
        fn commit(&self, file: &str, contenuto: &str, messaggio: &str, quando: &str) -> String {
            std::fs::write(self.dir.join(file), contenuto).unwrap();
            self.git(&["add", file]);
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.dir)
                .env("GIT_AUTHOR_DATE", quando)
                .env("GIT_COMMITTER_DATE", quando)
                .args(["commit", "-q", "-m", messaggio])
                .output()
                .unwrap();
            assert!(out.status.success());
            self.git(&["rev-parse", "HEAD"])
        }

        fn push(&self, refspec: &str) {
            self.git(&["push", "-q", "origin", refspec]);
        }
    }

    #[test]
    fn commits_before_the_merge_are_never_orphans() {
        // DIFFERENZIALE A VARIABILE UNICA: due commit sullo stesso ramo, uno
        // prima e uno dopo l'istante della fusione. Solo il secondo è un
        // orfano — la parentela non li distingue affatto (nessuno dei due è
        // antenato di develop dopo uno schiacciamento), il tempo sì.
        let repo = TestRepo::new("commits-before-merge-are-not-orphans");
        repo.commit("base.txt", "base", "base", "2026-08-01T09:00:00+00:00");
        repo.push("HEAD:develop");

        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.commit("before.txt", "inside-the-squash", "before", "2026-08-10T10:00:00+00:00");
        let after_commit = repo.commit("after.txt", "orphan", "after", "2026-08-10T12:00:00+00:00");
        repo.push("feature:feature");

        // Lo schiacciamento: develop riceve un commit nuovo con lo stesso
        // contenuto del "before", ma non è antenato di "feature" — è proprio
        // così che si comporta `gh pr merge --squash`.
        repo.git(&["checkout", "-q", "develop"]);
        std::fs::write(repo.dir.join("before.txt"), "inside-the-squash").unwrap();
        repo.git(&["add", "before.txt"]);
        let squash = Command::new("git")
            .arg("-C")
            .arg(&repo.dir)
            .env("GIT_AUTHOR_DATE", "2026-08-10T11:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2026-08-10T11:00:00+00:00")
            .args(["commit", "-q", "-m", "squash (#1)"])
            .output()
            .unwrap();
        assert!(squash.status.success());
        repo.push("develop:develop");
        repo.git(&["fetch", "-q", "origin"]);

        let found = orphans_after(repo.path(), "feature", "2026-08-10T11:00:00+00:00");
        assert_eq!(
            found.len(),
            1,
            "atteso un solo orfano: {:?}",
            found.iter().map(|o| &o.subject).collect::<Vec<_>>()
        );
        assert_eq!(found[0].sha, after_commit);
        assert_eq!(found[0].subject, "after");
    }

    #[test]
    fn content_already_on_trunk_is_not_an_orphan() {
        // MUTANTE: senza il controllo di contenuto, un commit scritto dopo la
        // fusione ma il cui contenuto è già su trunk (per esempio arrivato
        // da un altro ramo, un cherry-pick indipendente) verrebbe segnalato
        // lo stesso solo perché è cronologicamente dopo.
        let repo = TestRepo::new("content-already-on-trunk-is-not-orphan");
        repo.commit("base.txt", "base", "base", "2026-08-01T09:00:00+00:00");
        repo.push("HEAD:develop");

        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.commit("also.txt", "landed-elsewhere-too", "also elsewhere", "2026-08-10T12:00:00+00:00");
        repo.push("feature:feature");

        // Lo stesso contenuto arriva su develop per un'altra strada.
        repo.git(&["checkout", "-q", "develop"]);
        repo.commit(
            "also.txt",
            "landed-elsewhere-too",
            "same content, another commit",
            "2026-08-10T13:00:00+00:00",
        );
        repo.push("develop:develop");
        repo.git(&["fetch", "-q", "origin"]);

        let found = orphans_after(repo.path(), "feature", "2026-08-01T09:00:01+00:00");
        assert!(
            found.is_empty(),
            "contenuto già su trunk segnalato lo stesso: {:?}",
            found.iter().map(|o| &o.subject).collect::<Vec<_>>()
        );
    }

    #[test]
    fn several_orphans_come_back_in_full() {
        // Il caso `theo/auto/chore/check-shared-packages-offline`: più di un
        // commit rimasto fuori dallo stesso squash, e devono uscire tutti.
        let repo = TestRepo::new("several-orphans-come-back-in-full");
        repo.commit("base.txt", "base", "base", "2026-08-01T09:00:00+00:00");
        repo.push("HEAD:develop");

        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.commit("a.txt", "a", "squashed-a", "2026-08-14T10:00:00+00:00");
        let c1 = repo.commit("b.txt", "b", "orphan-1", "2026-08-14T16:00:00+00:00");
        let c2 = repo.commit("c.txt", "c", "orphan-2", "2026-08-14T17:00:00+00:00");
        repo.push("feature:feature");

        repo.git(&["checkout", "-q", "develop"]);
        repo.commit("a.txt", "a", "squash (#2)", "2026-08-14T15:00:00+00:00");
        repo.push("develop:develop");
        repo.git(&["fetch", "-q", "origin"]);

        let found = orphans_after(repo.path(), "feature", "2026-08-14T15:00:00+00:00");
        let shas: Vec<&str> = found.iter().map(|o| o.sha.as_str()).collect();
        assert_eq!(shas, vec![c1.as_str(), c2.as_str()]);
    }

    #[test]
    fn the_local_branch_is_the_fallback_when_origin_does_not_have_it() {
        // Il caso `fix/owner-journey-bugs`: la copia che prova può avere solo
        // il ramo locale, senza un `origin/<ramo>` corrispondente.
        let repo = TestRepo::new("local-branch-is-the-fallback");
        repo.commit("base.txt", "base", "base", "2026-08-01T09:00:00+00:00");
        repo.push("HEAD:develop");
        repo.git(&["fetch", "-q", "origin"]);

        repo.git(&["checkout", "-q", "-b", "local-only"]);
        repo.commit("x.txt", "x", "local only", "2026-08-05T09:00:00+00:00");

        let reference = branch_ref(repo.path(), "local-only");
        assert_eq!(reference, Some("local-only".to_string()));
    }

    #[test]
    fn the_report_names_every_orphan() {
        let orphans = vec![
            Orphan {
                sha: "1234567890".to_string(),
                subject: "first".to_string(),
            },
            Orphan {
                sha: "abcdef0123".to_string(),
                subject: "second".to_string(),
            },
        ];
        let said = report("my-branch", &orphans);
        assert!(said.contains("my-branch"), "{said}");
        assert!(said.contains("12345678"), "{said}");
        assert!(said.contains("first"), "{said}");
        assert!(said.contains("abcdef01"), "{said}");
        assert!(said.contains("second"), "{said}");
        assert!(said.contains("2 commits"), "{said}");
    }

    #[test]
    fn a_command_without_a_pull_number_stays_silent() {
        // Senza numero non c'è un istante di fusione da chiedere a GitHub:
        // meglio tacere che segnalare a tempo indovinato.
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh pr merge --auto"},
        });
        assert_eq!(after_merge(&payload).unwrap(), None);
    }

    #[test]
    fn a_command_that_does_not_merge_stays_silent() {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh pr create --fill"},
        });
        assert_eq!(after_merge(&payload).unwrap(), None);
    }

    #[test]
    fn a_broken_command_stays_silent() {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "gh pr merge 12 --squash"},
            "tool_response": {"is_error": true},
        });
        assert_eq!(after_merge(&payload).unwrap(), None);
    }
}
