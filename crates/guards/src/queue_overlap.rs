//! Quali voci di coda parlano della stessa cosa, e quali sono troppo vecchie
//! per essere credute sulla parola.
//!
//! IL DIFETTO. Le voci in `state/plancia/segnalazioni/` sono affermazioni
//! datate, scritte da agenti che non si leggono fra loro. Il 24/08/2026 una
//! voce prescriveva al punto 1 un tetto di memoria con `ulimit`, e un'altra
//! misurava che su Darwin `ulimit` non si imposta: chi leggeva la prima e si
//! fermava lì faceva la cosa sbagliata. Fra file diversi non c'era nessun
//! confronto.
//!
//! COSA VUOL DIRE «LA STESSA COSA», ed è la scelta che regge tutto: i
//! **soggetti** che una voce nomina fra apici inversi — il file, il comando,
//! l'identificatore. Due voci si toccano quando condividono almeno
//! [`MIN_SHARED_SUBJECTS`] soggetti **rari**, dove raro è misurato sul corpus
//! stesso: un soggetto nominato da più di un ottavo delle voci è il vocabolario
//! di casa (`git`, `settings.json`) e non dice niente su chi parla di cosa.
//!
//! COSA SI PERDE, dichiarato perché chi vede «nessuna coppia» sappia di cosa
//! non è coperto:
//! - **il testo non citato non conta**: due voci che discutono la stessa cosa
//!   solo in prosa italiana non si incontrano mai;
//! - **i blocchi recintati sono prova, non soggetto**, e vengono tolti prima:
//!   l'uscita incollata di un comando non iscrive nessuno;
//! - **non si giudica il senso**: questo modulo dice che due voci parlano della
//!   stessa cosa, non che si contraddicano. La contraddizione la vede chi
//!   legge, e senza questo non la vedeva affatto;
//! - **un soggetto comune sparisce**: due voci che condividono solo
//!   `settings.json` restano scollegate, ed è voluto — collegarle collegherebbe
//!   tutto con tutto.
//!
//! LA CURA DEL RUMORE. Non si aprono voci nuove per raccontare che due voci
//! vecchie litigano: il rilievo si scrive **dentro** le voci interessate, in un
//! blocco delimitato che si rigenera a ogni passata. Chi tocca il disco sta in
//! `claude-hooks/src/queue_freshness.rs`; qui non si legge e non si scrive
//! niente.

use crate::memory_anchor::frontmatter;
use crate::stale_facts::{Date, PATH_EXTENSIONS};
use std::collections::{BTreeMap, BTreeSet};

/// Quanti soggetti rari devono coincidere perché due voci si tocchino.
///
/// Uno solo non basta: su un corpus di casa un solo nome in comune capita per
/// vicinanza di argomento, e con la soglia a uno le coppie si moltiplicano
/// finché nessuno le guarda più.
pub const MIN_SHARED_SUBJECTS: usize = 2;

/// Un soggetto nominato da più voci di così è vocabolario comune, non un
/// argomento: si scarta prima del confronto.
///
/// È la frequenza documentale ridotta a due interi, per non portarsi dietro
/// una libreria. Il valore è **misurato, non scelto a tavolino**: su 133 voci
/// vere una soglia larga dava 200 coppie, e il campione mostrava che a legarle
/// erano `write`, `edit`, `cat`, `claude` — il vocabolario del mestiere.
pub const RARE_SUBJECT_FLOOR: usize = 4;

/// La stessa soglia, in proporzione, per quando la coda cresce: un soggetto
/// nominato da più di un trentaduesimo delle voci è comune anche se sono
/// tante. Vale la più larga delle due, così su un corpus piccolo la
/// proporzione non cancella tutto — con dieci voci un terzo di voce non è una
/// soglia.
pub const COMMON_SUBJECT_DIVISOR: usize = 32;

/// Oltre questa età in giorni, una voce ancora aperta non è più un fatto: è
/// un'ipotesi che nessuno ha rimisurato.
///
/// Due giorni perché è il punto in cui le voci di questa coda hanno cominciato
/// a mentire: quelle ferme da tre giorni il 24/08/2026 sono esattamente quelle
/// che hanno guidato lavoro sbagliato.
pub const STALE_DAYS: i64 = 2;

