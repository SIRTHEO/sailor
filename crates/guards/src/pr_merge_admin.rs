//! Blocca `gh pr merge … --admin`, che scavalca le branch protection.
//!
//! Porta di `hooks/block-pr-merge-admin.mjs`. Uno dei due ganci in Node, e per
//! misura **il più lento di tutti** (73 ms contro i 34-51 dei Python): siccome i
//! ganci di un evento girano in parallelo, era lui a dettare il muro dell'intera
//! catena `PreToolUse`.
//!
//! PERCHÉ NON BASTA UNA REGOLA DI PERMESSO. Le regole su `Bash` combaciano per
//! **prefisso**: `Bash(gh pr merge --admin:*)` copre solo la forma letterale, e
//! nella forma reale il flag arriva dopo il numero (`gh pr merge 262 --admin`).
//! Verificato il 28/07/2026: con la sola regola in `deny`, il comando passava.
//!
//! FAIL-CLOSED, e di proposito. Gli altri freni di questa configurazione
//! lasciano passare quando sono in dubbio, perché correggono un'abitudine. Qui
//! no: un blocco mancato fonde una richiesta con i controlli rossi, un blocco di
//! troppo costa una riga di spiegazione. Per lo stesso motivo si continua a
//! usare `\b` attorno a `merge` pur sapendo che accetta anche `merge-queue`:
//! sbaglia dal lato che non fa danno.

use hook_io::Decision;
use regex::Regex;
use std::sync::OnceLock;

fn splitter() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Ogni segmento di una catena va guardato per conto suo: il flag può stare
    // in un ramo qualsiasi, anche dentro una sostituzione di comando.
    RE.get_or_init(|| Regex::new(r"&&|\|\||;|\||\$\(|`|\n").unwrap())
}

fn merge_call() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bgh\s+pr\s+merge\b").unwrap())
}

fn admin_flag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `--admin` e `--admin=true`. Non tocca `--author`, che non ha questo
    // prefisso completo, né `--auto`, né `--administrator`.
    //
    // DIVERGENZA VOLUTA dall'originale, ed è una falla che il confronto ha
    // trovato: il Node pretendeva `=`, uno spazio o la fine riga dopo il flag,
    // quindi `X=$(gh pr merge 262 --admin)` **passava** — la parentesi non è
    // nessuna delle tre. Un bypass vero di un freno che difende una fusione
    // irreversibile su un repo condiviso.
    //
    // `[^\w-]` accetta la parentesi, l'apice e le virgolette, e continua a
    // escludere `--administrator` e `--admin-qualcosa`. Il prezzo è che ora
    // blocca anche il comando solo *citato* in una stringa: su questo freno è
    // il lato giusto in cui sbagliare.
    RE.get_or_init(|| Regex::new(r"\s--admin($|[^\w-])").unwrap())
}

pub fn judge(command: &str) -> Decision {
    let offending = splitter()
        .split(command)
        .any(|segment| merge_call().is_match(segment) && admin_flag().is_match(segment));

    if !offending {
        return Decision::Pass;
    }
    Decision::Block(
        "`gh pr merge --admin` scavalca le branch protection.\n\n\
         Il merge deve passare dai check, non sopra di essi. Se sono rossi la\n\
         strada è aggiustarli o rilanciare il job se è un flake, non forzare.\n\n\
         Se il bypass serve davvero, eseguilo tu con `! gh pr merge … --admin`."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_blocked(command: &str) -> bool {
        matches!(judge(command), Decision::Block(_))
    }

    #[test]
    fn it_blocks_the_flag_wherever_it_sits_in_the_line() {
        assert!(is_blocked("gh pr merge 262 --admin"));
        assert!(is_blocked("gh pr merge --admin 262"));
        assert!(is_blocked("gh pr merge 262 --admin=true"));
        assert!(is_blocked("gh pr merge 262 --admin --squash"));
    }

    #[test]
    fn it_looks_inside_every_branch_of_a_chain() {
        assert!(is_blocked("git push && gh pr merge 262 --admin"));
        assert!(is_blocked("echo x; gh pr merge 262 --admin"));
        assert!(is_blocked("gh pr checks || gh pr merge 262 --admin"));
        assert!(is_blocked("gh pr view\ngh pr merge 262 --admin"));
    }

    /// La falla che il confronto col Node ha scoperto: dentro una sostituzione
    /// di comando il flag chiude con `)`, e l'originale pretendeva `=`, spazio
    /// o fine riga. Passava. Qui non passa più — ed è l'unico punto in cui il
    /// porting **non** è equivalente, di proposito.
    #[test]
    fn it_closes_the_command_substitution_hole_the_node_version_had() {
        assert!(is_blocked("X=$(gh pr merge 262 --admin)"));
        assert!(is_blocked("echo `gh pr merge 262 --admin`"));
    }

    #[test]
    fn it_still_ignores_flags_that_merely_start_the_same_way() {
        assert!(!is_blocked("gh pr merge 262 --administrator"));
        assert!(!is_blocked("gh pr merge 262 --admin-dry-run"));
    }

    #[test]
    fn it_leaves_alone_what_only_looks_similar() {
        assert!(!is_blocked("gh pr merge 262"));
        assert!(!is_blocked("gh pr merge 262 --squash"));
        // `--author` e `--auto` condividono l'inizio ma non il flag intero
        assert!(!is_blocked("gh pr list --author theo"));
        assert!(!is_blocked("gh pr merge 262 --auto"));
    }

    /// Il gancio non guarda dentro le virgolette, quindi anche il comando
    /// *citato* viene bloccato. È un falso positivo reale e vive qui perché sia
    /// una scelta invece di una sorpresa: su questo freno vale la stessa regola
    /// del sottocomando somigliante — meglio bloccare di troppo.
    #[test]
    fn it_also_blocks_the_command_merely_quoted_in_a_string() {
        assert!(is_blocked("echo 'gh pr merge 262 --admin'"));
    }

    /// Divergenza nota e voluta rispetto a una lettura stretta: `\b` accetta il
    /// trattino, quindi un ipotetico `gh pr merge-queue … --admin` verrebbe
    /// bloccato. Su un freno di sicurezza è il lato giusto in cui sbagliare, e
    /// resta scritto qui perché non venga "corretto" per distrazione.
    #[test]
    fn it_errs_on_the_blocking_side_for_lookalike_subcommands() {
        assert!(is_blocked("gh pr merge-queue add --admin"));
    }
}
