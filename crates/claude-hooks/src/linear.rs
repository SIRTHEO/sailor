//! La parte con stato del divieto su Linear: il permesso di Theo e il registro.
//!
//! Il giudizio sta in `guards::linear_readonly` ed è puro. Qui c'è solo ciò che
//! tocca il disco, per una ragione precisa: la parte che decide dev'essere
//! provabile senza mettere le mani sullo stato della macchina, e il confronto
//! con l'originale deve poter girare su una `HOME` finta.

use guards::linear_readonly as linear;
use guards::linear_readonly::{Valve, Verdict};
use hook_io::{python_json, Decision, HookInput};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Gli strumenti che scrivono un file senza passare dalla shell.
const FILE_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Il permesso che scrive Theo con `~/.claude/scripts/permesso-linear.sh`, da un
/// terminale fuori da Claude Code.
///
/// PERCHÉ NON BASTA `OK_UTENTE=1`: quella valvola la digita l'agente, e un
/// permesso che chi lo usa può concedersi da solo non è un permesso. Questo file
/// invece l'agente non può scriverlo — sta nel nucleo protetto.
fn permission_path() -> PathBuf {
    home().join(".claude").join("state").join("linear-permesso.json")
}

/// Dove finiscono i tentativi. La deviazione vale solo verso una cartella
/// temporanea: non è una difesa (chi devia il registro ha comunque il comando
/// negato), è il minimo perché la variabile non diventi un modo comodo per
/// lavorare senza lasciare traccia.
fn register_path() -> PathBuf {
    if let Ok(chosen) = std::env::var("LINEAR_GANCIO_REGISTRO") {
        let absolute = std::fs::canonicalize(&chosen)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| chosen.clone());
        let temporary = ["/tmp/", "/private/tmp/"]
            .iter()
            .map(|s| s.to_string())
            .chain(std::env::var("TMPDIR").ok())
            .any(|prefix| !prefix.is_empty() && absolute.starts_with(&prefix));
        if temporary {
            return PathBuf::from(chosen);
        }
    }
    home()
        .join(".claude")
        .join("state")
        .join("linear-sola-lettura.jsonl")
}

struct Permission {
    scope: String,
    cards: Vec<String>,
}

/// Il permesso in corso, se c'è ed è ancora valido.
///
/// Qualunque guasto di lettura vale come «nessun permesso»: qui il fail-closed
/// non blocca il sistema, nega soltanto una scrittura su Linear, che è
/// esattamente il comportamento voluto.
fn valid_permission() -> Option<Permission> {
    let text = std::fs::read_to_string(permission_path()).ok()?;
    let d: Value = serde_json::from_str(&text).ok()?;
    let expires = match d.get("scade")? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.parse().ok()?,
        _ => return None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    if expires < now {
        return None;
    }
    let scope = d.get("ambito")?.as_str()?.to_string();
    if scope != "scritture" && scope != "done" {
        return None;
    }
    let cards = d
        .get("schede")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_uppercase()))
                .collect()
        })
        .unwrap_or_default();
    Some(Permission { scope, cards })
}

/// Il permesso di Theo copre questa scrittura?
fn authorise(command: &str) -> (bool, String) {
    let Some(p) = valid_permission() else {
        return (
            false,
            "serve il permesso di Theo: da un terminale fuori da Claude Code, \
             `~/.claude/scripts/permesso-linear.sh`"
                .to_string(),
        );
    };
    if !linear::closes_a_card(command) {
        return (true, format!("permesso «{}» di Theo", p.scope));
    }
    // Da qui in giù si sta chiudendo una scheda: punto 2 del mandato.
    if p.scope != "done" {
        return (
            false,
            "il permesso in corso vale per le modifiche, non per spostare una scheda in Done"
                .to_string(),
        );
    }
    let mut named: Vec<String> = linear::named_cards(command)
        .into_iter()
        .map(|c| c.to_uppercase())
        .collect();
    named.sort();
    named.dedup();
    if p.cards.is_empty() {
        return (false, "il permesso per Done non nomina nessuna scheda".to_string());
    }
    if named.is_empty() {
        return (
            false,
            "il comando non nomina la scheda che sta chiudendo".to_string(),
        );
    }
    let outside: Vec<&String> = named.iter().filter(|c| !p.cards.contains(c)).collect();
    if !outside.is_empty() {
        let list: Vec<&str> = outside.iter().map(|s| s.as_str()).collect();
        return (
            false,
            format!("il permesso di Theo non copre {}", list.join(", ")),
        );
    }
    (true, format!("permesso «done» di Theo per {}", named.join(", ")))
}

