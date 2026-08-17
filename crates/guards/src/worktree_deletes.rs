//! Concedere da sé le cancellazioni che colpiscono solo materiale usa-e-getta.
//!
//! Porto del giudizio di `skills/hooks/allow-worktree-deletes.py`. Qui non si
//! legge il disco tranne una domanda sola — «questo pezzo di percorso è un
//! collegamento?» — che arriva iniettata: seguire un collegamento significa
//! autorizzare in base a dove punta *adesso*, ed è l'unica cosa che a tavolino
//! non si può sapere.
//!
//! IL PROBLEMA. La regola R05 del plugin `claude-code-harness` intercetta ogni
//! cancellazione e risponde `ask`. Un `ask` emesso da un gancio vale anche sotto
//! `--dangerously-skip-permissions`, quindi congela le automazioni: nessuno
//! legge quella domanda, e il giro resta fermo finché una persona non passa
//! davanti allo schermo. Misurato l'11/08/2026 su `rm -rf "$R/packages/geo/dist"`,
//! una cartella di compilazione dentro una copia usa-e-getta.
//!
//! TACERE È LA RISPOSTA DI RISERVA. Un gancio che non capisce un comando non lo
//! autorizza: ogni ramo dubbio finisce in `Abstain`. È il motivo per cui questo
//! file non ha una via di mezzo — o si è letto tutto il comando, o si tace.
//!
//! RIDONDANZE VOLUTE, e conservate dal Python con la loro misura: quattro
//! controlli sopravvivono alla propria rimozione senza far cadere una prova,
//! perché un altro controllo li copre già. Restano scritti perché su un gancio
//! che **concede** permessi la prima linea dev'essere esplicita.

use crate::shell::split_words;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Sotto `workspaces/` servono almeno `<repo>/<worktree>/<qualcosa>`: due
/// livelli sarebbero il worktree intero, uno il repo intero. Il bersaglio
/// dev'essere materiale dentro una copia di lavoro, non la copia di lavoro.
pub const MIN_DEPTH: usize = 3;

/// Cosa risponde il gancio.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Si concede, col motivo che finisce nel registro e nel messaggio.
    Allow(String),
    /// Non si decide: la domanda resta dov'era.
    Abstain(String),
}

impl Verdict {
    pub fn reason(&self) -> &str {
        match self {
            Verdict::Allow(r) | Verdict::Abstain(r) => r,
        }
    }
    pub fn allowed(&self) -> bool {
        matches!(self, Verdict::Allow(_))
    }
}

/// I fatti: il comando, la radice delle copie, e la sola domanda per il disco.
pub struct Facts<'a> {
    pub command: &'a str,
    pub workspaces: &'a str,
    /// Vero se **quel** percorso è un collegamento simbolico. Iniettata perché
    /// è l'unica protezione che vuole un disco vero: un worktree può contenere
    /// un collegamento a `node_modules` del checkout canonico, e cancellare
    /// attraverso quel collegamento cancella roba del canonico.
    pub is_link: &'a dyn Fn(&str) -> bool,
}

fn rm_word() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^rm$|/rm$").unwrap())
}

fn assignment_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)=(\S+)\s*$").unwrap())
}

fn variable_ref() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap()
    })
}

fn indirect_delete() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bxargs\b|-delete\b|\bfind\b.*-exec").unwrap())
}

/// `(^|\s|/)rm\s` dell'originale: un `rm` che compare ma non si è saputo leggere.
fn bare_rm() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s|/)rm\s").unwrap())
}

/// Le variabili definite dal comando stesso, in forma `NOME=valore`.
///
/// Solo valori letterali: un valore che contiene una sostituzione non si
/// risolve leggendo, quindi non si risolve affatto.
pub fn assignments(command: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in command.replace(';', "\n").split('\n') {
        let Some(c) = assignment_line().captures(line) else {
            continue;
        };
        let raw = c.get(2).map(|m| m.as_str()).unwrap_or("");
        // `strip('"\'')` del Python toglie **entrambi** i caratteri da entrambe
        // le estremità, ripetutamente: non è «togli le virgolette se accoppiate».
        let val = raw.trim_matches(|ch| ch == '"' || ch == '\'');
        if val.contains("$(") || val.contains('`') {
            continue;
        }
        out.insert(c[1].to_string(), val.to_string());
    }
    out
}