/// L'apertura del blocco rigenerabile scritto dentro una voce.
pub const BLOCK_OPEN: &str = "<!-- freschezza-coda: inizio -->";
/// La chiusura dello stesso blocco.
pub const BLOCK_CLOSE: &str = "<!-- freschezza-coda: fine -->";

// ─── I soggetti ──────────────────────────────────────────────────────────────

/// Toglie i blocchi recintati da tre apici inversi.
///
/// Sono l'uscita incollata di un comando: la prova di ciò che la voce afferma,
/// non l'argomento di cui parla. Tenerli dentro iscriveva fra i soggetti ogni
/// parola di un registro d'errore, e due voci che incollano lo stesso errore di
/// `cargo` risultavano parlare della stessa cosa.
///
/// Serve anche alla lettura degli apici singoli: una recinzione sposta la
/// parità di `split` e fa leggere come «dentro» tutto ciò che sta fuori.
pub fn strip_fences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            inside = !inside;
            continue;
        }
        if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Le estensioni che rendono un token un file, prese da `stale_facts` invece di
/// riscriverle: due elenchi paralleli divergono alla prima aggiunta.
fn has_known_extension(token: &str) -> bool {
    token
        .rsplit_once('.')
        .is_some_and(|(_, ext)| PATH_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// L'ultimo segmento di un percorso.
///
/// `<scratchpad>/rss-watchdog.sh` e `rust/tools/rss-watchdog.sh` nominano lo
/// stesso file per chi legge, e devono valere lo stesso soggetto. Per questo
/// non si riusa `memory_anchor::citations`, che scarterebbe il primo dei due
/// perché non esisterà mai su disco: quella funzione risponde a «questo file
/// c'è?», questa a «di cosa parla la voce».
fn basename(token: &str) -> &str {
    token.rsplit('/').find(|s| !s.is_empty()).unwrap_or(token)
}

/// Toglie la posizione in coda a un riferimento: `duplication.rs:332` e
/// `duplication.rs:12-30` parlano dello stesso file.
fn strip_locator(token: &str) -> &str {
    let Some((head, tail)) = token.rsplit_once(':') else {
        return token;
    };
    if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return head;
    }
    token
}

/// Da un token grezzo al soggetto normalizzato, o niente se non è un soggetto.
fn token_subject(raw: &str) -> Option<String> {
    let token = raw.trim_matches(|c: char| matches!(c, '(' | ')' | '"' | '\'' | ',' | ';' | '.'));
    let token = basename(strip_locator(token));
    let token = token.trim_end_matches("()");
    if token.len() < 3 || token.len() > 60 {
        return None;
    }
    if !token
        .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
    {
        return None;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return None;
    }
    if is_identity(token) {
        return None;
    }
    Some(token.to_ascii_lowercase())
}

/// Vero per un token che è un'identità e non un argomento: l'identificativo di
/// una sessione (`ffc0d846`), una revisione di git (`7ad541b`).
///
/// Due voci che nominano la stessa sessione hanno **lo stesso autore**, non lo
/// stesso soggetto: sulle voci vere era il primo produttore di coppie finte.
/// La cifra è obbligatoria perché senza, parole italiane fatte di sole lettere
/// esadecimali — `facce`, `bacca` — sparirebbero con loro.
fn is_identity(token: &str) -> bool {
    token.len() >= 6
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_digit())
}