/// Una riga nel registro. Le chiavi e il loro ordine sono quelli del Python:
/// l'archivio conta 13.560 righe, e chi lo interroga non deve accorgersi di
/// quale delle due implementazioni ha scritto.
fn note(outcome: &str, reason: &str, tool: &str, session: &str, cwd: &str, command: &str) {
    let path = register_path();
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let line = json!({
        "quando": hook_io::local_time::now_local_iso8601(),
        "esito": outcome,
        "motivo": reason,
        "strumento": tool,
        "sessione": session,
        "cwd": cwd,
        // 400 **caratteri**, non byte: il Python affetta una stringa
        "comando": command.chars().take(400).collect::<String>(),
    });
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", python_json::dumps_unicode(&line));
    }
}

/// Il file di configurazione nominato è **nostro**, o solo omonimo?
///
/// UN PERCORSO SENZA CARTELLE RESTA NOSTRO: `settings.json` scritto nudo
/// quasi sempre significa «quello della cartella dove sto», e la cartella dove
/// si lavora è la nostra. Nel dubbio il freno tiene — sbagliare per eccesso
/// qui costa un messaggio, sbagliare per difetto costa la valvola.
///
/// Una copia di configurazione dichiarata a mano vale come la nostra: è la
/// stessa cassa, spostata per una prova a due bracci (`CLAUDE_CONFIG_DIR`).
fn config_file_is_ours(path: &str) -> bool {
    if !path.contains('/') {
        return true;
    }
    if path.contains("/.claude/") || path.contains("/.claude-") {
        return true;
    }
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let dir = dir.trim_end_matches('/');
        if !dir.is_empty() && path.starts_with(dir) {
            return true;
        }
    }
    false
}

fn refusal(reason: &str) -> Decision {
    // Due rifiuti, non uno con la ragione interpolata: il divieto protegge due
    // cose diverse, e fino al 18/08/2026 chi toccava `settings.json` riceveva
    // per intero l'omelia sull'elenco chiuso dei sottocomandi Linear, col
    // motivo vero relegato fra parentesi. Un messaggio che parla d'altro si
    // legge come un difetto del freno, e chi lo riceve gira intorno al freno.
    if linear::is_protected_file(reason) {
        return Decision::Deny(format!(
            "Questo file regge il divieto di scrittura su Linear ({reason}), e non si modifica \
             dall'interno di una sessione: una valvola che autorizza il proprio smontaggio non è \
             una valvola. Leggerlo, cercarlo e lanciarne le prove resta libero. Se il cambiamento \
             serve davvero, dillo a Theo e lo scrive lui da un terminale fuori da Claude Code — \
             per la configurazione dei ganci c'è il suo permesso scritto, per il nucleo del \
             divieto non c'è niente, di proposito."
        ));
    }
    Decision::Deny(format!(
        "Linear è in sola lettura per le automazioni ({reason}). Lo stato delle schede lo muove \
         Theo, e nessun giro può spostare una scheda in Done — mandato dell'11/08/2026. Passano \
         solo le letture di un elenco chiuso: orca linear list | list-issues | issue | search | \
         team … | project list, e linear issues | issue show | projects | project show | \
         milestones | labels | roadmap. Lo stato del lavoro va sulla scheda Orca del worktree, \
         non su Linear. Se Theo ha autorizzato questa scrittura per iscritto in questa \
         conversazione, rilancia con OK_UTENTE=1 davanti al comando; se non l'ha fatto, chiedi \
         invece di concedertelo da solo — l'uso della valvola resta scritto nel registro."
    ))
}