/// `$R/x` e `${R}/x` diventano il percorso vero. `None` se non si può.
pub fn expand(piece: &str, variables: &HashMap<String, String>) -> Option<String> {
    let mut out = String::new();
    let mut last = 0usize;
    for c in variable_ref().captures_iter(piece) {
        let whole = c.get(0).unwrap();
        let name = c
            .get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        // Una variabile che il comando non definisce non si indovina: il
        // bersaglio diventa illeggibile, e un bersaglio illeggibile non passa.
        let value = variables.get(name)?;
        out.push_str(&piece[last..whole.start()]);
        out.push_str(value);
        last = whole.end();
    }
    out.push_str(&piece[last..]);
    Some(out)
}

/// `os.path.normpath` ridotto a ciò che serve: `.` e le barre doppie.
///
/// I `..` non arrivano mai qui — `inside_worktree` li rifiuta prima — ma si
/// risolvono comunque, perché un normpath che li lasciasse passare renderebbe
/// quel rifiuto l'unica difesa invece di una delle due.
fn normpath(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for piece in path.split('/') {
        match piece {
            "" | "." => continue,
            ".." => {
                if matches!(parts.last(), Some(&p) if p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// Vero solo se il percorso è materiale interno a una copia di lavoro.
pub fn inside_worktree(path: &str, workspaces: &str, is_link: &dyn Fn(&str) -> bool) -> bool {
    // Ridondanza voluta n°4: l'obbligo del percorso assoluto è implicato dal
    // confronto sul prefisso, e resta scritto perché è la prima linea.
    if path.is_empty() || !path.starts_with('/') {
        return false;
    }
    // Prima della normalizzazione, come l'originale: un `..` che dopo il
    // calcolo resta dentro `workspaces` cambierebbe comunque copia di lavoro,
    // scavalcando quella del giro in corso per entrare in quella di un altro.
    if path.split('/').any(|p| p == "..") {
        return false;
    }
    let norm = normpath(path);
    let root = normpath(workspaces);
    let prefisso = format!("{root}/");
    if !norm.starts_with(&prefisso) {
        return false;
    }
    let rest: Vec<&str> = norm[prefisso.len()..].split('/').collect();
    if rest.len() < MIN_DEPTH || rest.iter().any(|p| p.is_empty()) {
        return false;
    }
    // Un collegamento in qualunque punto della catena porta fuori.
    let mut current = root;
    for piece in rest {
        current = format!("{current}/{piece}");
        if is_link(&current) {
            return false;
        }
    }
    true
}

/// Gli argomenti di un `rm` in questa riga. `None` se la riga non è giudicabile.
pub fn targets(line: &str, variables: &HashMap<String, String>) -> Option<Vec<String>> {
    let pieces = split_words(line)?;
    if pieces.is_empty() || !rm_word().is_match(&pieces[0]) {
        return None;
    }
    let mut out = Vec::new();
    for p in &pieces[1..] {
        if p == "--" || p.starts_with('-') {
            continue;
        }
        out.push(expand(p, variables)?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn decide(f: &Facts) -> Verdict {
    if f.command.trim().is_empty() {
        return Verdict::Abstain("comando vuoto".into());
    }
    // Ridondanza voluta n°1: una sostituzione può nascondere qualunque cosa, e
    // non si giudica un testo che non si è letto per intero.
    if f.command.contains("$(") || f.command.contains('`') {
        return Verdict::Abstain("il comando contiene una sostituzione: non giudicabile".into());
    }
    // Ridondanza voluta n°3: `rm` non sarebbe la prima parola comunque.
    if indirect_delete().is_match(f.command) {
        return Verdict::Abstain("cancellazione indiretta (xargs/find): non giudicabile".into());
    }

    let variables = assignments(f.command);
    // `&&`, `||` e `|` separano comandi quanto `;` e il ritorno a capo: una riga
    // sola può contenerne diversi, e vanno guardati uno per uno.
    let mut found: Vec<String> = Vec::new();
    for line in split_commands(f.command) {
        match targets(line.trim(), &variables) {
            Some(b) => found.extend(b),
            None => {
                if bare_rm().is_match(&line) {
                    return Verdict::Abstain("un rm che non so leggere per intero".into());
                }
            }
        }
    }

    if found.is_empty() {
        return Verdict::Abstain("nessuna cancellazione da autorizzare".into());
    }
    if let Some(fuori) = found
        .iter()
        .find(|t| !inside_worktree(t, f.workspaces, f.is_link))
    {
        return Verdict::Abstain(format!(
            "bersaglio fuori dai worktree usa-e-getta: {fuori}"
        ));
    }
    Verdict::Allow(format!(
        "{} bersaglio/i, tutti dentro {}",
        found.len(),
        f.workspaces
    ))
}

/// `re.split(r'&&|\|\||[;\n|]', command)` dell'originale.
///
/// Scritto a mano e non con una regex perché `&&` e `||` vanno riconosciuti
/// prima dei singoli caratteri: cercando `|` per primo, `||` diventerebbe due
/// separatori vuoti — stesso esito qui, ma per un motivo che non regge al
/// prossimo ritocco.
fn split_commands(command: &str) -> Vec<String> {
    let bytes: Vec<char> = command.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let due: String = bytes[i..(i + 2).min(bytes.len())].iter().collect();
        if due == "&&" || due == "||" {
            out.push(std::mem::take(&mut current));
            i += 2;
            continue;
        }
        if matches!(bytes[i], ';' | '\n' | '|') {
            out.push(std::mem::take(&mut current));
            i += 1;
            continue;
        }
        current.push(bytes[i]);
        i += 1;
    }
    out.push(current);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "/home/someone/orca/workspaces";

    fn giudica(command: &str) -> Verdict {
        decide(&Facts {
            command,
            workspaces: W,
            is_link: &|_| false,
        })
    }

    #[test]
    fn il_caso_vero_che_ha_congelato_il_giro_864() {
        let v = giudica(&format!("rm -rf {W}/packages/theo-auto-pg/packages/geo/dist"));
        assert!(v.allowed(), "{v:?}");
        assert!(v.reason().contains("1 bersaglio/i"), "{v:?}");
    }

    #[test]
    fn una_variabile_definita_dal_comando_si_risolve() {
        assert!(giudica(&format!("R={W}/packages/x\nrm -rf \"$R/packages/geo/dist\"")).allowed());
        assert!(giudica(&format!("R={W}/suite/wt\nrm -rf \"${{R}}/node_modules\"")).allowed());
    }

    #[test]
    fn una_variabile_che_il_comando_non_definisce_non_si_indovina() {
        assert!(!giudica("rm -rf \"$NONDEFINITA/x\"").allowed());
    }

    #[test]
    fn piu_bersagli_dentro_passano_e_uno_fuori_ferma_tutto() {
        assert!(giudica(&format!("rm -rf {W}/a/b/x {W}/a/b/y")).allowed());
        let v = giudica(&format!("rm -rf {W}/a/b/x /tmp/y"));
        assert!(!v.allowed());
        assert!(v.reason().contains("/tmp/y"), "il motivo nomina il colpevole: {v:?}");
    }

    #[test]
    fn senza_r_resta_una_cancellazione() {
        // R05 intercetta anche `rm` semplice: coprirla è il punto del gancio.
        assert!(giudica(&format!("rm {W}/a/b/file.txt")).allowed());
    }

    #[test]
    fn la_profondita_protegge_repo_e_worktree_interi() {
        assert!(!giudica(&format!("rm -rf {W}")).allowed());
        assert!(!giudica(&format!("rm -rf {W}/suite")).allowed());
        assert!(!giudica(&format!("rm -rf {W}/suite/wt")).allowed());
        // Tre livelli è il minimo che passa, ed è il confine esatto.
        assert!(giudica(&format!("rm -rf {W}/suite/wt/dist")).allowed());
    }

    #[test]
    fn il_checkout_canonico_non_e_usa_e_getta() {
        assert!(!giudica("rm -rf /home/someone/other-repo/work/suite/dist").allowed());
    }

    #[test]
    fn una_cartella_che_somiglia_a_workspaces_non_e_workspaces() {
        // Isola il confronto sul prefisso: la sola profondità lo lascerebbe
        // passare, perché di livelli ne ha abbastanza.
        assert!(!giudica("rm -rf /home/someone/orca/workspacesXX/aa/bb").allowed());
    }

    #[test]
    fn la_risalita_non_passa_nemmeno_quando_resta_dentro() {
        assert!(!giudica(&format!("rm -rf {W}/a/b/../../../../etc")).allowed());
        // Questo isola il divieto di `..`: dopo la normalizzazione il percorso
        // è dentro workspaces e abbastanza profondo, quindi passerebbe da tutti
        // gli altri controlli — ma ha scavalcato la propria copia di lavoro per
        // entrare in quella di un altro giro.
        assert!(!giudica(&format!("rm -rf {W}/repo/wt/../../altro/wt2/dist")).allowed());
    }

    #[test]
    fn si_guarda_anche_cio_che_viene_dopo_il_separatore() {
        for sep in ["&&", ";", "||", "|"] {
            let v = giudica(&format!("rm -rf {W}/a/b/x {sep} rm -rf /etc/passwd"));
            assert!(!v.allowed(), "separatore {sep}: {v:?}");
        }
    }

    #[test]
    fn cio_che_non_si_sa_leggere_non_si_autorizza() {
        assert!(!giudica("rm -rf $(cat lista)").allowed());
        assert!(!giudica(&format!("find {W}/a/b -name \"*.o\" -delete")).allowed());
        assert!(!giudica("echo x | xargs rm -rf").allowed());
        assert!(!giudica("npm test").allowed());
        assert!(!giudica("").allowed());
        // Un bersaglio relativo: la cartella di lavoro non si conosce, quindi
        // non si decide.
        assert!(!giudica(&format!("cd {W}/a/b && rm -rf dist")).allowed());
    }

    #[test]
    fn un_collegamento_nella_catena_porta_fuori() {
        // È la protezione che a tavolino non si vede: un worktree può contenere
        // un collegamento a `node_modules` del checkout canonico, e cancellare
        // attraverso quel collegamento cancella roba del canonico.
        let link = format!("{W}/repo/wt/node_modules");
        let f = |command: &str| {
            decide(&Facts {
                command,
                workspaces: W,
                is_link: &|p: &str| p == link,
            })
        };
        assert!(f(&format!("rm -rf {W}/repo/wt/dist")).allowed(), "una cartella vera passa");
        assert!(!f(&format!("rm -rf {W}/repo/wt/node_modules")).allowed());
        assert!(
            !f(&format!("rm -rf {W}/repo/wt/node_modules/qualcosa")).allowed(),
            "nemmeno passando attraverso il collegamento"
        );
    }

    #[test]
    fn il_normpath_si_comporta_come_quello_di_python() {
        assert_eq!(normpath("/a//b/./c"), "/a/b/c");
        assert_eq!(normpath("/a/b/../c"), "/a/c");
        assert_eq!(normpath("/a/../.."), "/");
        assert_eq!(normpath("a/b/../c"), "a/c");
        assert_eq!(normpath("/"), "/");
    }

    #[test]
    fn le_opzioni_non_sono_bersagli() {
        // `--` e le opzioni si saltano; se resta solo quello, non c'è niente da
        // autorizzare e si tace invece di concedere il vuoto.
        assert!(!giudica("rm -rf --").allowed());
        assert!(giudica(&format!("rm -rf -- {W}/a/b/c")).allowed());
    }
}