/// Vero per una parola che ha la forma di un comando da riga di comando.
///
/// Serve a distinguere `ulimit -v` — dove la testa è il soggetto — da
/// `stato: aperta`, che fra apici inversi è un pezzo di formato citato e non
/// nomina niente.
fn is_command_word(word: &str) -> bool {
    !word.is_empty()
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

/// I soggetti che un singolo pezzo fra apici inversi nomina.
fn push_subjects(span: &str, out: &mut BTreeSet<String>) {
    if span.is_empty() || span.len() > 200 {
        return;
    }
    let words: Vec<&str> = span.split_whitespace().collect();
    if words.len() == 1 {
        if let Some(s) = token_subject(words[0]) {
            out.insert(s);
        }
        return;
    }
    // Una riga di comando: la testa è il soggetto, e la testa col sottocomando
    // lo è a sua volta — `cargo` è vocabolario di casa, `cargo mutants` no.
    if is_command_word(words[0]) {
        if let Some(head) = token_subject(words[0]) {
            if let Some(sub) = words
                .get(1)
                .filter(|w| is_command_word(w))
                .and_then(|w| token_subject(w))
            {
                out.insert(format!("{head} {sub}"));
            }
            out.insert(head);
        }
    }
    // E ogni parola che è un file resta un soggetto ovunque stia nella riga:
    // `WAKEUP_DATE_OVERRIDE=… ~/.claude/scripts/nine-am-wakeup.sh --dry-run` ha
    // una testa che non è un comando, e il file che nomina conta lo stesso.
    for w in &words {
        if has_known_extension(strip_locator(w)) {
            if let Some(s) = token_subject(w) {
                out.insert(s);
            }
        }
    }
}

/// I soggetti di una voce: ciò che nomina fra apici inversi, normalizzato.
///
/// Il blocco rigenerabile viene tolto prima, e non è un dettaglio: il blocco
/// nomina le voci gemelle e i soggetti in comune, quindi lasciarlo dentro
/// farebbe crescere l'insieme a ogni passata finché tutto tocca tutto.
pub fn subjects(text: &str) -> BTreeSet<String> {
    let clean = strip_fences(&strip_block(text));
    let mut out = BTreeSet::new();
    for span in clean.split('`').skip(1).step_by(2) {
        push_subjects(span.trim(), &mut out);
    }
    out
}

// ─── La voce ─────────────────────────────────────────────────────────────────

/// Una voce di coda ridotta a ciò che serve per confrontarla con le altre.
#[derive(Debug, Clone)]
pub struct Voice {
    /// Il nome del file, che è come le voci si citano fra loro.
    pub name: String,
    /// La prima parola di `stato:`, o vuoto se il campo manca.
    pub state: String,
    pub subjects: BTreeSet<String>,
    /// La data più recente dichiarata nel frontmatter: l'ultima volta che
    /// qualcuno ha guardato questa voce e l'ha detto.
    pub last_touched: Option<Date>,
}

/// La prima parola di `stato:`, che è il valore; il resto della riga è
/// commento per chi legge.
///
/// Lo dice il formato della coda, e il 21/08/2026 una voce riaperta è rimasta
/// invisibile a chi leggeva il valore intero.
pub fn state_word(text: &str) -> Option<String> {
    let front = frontmatter(text)?;
    front
        .lines()
        .find_map(|l| l.trim().strip_prefix("stato:"))
        .and_then(|v| v.split_whitespace().next())
        .map(str::to_string)
}

/// Una voce chiusa è storia: non guida più lavoro, e non si marca.
pub fn is_closed(state: &str) -> bool {
    state == "chiusa"
}

pub fn read_voice(name: &str, text: &str) -> Voice {
    Voice {
        name: name.to_string(),
        state: state_word(text).unwrap_or_default(),
        subjects: subjects(text),
        last_touched: last_declared_date(text),
    }
}

/// La data più recente fra quelle che il frontmatter dichiara.
///
/// Non si usa la data del file: le voci si riscrivono, quindi l'ultima
/// modifica del disco non dice quando qualcuno ha **misurato**. Una misura
/// fatta così il 21/08/2026 dichiarò «17 su 17 mai riprese», comprese le nove
/// che erano chiuse.
pub fn last_declared_date(text: &str) -> Option<Date> {
    let front = frontmatter(text)?;
    front
        .lines()
        .filter_map(|line| line.split_once(':').map(|(_, value)| value))
        .filter_map(crate::queue_clock::declared_date)
        .max()
}

/// Da quanti giorni questa voce non viene rimisurata, quando è troppi.
///
/// Torna `None` per una voce chiusa, per una senza date leggibili — «non lo so»
/// non è «è vecchia» — e per una ancora dentro la soglia.
pub fn stale_days(voice: &Voice, today: Date) -> Option<i64> {
    if is_closed(&voice.state) {
        return None;
    }
    let last = voice.last_touched?;
    let days = today.days() - last.days();
    (days > STALE_DAYS).then_some(days)
}

// ─── Il confronto ────────────────────────────────────────────────────────────

/// Due voci che nominano gli stessi soggetti rari.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub a: String,
    pub b: String,
    /// I soggetti in comune, in ordine, così il rilievo è ricopiabile.
    pub shared: Vec<String>,
}