/// Il gancio intero. Restituisce il codice di uscita: sempre 0, perché il
/// rifiuto viaggia sull'altro canale (`permissionDecision: deny` su stdout).
pub fn run(input: &HookInput) -> i32 {
    let tool = input.tool_name.clone().unwrap_or_default();
    let session = input.session_id.clone().unwrap_or_default();
    let cwd = input.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let empty = json!({});
    let tool_input = input.tool_input.as_ref().unwrap_or(&empty);

    if FILE_TOOLS.contains(&tool.as_str()) {
        let path = tool_input
            .get("file_path")
            .or_else(|| tool_input.get("notebook_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some((reason, valve)) = linear::reason_on_file(path) else {
            return 0;
        };
        // UN OMONIMO NON È IL NOSTRO FILE. Il criterio del nucleo riconosce la
        // configurazione dal **solo nome**, e sotto casa dell'utente ci sono
        // almeno tre programmi che chiamano `settings.json` la propria: il
        // 26/08/2026 questo ha negato di toccare quella della CLI di Google,
        // impedendo di dare a un motore esterno la mappa e l'indice — cioè il
        // lavoro che gli fa fare meno errori.
        //
        // La correzione sta qui e non nel nucleo per una ragione sola: il
        // nucleo non si modifica dall'interno di una sessione, di proposito, e
        // quella regola vale anche quando a chiedere il cambiamento è un
        // difetto del nucleo stesso. Qui si restringe **cosa arriva** al
        // rifiuto, senza toccare cosa il nucleo considera protetto.
        //
        // Il nucleo (`Valve::Core`) non passa di qui: quei file sono nostri e
        // basta, ovunque si trovino.
        if valve == Valve::UserDeclared && !config_file_is_ours(path) {
            return 0;
        }
        // `json.dumps(ingresso)` senza `ensure_ascii=False`: qui il Python usa
        // la forma predefinita, mentre la riga che lo contiene no. La differenza
        // è visibile solo su un percorso accentato, e c'è.
        let shown = python_json::dumps(tool_input);
        // Sul file di configurazione vale il permesso di Theo, non la valvola:
        // `Write` non ha una riga di comando dove scriverla.
        if valve == Valve::UserDeclared && valid_permission().is_some() {
            note("autorizzato", &reason, &tool, &session, &cwd, &shown);
            return 0;
        }
        let outcome = if valve == Valve::Core { "negato-nucleo" } else { "negato" };
        note(outcome, &reason, &tool, &session, &cwd, &shown);
        return hook_io::emit("linear-readonly", &refusal(&reason));
    }

    if tool != "Bash" {
        // Per nome (`mcp__linear__create_issue`) e per contenuto: uno strumento
        // generico che esegue azioni altrui — un ponte, un runner di flussi —
        // porta il comando dentro i propri argomenti, e il nome non lo dice.
        let text = python_json::dumps_unicode(tool_input);
        let mut reason = linear::reason_mcp(&tool);
        if reason.is_none() && tool.starts_with("mcp__") {
            reason = linear::names_write(&text)
                .map(|inside| format!("{inside} dentro gli argomenti di {tool}"));
        }
        let Some(reason) = reason else { return 0 };
        let shown: String = text.chars().take(400).collect();
        let (ok, why) = authorise(&format!("{tool} {text}"));
        if ok {
            note("autorizzato", &format!("{reason} — {why}"), &tool, &session, &cwd, &shown);
            return 0;
        }
        let full = format!("{reason} — {why}");
        note("negato", &full, &tool, &session, &cwd, &shown);
        return hook_io::emit("linear-readonly", &refusal(&full));
    }

    let command = input.bash_command();
    if command.is_empty() {
        return 0;
    }
    let (mut reason, valve, segment) = match linear::judge_bash(command) {
        Verdict::Pass => return 0,
        // La valvola è stata usata: il comando passa e l'uso resta scritto. È
        // la promessa che il messaggio di rifiuto fa a chi la digita, e senza
        // questa riga sarebbe una promessa vuota.
        Verdict::Declared { reason, .. } => {
            note("autorizzato", &reason, &tool, &session, &cwd, command);
            return 0;
        }
        // Il permesso di Theo non viene nemmeno consultato: vedi `Verdict::Sealed`.
        Verdict::Sealed { reason } => {
            note("negato", &reason, &tool, &session, &cwd, command);
            return hook_io::emit("linear-readonly", &refusal(&reason));
        }
        Verdict::Refused {
            reason,
            valve,
            segment,
        } => (reason, valve, segment),
    };

    // Le scritture su Linear le sblocca solo il permesso di Theo, e il passaggio
    // a Done chiede un permesso che nomina la scheda.
    if valve == Valve::TheoPermission {
        let (ok, why) = authorise(&segment);
        if ok {
            note(
                "autorizzato",
                &format!("{reason} — {why}"),
                &tool,
                &session,
                &cwd,
                command,
            );
            return 0;
        }
        reason = format!("{reason} — {why}");
    }
    let outcome = if valve == Valve::Core { "negato-nucleo" } else { "negato" };
    note(outcome, &reason, &tool, &session, &cwd, command);
    hook_io::emit("linear-readonly", &refusal(&reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(reason: &str) -> String {
        match refusal(reason) {
            Decision::Deny(m) => m,
            _ => panic!("refusal must deny"),
        }
    }

    /// Il rifiuto deve nominare ciò che ha visto. Sul file protetto parlava di
    /// Linear, e un messaggio fuori tema si legge come un freno rotto.
    #[test]
    fn the_refusal_on_a_protected_file_does_not_lecture_about_linear() {
        let m = text("riscrittura di la configurazione dei ganci (settings.json)");
        assert!(m.contains("regge il divieto"), "{m}");
        assert!(m.contains("settings.json"), "{m}");
        assert!(!m.contains("orca linear list"), "{m}");
        assert!(!m.contains("OK_UTENTE"), "{m}");
    }

    #[test]
    fn the_refusal_on_a_linear_subcommand_still_says_which_reads_pass() {
        let m = text("sottocomando fuori dall'elenco delle letture");
        assert!(m.contains("orca linear list"), "{m}");
        assert!(m.contains("OK_UTENTE"), "{m}");
        assert!(!m.contains("regge il divieto"), "{m}");
    }

    // ── un omonimo non è il nostro file ────────────────────────────────
    //
    // Il caso vero del 26/08/2026: il freno ha negato di toccare la
    // configurazione della CLI di Google, riconoscendola dal solo nome.

    #[test]
    fn the_settings_of_another_program_are_not_ours() {
        assert!(!config_file_is_ours("/home/someone/.gemini/settings.json"));
        assert!(!config_file_is_ours("/home/someone/.config/qualcosa/settings.json"));
        assert!(!config_file_is_ours("/Applications/Roba.app/settings.json"));
    }

    #[test]
    fn our_own_settings_stay_protected() {
        assert!(config_file_is_ours("/home/someone/.claude/settings.json"));
        assert!(config_file_is_ours("/home/someone/.claude/settings.local.json"));
    }

    /// Un percorso senza cartelle resta nostro: è la cartella dove si lavora,
    /// e nel dubbio il freno tiene.
    #[test]
    fn a_bare_name_is_still_ours() {
        assert!(config_file_is_ours("settings.json"));
    }

    /// Una copia di configurazione dichiarata a mano è la stessa cassa,
    /// spostata: vale come la nostra.
    #[test]
    fn a_declared_config_copy_is_ours_too() {
        let key = "CLAUDE_CONFIG_DIR";
        let before = std::env::var(key).ok();
        std::env::set_var(key, "/tmp/prova-config-ab");
        let verdict = config_file_is_ours("/tmp/prova-config-ab/settings.json");
        match before {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        assert!(verdict, "una copia dichiarata deve restare protetta");
    }
}