/// Quante voci nominano ciascun soggetto.
fn document_frequency(voices: &[Voice]) -> BTreeMap<&str, usize> {
    let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
    for v in voices {
        for s in &v.subjects {
            *counted.entry(s.as_str()).or_insert(0) += 1;
        }
    }
    counted
}

/// Vero per un soggetto troppo diffuso per dire qualcosa.
fn is_common(frequency: usize, total: usize) -> bool {
    frequency > RARE_SUBJECT_FLOOR.max(total / COMMON_SUBJECT_DIVISOR)
}

/// Le coppie sospette del corpus.
///
/// Una coppia di voci **entrambe chiuse** non esce: è storia contro storia, e
/// non guida il lavoro di nessuno. Una chiusa contro una aperta esce, perché è
/// il caso in cui la voce viva ripete un errore già smontato.
pub fn suspect_pairs(voices: &[Voice]) -> Vec<Pair> {
    let frequency = document_frequency(voices);
    let total = voices.len();
    let mut out = Vec::new();
    for (i, a) in voices.iter().enumerate() {
        for b in voices.iter().skip(i + 1) {
            if is_closed(&a.state) && is_closed(&b.state) {
                continue;
            }
            let shared: Vec<String> = a
                .subjects
                .intersection(&b.subjects)
                .filter(|s| !is_common(*frequency.get(s.as_str()).unwrap_or(&0), total))
                .cloned()
                .collect();
            if shared.len() >= MIN_SHARED_SUBJECTS {
                out.push(Pair {
                    a: a.name.clone(),
                    b: b.name.clone(),
                    shared,
                });
            }
        }
    }
    out
}

/// Le compagne di una voce, con i soggetti che le legano.
pub fn partners_of<'a>(name: &str, pairs: &'a [Pair]) -> Vec<(&'a str, &'a [String])> {
    pairs
        .iter()
        .filter_map(|p| {
            if p.a == name {
                Some((p.b.as_str(), p.shared.as_slice()))
            } else if p.b == name {
                Some((p.a.as_str(), p.shared.as_slice()))
            } else {
                None
            }
        })
        .collect()
}

// ─── Il blocco dentro la voce ────────────────────────────────────────────────

/// Il testo senza il blocco rigenerabile, delimitatori compresi.
///
/// Senza memoria di proposito: chi scrive il blocco toglie prima quello di
/// ieri, così una passata che non ha più niente da dire lascia il file pulito
/// invece di lasciarci un avviso scaduto.
pub fn strip_block(text: &str) -> String {
    let Some(start) = text.find(BLOCK_OPEN) else {
        return text.to_string();
    };
    let Some(end_offset) = text[start..].find(BLOCK_CLOSE) else {
        return text.to_string();
    };
    let end = start + end_offset + BLOCK_CLOSE.len();
    // Gli a capo attorno al blocco sono suoi: se ne restassero, ogni passata
    // che toglie e rimette il rilievo farebbe crescere il file di una riga
    // vuota — e due passate di seguito non darebbero più gli stessi byte.
    let head = text[..start].trim_end_matches('\n');
    let tail = text[end..].trim_start_matches('\n');
    if head.is_empty() {
        return tail.to_string();
    }
    format!("{head}\n\n{tail}")
}

/// Il corpo del rilievo, o niente quando non c'è niente da dire.
///
/// Le righe sono citate (`>`) di proposito: chi apre il file le vede staccate
/// dal testo che un umano ha scritto, e nessuno le scambia per l'affermazione
/// della voce.
pub fn block_body(
    stale: Option<(i64, Option<Date>)>,
    partners: &[(&str, &[String])],
    drifted: &[String],
) -> Option<String> {
    if stale.is_none() && partners.is_empty() && drifted.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if let Some((days, last)) = stale {
        let when = match last {
            Some(d) => format!(", ultima data dichiarata {}", d.iso()),
            None => String::new(),
        };
        lines.push(format!(
            "> **DA RIVERIFICARE PRIMA DI AGIRCI** — aperta da {days} giorni{when}. \
             Quello che c'è scritto qui sotto era vero allora: rimisuralo, non eseguirlo."
        ));
    }
    for (name, shared) in partners {
        let quoted: Vec<String> = shared.iter().map(|s| format!("`{s}`")).collect();
        lines.push(format!(
            "> **Un'altra voce parla delle stesse cose**: `{name}` — in comune: {}. \
             Se le due si contraddicono, vale quella misurata dopo.",
            quoted.join(", ")
        ));
    }
    for anchor in drifted {
        lines.push(format!(
            "> **Il codice sotto è cambiato**: l'ancoraggio `{anchor}` non descrive \
             più il file di adesso."
        ));
    }
    Some(lines.join("\n>\n"))
}

/// La voce col blocco rigenerato subito sotto il frontmatter.
///
/// Sotto e non sopra: il frontmatter lo legge un programma — il selettore della
/// coda guarda `stato:` e `per:` — e infilargli davanti del testo lo
/// spegnerebbe. Chi legge con gli occhi trova comunque il rilievo prima del
/// racconto.
pub fn with_block(text: &str, body: Option<&str>) -> String {
    let bare = strip_block(text);
    let Some(body) = body else {
        return bare;
    };
    let block = format!("{BLOCK_OPEN}\n{body}\n{BLOCK_CLOSE}");
    let Some(front) = frontmatter(&bare) else {
        return format!("{block}\n\n{bare}");
    };
    // `4` sono i byte di `---\n`; ciò che resta comincia col `\n---` di
    // chiusura, che va tenuto insieme al suo a capo.
    let after_front = 4 + front.len();
    let rest = &bare[after_front..];
    let closing = rest
        .strip_prefix('\n')
        .and_then(|r| r.find('\n').map(|i| 1 + i + 1))
        .unwrap_or(rest.len());
    format!(
        "{}{}\n{block}\n\n{}",
        &bare[..after_front],
        &rest[..closing],
        rest[closing..].trim_start_matches('\n')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(name: &str, text: &str) -> Voice {
        read_voice(name, text)
    }

    fn doc(front: &str, body: &str) -> String {
        format!("---\n{front}\n---\n\n{body}\n")
    }

    #[test]
    fn a_command_head_is_a_subject_and_so_is_its_subcommand() {
        // Mutazione che lo rende rosso: togliere da `push_subjects`
        // l'inserimento della testa, o quello della coppia testa+sottocomando.
        let found = subjects("prova con `ulimit -v 4000` e `cargo mutants --list`");
        assert!(found.contains("ulimit"));
        assert!(found.contains("cargo mutants"));
        assert!(found.contains("cargo"));
    }

    #[test]
    fn a_path_becomes_its_basename_wherever_it_is_written() {
        // Il caso vero del 24/08: la stessa copia usa-e-getta nominata
        // `<scratchpad>/rss-watchdog.sh` in una voce e
        // `rust/tools/rss-watchdog.sh` nell'altra. Senza il nome nudo le due
        // voci non si incontrano.
        let a = subjects("sta in `<scratchpad>/rss-watchdog.sh` della sessione");
        let b = subjects("va portato in `rust/tools/rss-watchdog.sh`");
        assert!(a.contains("rss-watchdog.sh"));
        assert_eq!(a.intersection(&b).count(), 1);
    }

    #[test]
    fn a_file_inside_a_longer_command_line_still_counts() {
        // Mutazione che lo rende rosso: togliere il ciclo finale di
        // `push_subjects`, quello che raccoglie i file parola per parola.
        let found =
            subjects("prova `WAKEUP_DATE_OVERRIDE=x ~/.claude/scripts/nine-am-wakeup.sh --dry-run`");
        assert!(found.contains("nine-am-wakeup.sh"));
    }

    #[test]
    fn a_quoted_piece_of_format_is_not_a_command() {
        // `stato: aperta` fra apici inversi è il formato citato, non un gesto:
        // iscriverlo darebbe lo stesso soggetto a ogni voce che spiega il
        // formato. Mutazione che lo rende rosso: far tornare sempre `true` da
        // `is_command_word`.
        let found = subjects("si scrive `stato: aperta` in testa");
        assert!(found.is_empty());
    }

    #[test]
    fn a_session_id_is_an_author_not_a_subject() {
        // Due voci che nominano la stessa sessione hanno lo stesso autore, non
        // lo stesso argomento: sulle voci vere era il primo produttore di
        // coppie finte.
        // Mutazione che lo rende rosso: far tornare sempre `false` da
        // `is_identity`.
        let found = subjects("la sessione `ffc0d846` e la revisione `7ad541b` su `alfa.rs`");
        assert_eq!(found.len(), 1);
        assert!(found.contains("alfa.rs"));
        // Una parola fatta di sole lettere esadecimali, senza cifre, resta un
        // soggetto: è la ragione per cui la cifra è obbligatoria.
        assert!(subjects("il campo `added` del record").contains("added"));
    }

    #[test]
    fn a_position_does_not_make_a_second_file() {
        let found = subjects("il difetto è in `duplication.rs:332`, non in `duplication.rs`");
        assert_eq!(found.len(), 1);
        assert!(found.contains("duplication.rs"));
    }

    #[test]
    fn a_fenced_block_is_evidence_and_names_nobody() {
        // Mutazione che lo rende rosso: far tornare a `strip_fences` il testo
        // intero.
        let text = "parla di `ulimit`\n\n```\n$ cargo mutants --list\nerror: interrupted\n```\n";
        let found = subjects(text);
        assert!(found.contains("ulimit"));
        assert!(!found.contains("cargo mutants"));
    }

    #[test]
    fn two_voices_that_share_two_rare_subjects_are_a_pair() {
        // La coppia vera del 24/08, in miniatura: una prescrive `ulimit`,
        // l'altra misura che non si imposta.
        let voices = vec![
            voice(
                "jetsam.md",
                &doc(
                    "stato: aperta",
                    "tetto con `ulimit -v` in un involucro; `setrlimit`",
                ),
            ),
            voice(
                "segnale.md",
                &doc(
                    "stato: aperta",
                    "`ulimit -v` non si imposta: `setrlimit` risponde EINVAL",
                ),
            ),
            voice("altra.md", &doc("stato: aperta", "parla di `orca-cleanup.rs`")),
        ];
        let pairs = suspect_pairs(&voices);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].a, "jetsam.md");
        assert_eq!(pairs[0].b, "segnale.md");
        assert!(pairs[0].shared.contains(&"ulimit".to_string()));
        assert!(pairs[0].shared.contains(&"setrlimit".to_string()));
    }

    #[test]
    fn a_voice_nobody_else_touches_stays_alone() {
        // Il caso negativo: senza, il criterio prenderebbe tutto e nessuno lo
        // guarderebbe più.
        let voices = vec![
            voice("a.md", &doc("stato: aperta", "`ulimit` e `setrlimit`")),
            voice("b.md", &doc("stato: aperta", "`ulimit` e `setrlimit`")),
            voice(
                "sola.md",
                &doc("stato: aperta", "`spotlight-marker.rs` e `mdfind`"),
            ),
        ];
        let pairs = suspect_pairs(&voices);
        assert!(partners_of("sola.md", &pairs).is_empty());
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn one_shared_subject_is_not_enough() {
        // Mutazione che lo rende rosso: portare `MIN_SHARED_SUBJECTS` a 1.
        let voices = vec![
            voice("a.md", &doc("stato: aperta", "`ulimit` e `alfa.rs`")),
            voice("b.md", &doc("stato: aperta", "`ulimit` e `beta.rs`")),
        ];
        assert!(suspect_pairs(&voices).is_empty());
    }

    #[test]
    fn a_subject_named_by_everyone_carries_no_information() {
        // Nove voci che nominano tutte `settings.json` e `git status`: con la
        // soglia della frequenza spenta diventerebbero trentasei coppie.
        // Mutazione che lo rende rosso: far tornare sempre `false` da
        // `is_common`.
        let voices: Vec<Voice> = (0..9)
            .map(|i| {
                voice(
                    &format!("v{i}.md"),
                    &doc("stato: aperta", "tocca `settings.json` con `git status`"),
                )
            })
            .collect();
        assert!(suspect_pairs(&voices).is_empty());
    }

    #[test]
    fn two_closed_voices_do_not_make_a_pair() {
        let closed = vec![
            voice("a.md", &doc("stato: chiusa", "`ulimit` e `setrlimit`")),
            voice("b.md", &doc("stato: chiusa", "`ulimit` e `setrlimit`")),
        ];
        assert!(suspect_pairs(&closed).is_empty());
        // Ma una chiusa contro una aperta esce: è il caso in cui la viva ripete
        // un errore già smontato.
        let mixed = vec![
            voice("a.md", &doc("stato: chiusa", "`ulimit` e `setrlimit`")),
            voice("b.md", &doc("stato: aperta", "`ulimit` e `setrlimit`")),
        ];
        assert_eq!(suspect_pairs(&mixed).len(), 1);
    }

    #[test]
    fn the_state_is_the_first_word_and_the_rest_is_a_comment() {
        let text = doc("stato: aperta — RIAPERTA alle 09:50", "corpo");
        assert_eq!(state_word(&text).as_deref(), Some("aperta"));
        assert!(!is_closed(&state_word(&text).unwrap()));
        assert_eq!(state_word("niente frontmatter"), None);
    }

    #[test]
    fn the_age_comes_from_the_most_recent_declared_date() {
        // Mutazione che lo rende rosso: sostituire `.max()` con `.next()` in
        // `last_declared_date`, cioè leggere la prima data invece dell'ultima.
        let text = doc(
            "quando: 2026-08-21 09:00\nstato: aperta\npresa: 2026-08-23 11:00",
            "corpo",
        );
        let v = voice("x.md", &text);
        assert_eq!(v.last_touched, Date::new(2026, 8, 23));
        let today = Date::new(2026, 8, 24).unwrap();
        assert_eq!(stale_days(&v, today), None);
        let later = Date::new(2026, 8, 27).unwrap();
        assert_eq!(stale_days(&v, later), Some(4));
    }

    #[test]
    fn a_closed_voice_never_goes_stale_and_neither_does_a_dateless_one() {
        let today = Date::new(2026, 8, 24).unwrap();
        let closed = voice("c.md", &doc("quando: 2026-08-01\nstato: chiusa", "corpo"));
        assert_eq!(stale_days(&closed, today), None);
        // «Non lo so» non è «è vecchia».
        let dateless = voice("d.md", &doc("quando: boh\nstato: aperta", "corpo"));
        assert_eq!(stale_days(&dateless, today), None);
    }

    #[test]
    fn nothing_to_say_means_no_block() {
        assert!(block_body(None, &[], &[]).is_none());
    }

    #[test]
    fn the_block_goes_under_the_frontmatter_and_regenerates_in_place() {
        let text = doc("stato: aperta\nper: un builder", "# titolo\n\ncorpo");
        let body = block_body(Some((3, Date::new(2026, 8, 21))), &[], &[]).unwrap();
        let once = with_block(&text, Some(&body));
        // Il frontmatter resta intatto e primo: lo legge un programma.
        assert!(once.starts_with("---\nstato: aperta\nper: un builder\n---\n"));
        assert!(once.contains("DA RIVERIFICARE"));
        assert!(once.contains("# titolo"));
        // Idempotente: due passate danno lo stesso file.
        let twice = with_block(&once, Some(&body));
        assert_eq!(once, twice);
        // E si toglie senza lasciare traccia.
        assert_eq!(with_block(&once, None), text);
    }

    #[test]
    fn the_block_does_not_feed_itself() {
        // IL DIFETTO CHE QUESTO CASO ESISTE PER PRENDERE: il blocco nomina la
        // voce gemella e i soggetti in comune. Se la passata dopo li leggesse
        // come soggetti della voce, l'insieme crescerebbe a ogni giro finché
        // tutto tocca tutto.
        // Mutazione che lo rende rosso: togliere `strip_block` da `subjects`.
        let text = doc("stato: aperta", "parla solo di `alfa.rs`");
        let before = subjects(&text);
        let body = block_body(
            None,
            &[(
                "gemella.md",
                ["ulimit".to_string(), "setrlimit".to_string()].as_slice(),
            )],
            &[],
        )
        .unwrap();
        let marked = with_block(&text, Some(&body));
        assert!(marked.contains("gemella.md"));
        assert_eq!(subjects(&marked), before);
    }

    #[test]
    fn a_drifted_anchor_downgrades_the_voice() {
        let body = block_body(None, &[], &["~/x/duplication.rs#sha1".to_string()]).unwrap();
        assert!(body.contains("Il codice sotto è cambiato"));
        assert!(body.contains("duplication.rs#sha1"));
    }
}
