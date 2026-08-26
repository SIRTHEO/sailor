//! Un istinto vive solo se porta chi lo regge: `measured`, `expires`, `measure`.
//! Senza scadenza un istinto scritto una volta varrebbe per sempre — qui decide
//! `select()`, pura: riceve i testi e la data di oggi, sceglie chi si inietta
//! intero, chi è scaduto e chi non ha mai avuto una misura.
//!
//! Il parser del frontmatter è uno scanner riga per riga apposta, non un parser
//! YAML: i cinque campi che contano sono righe piatte fra due `---`, e un
//! valore con `:` dentro (un comando, un orario) resta intero perché si separa
//! solo sul primo.
//!
//! I due tetti valgono sul blocco intero, coda «Da rimisurare» compresa, e si
//! applicano scorrendo l'elenco per `confidence` decrescente. Un corpo che non
//! entrerebbe **nemmeno in un blocco vuoto** si salta e si prova il successivo,
//! così un istinto enorme non porta fuori anche i piccoli che avevano ancora
//! spazio; se invece a fermarlo è solo lo spazio già preso da chi lo precede in
//! classifica, l'elenco si chiude lì — altrimenti passerebbe avanti il primo
//! corpo abbastanza corto, e la dimensione varrebbe più del merito.
//!
//! Chi resta fuori lascia comunque la sua riga `trigger:`, in fondo al blocco e
//! **fuori** dal tetto: un istinto sparito del tutto non avverte nessuno, e
//! l'innesco costa un quindicesimo del corpo.

/// L'intestazione fissa del blocco iniettato, invariata dallo script che sostituisce.
const HEADER: &str = "# Learned instincts (from your own past session observations)\n\nHeuristics with a confidence score, not hard rules. Apply when the trigger matches.\n\n";

/// Quanti istinti vivi entrano al massimo nel blocco iniettato, oltre il
/// filtro di scadenza: il blocco cresceva senza freno (+135% dal 20/08),
/// quindi si taglia ai primi N per `confidence` anche se sono tutti vivi.
const MAX_LIVE_COUNT: usize = 8;

/// Tetto in byte del blocco iniettato, separatori e coda «Da rimisurare»
/// compresi. Non è un numero scelto a mano: il prologo ha un budget sotto i
/// 5.000 token e la quota degli istinti è il 20,5%, cioè ~1.025 token ≈ 2.400
/// byte a 2,31 byte/token; 3.500 è il primo scaglione realizzabile che non
/// riduce il blocco a un solo istinto sul corpus di stanotte.
/// Provvisorio finché il tetto definitivo del prologo non è deciso (24/08).
const MAX_LIVE_BYTES: usize = 3_500;

/// Quanto di `MAX_LIVE_BYTES` può prendersi la coda «Da rimisurare». La coda
/// stava fuori da ogni tetto e cresceva per sempre — 40 istinti scaduti fanno
/// 10.053 byte, il triplo del blocco intero — mentre è la parte meno urgente
/// da leggere: qui ha una quota piccola, e ciò che avanza torna ai corpi vivi.
const MAX_REMEASURE_BYTES: usize = 800;

/// Il titolo della coda, contato dentro `MAX_REMEASURE_BYTES`.
const REMEASURE_TITLE: &str = "## Da rimisurare\n\n";

/// Quanto può prendersi la sezione degli inneschi. Sta **fuori** da
/// `MAX_LIVE_BYTES` e si paga apposta: il 24/08/2026 la trappola dello zsh ha
/// morso una sessione in diretta — un `find` con le esclusioni in una variabile
/// ha risposto «zero file» invece di 1.179 — proprio mentre il suo corpo era
/// fra i sei tagliati dal tetto. La quota esiste solo perché la sezione non
/// cresca senza freno come faceva la coda: i sei inneschi di oggi ne fanno
/// 1.350 misurati, e oltre la quota si degrada alla riga di conto.
const MAX_TRIGGER_BYTES: usize = 1_600;

/// Il titolo della sezione degli inneschi, contato dentro `MAX_TRIGGER_BYTES`.
/// Dice cosa sono queste righe, perché un innesco nudo si legge come un istinto
/// monco, e dove sta il corpo per chi vuole andarselo a prendere.
const TRIGGER_TITLE: &str = "## Trappole di cui non è entrato il corpo\n\nQueste trappole esistono: il corpo non è entrato nel tetto, l'innesco sì. Il corpo intero sta in `~/.claude/homunculus/instincts/personal/<id>.md`.\n\n";

/// Spazio tenuto da parte per la riga che dichiara le righe non elencate: il
/// conto non arriva a sei cifre, quindi ci sta sempre. Senza questa riserva la
/// coda troncata sfonderebbe la quota proprio nel caso che la quota esiste per
/// contenere.
const OVERFLOW_LINE_MAX: usize = 48;

/// Il campo `confidence` letto dal frontmatter. Il terzo caso esiste perché una
/// riga che c'è ma non è un numero (`0,95` con la virgola) cadeva a 0.0 in
/// silenzio: l'istinto finiva ultimo in classifica e la testa ne dava una causa
/// falsa. Non lo è nemmeno un valore non finito (`NaN`), che non si ordina.
#[derive(Default, PartialEq, Debug)]
enum Confidence {
    #[default]
    Absent,
    Value(f64),
    Unreadable(String),
}

/// I soli campi del frontmatter che contano per la decadenza.
///
/// Visibile a tutto il crate perché la decadenza si chiede in due posti: qui,
/// che decide chi entra nel prologo, e nella consegna a evento, che decide se
/// un corpo vale ancora la pena di essere mandato. Due letture separate dello
/// stesso frontmatter divergerebbero alla prima riga storta.
#[derive(Default)]
pub(crate) struct Frontmatter {
    id: Option<String>,
    /// La riga che dice «quando ti succede questo»: è l'unica parte che si
    /// inietta anche quando il corpo non entra nel tetto.
    trigger: Option<String>,
    confidence: Confidence,
    measured: Option<String>,
    pub(crate) expires: Option<String>,
    measure: Option<String>,
}

/// Una riga dichiara un campo solo se, prima dei due punti, c'è un solo token
/// alfanumerico che comincia con una lettera. Senza questo controllo una frase
/// del corpo con un `:` dentro (un orario, un comando) veniva letta come un
/// campo, e senza il `---` di chiusura il corpo intero restava dentro il
/// frontmatter.
fn is_frontmatter_key(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Legge i campi `chiave: valore` fra i due `---` iniziali. Nessun `---` in
/// testa → frontmatter assente, tutti i campi `None`, nessun panico. Il
/// parser si ferma alla prima riga che non è né vuota né `chiave: valore`:
/// senza una chiusura esplicita è così che il corpo smette di essere letto
/// come se fosse ancora frontmatter.
pub(crate) fn parse_frontmatter(text: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return fm;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            break; // il corpo è cominciato senza una chiusura: ci si ferma qui
        };
        let key = key.trim();
        if !is_frontmatter_key(key) {
            break;
        }
        let value = value.trim().trim_matches('"');
        match key {
            "id" => fm.id = Some(value.to_string()),
            "trigger" => fm.trigger = Some(value.to_string()),
            "confidence" => {
                fm.confidence = match value.parse::<f64>() {
                    Ok(v) if v.is_finite() => Confidence::Value(v),
                    _ => Confidence::Unreadable(value.to_string()),
                }
            }
            "measured" => fm.measured = Some(value.to_string()),
            "expires" => fm.expires = Some(value.to_string()),
            "measure" => fm.measure = Some(value.to_string()),
            _ => {}
        }
    }
    fm
}

/// `YYYY-MM-DD` esatto: quattro cifre, un trattino, due cifre, un trattino, due
/// cifre. Non controlla il calendario (un 13º mese passa): serve solo a
/// garantire che il confronto lessicografico fra date valga quello cronologico.
pub(crate) fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
}

/// Cosa dice il frontmatter sulla decadenza, una volta validate le date.
enum Dating<'a> {
    /// Entrambe le date ci sono e sono nella forma ISO.
    Both(&'a str, &'a str),
    /// C'è una data ma non nella forma `YYYY-MM-DD`: vale come assente, ma il
    /// messaggio nomina il valore storto invece di tacere sulla causa.
    NotIso(&'a str),
    /// Manca `measured` o `expires`.
    Missing,
}

fn dating(fm: &Frontmatter) -> Dating<'_> {
    match (fm.measured.as_deref(), fm.expires.as_deref()) {
        (Some(m), Some(_)) if !is_iso_date(m) => Dating::NotIso(m),
        (Some(_), Some(e)) if !is_iso_date(e) => Dating::NotIso(e),
        (Some(m), Some(e)) => Dating::Both(m, e),
        _ => Dating::Missing,
    }
}

/// Il testo pronto per `additionalContext`, coi conteggi che l'hanno prodotto.
pub struct Rendered {
    pub text: String,
    pub live: usize,
    pub expired: usize,
    pub undated: usize,
    /// Quanti istinti vivi sono stati esclusi da un tetto, non dalla scadenza:
    /// la somma dei due che seguono, 0 finché nessuno dei due limiti scatta.
    pub excluded_by_cap: usize,
    /// Esclusi perché il loro corpo non entrava nel budget rimasto.
    pub excluded_by_bytes: usize,
    /// Esclusi perché le prime `MAX_LIVE_COUNT` posizioni erano già occupate.
    pub excluded_by_count: usize,
}

/// Rende una sezione di righe dentro `quota`, titolo compreso: righe intere
/// finché entrano, poi una riga sola che dice quante ne restano fuori. Vuota
/// se non c'è niente da elencare, così chi la chiama sa che non occupa nulla.
/// La usano tutte e due le sezioni in fondo al blocco — «Da rimisurare» e le
/// trappole senza corpo — perché il troncamento è lo stesso e una seconda copia
/// divergerebbe alla prima correzione.
fn render_capped_list(title: &str, lines: &[String], quota: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from(title);
    let mut listed = 0usize;
    for line in lines {
        let separator = if listed == 0 { 0 } else { 1 }; // "\n"
        if out.len() + separator + line.len() + OVERFLOW_LINE_MAX > quota {
            break;
        }
        if separator == 1 {
            out.push('\n');
        }
        out.push_str(line);
        listed += 1;
    }
    if listed < lines.len() {
        let hidden = lines.len() - listed;
        // Il titolo finisce già con una riga vuota: la coda che segue subito il
        // titolo non ne vuole un'altra davanti.
        if listed > 0 {
            out.push('\n');
        }
        out.push_str(&format!("… e altre {hidden} non elencate"));
    }
    out
}

/// Un istinto vivo in attesa di sapere se il suo corpo entra nel tetto. Porta
/// con sé nome e innesco perché chi resta fuori va comunque nominato: la riga
/// d'innesco si costruisce dopo la selezione, non prima.
struct Live {
    confidence: f64,
    id: String,
    trigger: Option<String>,
    body: String,
}

/// Sceglie quali istinti iniettare interi e quali ridurre a una riga.
///
/// `today` e le date del frontmatter sono confrontate come stringhe ISO
/// (`YYYY-MM-DD`): l'ordine lessicografico coincide con quello cronologico.
/// Un testo `None` è un file trovato ma non leggibile: conta come «senza
/// misura» invece di sparire dai tre numeri, che così restano esaustivi.
pub fn select(instincts: &[(String, Option<String>)], today: &str) -> Rendered {
    let mut live: Vec<Live> = Vec::new();
    let mut remeasure: Vec<String> = Vec::new();
    let (mut live_count, mut expired_count, mut undated_count) = (0usize, 0usize, 0usize);

    for (name, text) in instincts {
        let Some(text) = text else {
            undated_count += 1;
            remeasure.push(format!("istinto senza misura: {name} — illeggibile"));
            continue;
        };
        let fm = parse_frontmatter(text);
        let id = fm.id.as_deref().unwrap_or("senza-id");
        let confidence = match &fm.confidence {
            Confidence::Value(c) => *c,
            Confidence::Absent => 0.0, // niente riga: ultimo in classifica, ma vivo
            Confidence::Unreadable(bad) => {
                // Un valore illeggibile non si ordina: si dichiara la causa vera
                // invece di iniettarlo in fondo come se valesse 0.0.
                undated_count += 1;
                remeasure.push(format!(
                    "istinto senza misura: {id} — confidence non leggibile: {bad}"
                ));
                continue;
            }
        };
        match dating(&fm) {
            Dating::Both(_measured, expires) if today <= expires => {
                live_count += 1;
                live.push(Live {
                    confidence,
                    id: id.to_string(),
                    trigger: fm.trigger.clone(),
                    body: text.clone(),
                });
            }
            Dating::Both(measured, expires) => {
                expired_count += 1;
                let measure = fm.measure.as_deref().unwrap_or("misura non dichiarata");
                remeasure.push(format!(
                    "istinto scaduto: {id} (misurato il {measured}, scaduto il {expires}) — da rimisurare: {measure}"
                ));
            }
            Dating::NotIso(bad) => {
                undated_count += 1;
                remeasure.push(format!("istinto senza misura: {id} — data non ISO: {bad}"));
            }
            Dating::Missing => {
                undated_count += 1;
                remeasure.push(format!(
                    "istinto senza misura: {id} — non si inietta finché non porta measured/expires/measure"
                ));
            }
        }
    }

    // Confidence decrescente; a parità l'ordine di arrivo (sort stabile), che è
    // quello alfabetico dei file. È un criterio dichiaratamente arbitrario:
    // sugli otto istinti di oggi la confidence separa quattro scaglioni
    // (0,95 ×3 · 0,9 ×2 · 0,85 ×2 · 0,8), quindi decide la testa della
    // classifica ma non le parità, e nel frontmatter non c'è un secondo segnale
    // di merito da usare al suo posto.
    live.sort_by(|a, b| {
        b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
    });

    // La coda si rende per prima perché il suo ingombro toglie budget ai corpi
    // vivi: il tetto vale sul blocco emesso, non sui soli istinti iniettati.
    // I due byte del distacco si scalano anche quando nessun corpo entra: due
    // byte di prudenza costano meno di un tetto che sfonda per un caso di bordo.
    let remeasure_text = render_capped_list(REMEASURE_TITLE, &remeasure, MAX_REMEASURE_BYTES);
    let joint = if remeasure_text.is_empty() { 0 } else { 2 }; // "\n\n"
    let live_budget = MAX_LIVE_BYTES.saturating_sub(remeasure_text.len() + joint);

    let mut included: Vec<&str> = Vec::new();
    let mut cut: Vec<&Live> = Vec::new();
    let mut bytes_so_far = 0usize;
    let (mut cut_by_count, mut cut_by_bytes) = (0usize, 0usize);
    // Vero da quando un corpo è stato fermato dallo spazio già occupato: da lì
    // in poi non entra più nessuno, nemmeno chi ci starebbe.
    let mut budget_taken = false;
    for item in &live {
        if included.len() >= MAX_LIVE_COUNT {
            cut_by_count += 1;
            cut.push(item);
            continue;
        }
        let separator = if included.is_empty() { 0 } else { 2 }; // "\n\n"
        let candidate_bytes = bytes_so_far + separator + item.body.len();
        if budget_taken || candidate_bytes > live_budget {
            cut_by_bytes += 1;
            cut.push(item);
            // Due esclusioni diverse. Un corpo più lungo del budget intero non
            // sarebbe entrato nemmeno da solo: saltarlo non toglie niente a
            // nessuno, e i piccoli dietro tengono lo spazio che avevano. Un
            // corpo che invece ci sarebbe stato in un blocco vuoto è fermo solo
            // per lo spazio preso da chi lo precede in classifica: qui l'elenco
            // si chiude, altrimenti passerebbe avanti il primo corpo abbastanza
            // corto e la dimensione varrebbe più del merito.
            if item.body.len() <= live_budget {
                budget_taken = true;
            }
            continue;
        }
        bytes_so_far = candidate_bytes;
        included.push(item.body.as_str());
    }
    let excluded_by_cap = cut_by_count + cut_by_bytes;

    // Chi è stato tagliato lascia il suo innesco: chi legge deve sapere che la
    // trappola esiste, anche quando il corpo non entra.
    let trigger_lines: Vec<String> = cut
        .iter()
        .map(|item| {
            let trigger = item.trigger.as_deref().unwrap_or("innesco non dichiarato");
            format!("- {} — {}", item.id, trigger)
        })
        .collect();
    let trigger_text = render_capped_list(TRIGGER_TITLE, &trigger_lines, MAX_TRIGGER_BYTES);

    let mut text = String::new();
    text.push_str(HEADER);
    text.push_str(&format!(
        "Vivi: {live_count} · scaduti: {expired_count} · senza misura: {undated_count}"
    ));
    if excluded_by_cap > 0 {
        // Una causa per gruppo, con quanti ne ha presi: la riga dice perché
        // quel numero manca, non una ragione sola per tutti.
        let mut causes: Vec<String> = Vec::new();
        if cut_by_bytes > 0 {
            causes.push(format!("{cut_by_bytes} oltre il tetto di {MAX_LIVE_BYTES} byte"));
        }
        if cut_by_count > 0 {
            causes.push(format!(
                "{cut_by_count} oltre le prime {MAX_LIVE_COUNT} per confidence"
            ));
        }
        text.push_str(&format!(
            " · tagliati dal tetto: {excluded_by_cap} ({})",
            causes.join(", ")
        ));
    }
    text.push_str("\n\n");
    let head_len = text.len();
    for (i, body) in included.iter().enumerate() {
        if i > 0 {
            text.push_str("\n\n");
        }
        text.push_str(body);
    }
    // Le due sezioni in fondo si attaccano allo stesso modo: una riga vuota solo
    // se davanti c'è già qualcosa. Il metro è quanto si è scritto oltre la testa,
    // non il numero dei vivi: una sezione che non segue niente non vuole stacco.
    for section in [&remeasure_text, &trigger_text] {
        if section.is_empty() {
            continue;
        }
        if text.len() > head_len {
            text.push_str("\n\n");
        }
        text.push_str(section);
    }

    Rendered {
        text,
        live: live_count,
        expired: expired_count,
        undated: undated_count,
        excluded_by_cap,
        excluded_by_bytes: cut_by_bytes,
        excluded_by_count: cut_by_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instinct(id: &str, measured: &str, expires: &str, confidence: f64, body: &str) -> String {
        format!(
            "---\nid: {id}\nconfidence: {confidence}\nmeasured: {measured}\nexpires: {expires}\nmeasure: comando di prova\n---\n{body}\n"
        )
    }

    /// Un istinto vivo lungo **esattamente** `size` byte: i tetti in byte si
    /// provano sul limite, non «più o meno lì intorno», e il caso che conta è
    /// il corpo che ci sta per un byte solo.
    fn live_of_size(id: &str, confidence: f64, size: usize) -> String {
        let empty = instinct(id, "2026-08-01", "2026-08-30", confidence, "");
        assert!(size >= empty.len(), "{size} byte non bastano per il frontmatter");
        let padding = "X".repeat(size - empty.len());
        let text = instinct(id, "2026-08-01", "2026-08-30", confidence, &padding);
        assert_eq!(text.len(), size);
        text
    }

    fn expired(id: &str, body: &str) -> String {
        instinct(id, "2026-01-01", "2026-02-01", 0.9, body)
    }

    /// Il blocco su cui vale davvero `MAX_LIVE_BYTES`: corpi iniettati e coda,
    /// senza l'intestazione fissa né la riga dei conteggi, che nel budget non
    /// passano, e senza la sezione degli inneschi, che sta fuori dal tetto per
    /// scelta. Senza questo ritaglio il tetto si prova con uno scarto a occhio,
    /// e uno scarto abbastanza largo fa passare qualunque formula.
    fn capped_bytes(rendered: &Rendered) -> usize {
        let after_header = rendered.text.strip_prefix(HEADER).expect("l'intestazione c'è");
        let start = after_header.find("\n\n").expect("la riga dei conteggi c'è") + 2;
        let end = match after_header.find(TRIGGER_TITLE) {
            // La riga vuota che stacca la sezione si scala con lei; se la
            // sezione apre il blocco non c'è nessuno stacco da togliere.
            Some(pos) if pos > start => pos - 2,
            Some(_) => start,
            None => after_header.len(),
        };
        end - start
    }

    /// La sezione degli inneschi, dal titolo in poi.
    fn triggers_of(rendered: &Rendered) -> &str {
        let start = rendered.text.find(TRIGGER_TITLE).expect("la sezione degli inneschi c'è");
        &rendered.text[start..]
    }

    /// Un istinto vivo che dichiara un innesco, per provare cosa resta quando il
    /// corpo non entra.
    fn live_with_trigger(id: &str, confidence: f64, trigger: &str, size: usize) -> String {
        let head = format!(
            "---\nid: {id}\ntrigger: \"{trigger}\"\nconfidence: {confidence}\n\
measured: 2026-08-01\nexpires: 2026-08-30\nmeasure: comando di prova\n---\n"
        );
        assert!(size > head.len(), "{size} byte non bastano per il frontmatter");
        let text = format!("{head}{}\n", "X".repeat(size - head.len() - 1));
        assert_eq!(text.len(), size);
        text
    }

    /// La coda emessa, dal titolo in poi: è su questa che vale
    /// `MAX_REMEASURE_BYTES`, non sul blocco intero.
    fn tail_of(rendered: &Rendered) -> &str {
        let start = rendered.text.find(REMEASURE_TITLE).expect("la coda c'è");
        &rendered.text[start..]
    }

    /// Una riga di coda lunga **esattamente** `len` byte e riconoscibile dal suo
    /// numero: la quota della coda si prova sul byte, e i casi che contano sono
    /// quello che ci sta per un byte e quello che sfonda per uno.
    fn tail_line(n: usize, len: usize) -> String {
        let head = format!("riga-{n}:");
        assert!(len >= head.len(), "{len} byte non bastano per l'intestazione della riga");
        format!("{head}{}", "X".repeat(len - head.len()))
    }

    #[test]
    fn a_live_instinct_is_injected_whole() {
        let text = instinct("vivo", "2026-08-01", "2026-08-30", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: cambiare `<=` in `<` esclude il giorno di scadenza
        assert!(out.text.contains("CORPO SEGRETO"));
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0));
        // rotto così → rosso: stampare la causa del taglio anche a taglio zero
        assert!(!out.text.contains("tagliati dal tetto"), "{}", out.text);
    }

    #[test]
    fn an_instinct_expired_by_one_day_gives_only_the_line() {
        let text = instinct("scaduto", "2026-08-01", "2026-08-10", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-11");
        // rotto così → rosso: togliere il ramo `expired_count += 1` inietta il corpo
        assert!(!out.text.contains("CORPO SEGRETO"));
        assert!(out.text.contains("istinto scaduto: scaduto (misurato il 2026-08-01, scaduto il 2026-08-10)"));
        // rotto così → rosso: senza il ramo `measure` la riga dice «misura non
        // dichiarata» al posto del comando che la rifà
        assert!(out.text.contains("da rimisurare: comando di prova"), "{}", out.text);
        assert_eq!((out.live, out.expired, out.undated), (0, 1, 0));
    }

    #[test]
    fn an_instinct_without_dates_gives_only_the_line() {
        let text = "---\nid: nodata\nconfidence: 0.5\n---\nCORPO SEGRETO\n".to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: far cadere questo caso nel ramo `live` inietta il corpo
        assert!(!out.text.contains("CORPO SEGRETO"));
        assert!(out.text.contains("istinto senza misura: nodata"));
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
    }

    #[test]
    fn live_instincts_sort_by_confidence_descending() {
        let low = instinct("basso", "2026-08-01", "2026-08-30", 0.5, "BASSO");
        let high = instinct("alto", "2026-08-01", "2026-08-30", 0.9, "ALTO");
        let out = select(
            &[("a.md".into(), Some(low)), ("b.md".into(), Some(high))],
            "2026-08-15",
        );
        // rotto così → rosso: invertire `b.0.partial_cmp(&a.0)` in `a.0.partial_cmp(&b.0)`
        let pos_high = out.text.find("ALTO").expect("ALTO presente");
        let pos_low = out.text.find("BASSO").expect("BASSO presente");
        assert!(pos_high < pos_low, "atteso ALTO prima di BASSO: {}", out.text);
    }

    #[test]
    fn a_missing_frontmatter_does_not_panic() {
        let text = "nessun frontmatter qui, solo corpo\n".to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: un `unwrap()` al posto di `unwrap_or("senza-id")` panica qui
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(out.text.contains("istinto senza misura: senza-id"));
    }

    /// Revisione: `expires: 2026-9-5` (mese non due cifre) con oggi `2026-10-15`
    /// confrontava come stringa e risultava vivo, perché `"2026-10-15" <=
    /// "2026-9-5"` è vero lessicograficamente («1» < «9»). Deve invece contare
    /// come senza misura.
    #[test]
    fn a_non_padded_expiry_is_not_read_as_a_valid_date() {
        let text = instinct("male-scritto", "2026-08-01", "2026-9-5", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-10-15");
        // rotto così → rosso: togliere `is_iso_date` fa tornare vivo questo caso
        assert!(!out.text.contains("CORPO SEGRETO"));
        assert!(out.text.contains("istinto senza misura: male-scritto — data non ISO: 2026-9-5"));
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
    }

    /// Revisione: senza `---` di chiusura il parser leggeva anche il corpo, e un
    /// secondo `expires:` scritto lì dentro (una data passata, di esempio)
    /// degradava un istinto vivo. Il parser deve fermarsi alla prima riga di
    /// corpo, non arrivare fino in fondo al file.
    #[test]
    fn an_unclosed_frontmatter_does_not_read_the_body_as_more_fields() {
        let text = "---\nid: senza-chiusura\nmeasured: 2026-08-01\nexpires: 2026-12-31\n\
# Titolo del corpo, niente `---` sopra\n\nEvidenza con expires: 2020-01-01 dentro una frase\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: continuare a leggere oltre la prima riga di corpo
        // farebbe rileggere `expires: 2020-01-01` e scadere l'istinto
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0));
    }

    /// Un istinto vivo il cui corpo comincia con una frase che porta un `:`
    /// dentro, e più sotto ha una riga rientrata che *sembra* un campo. Serve la
    /// coppia: la frase da sola non nomina nessuno dei cinque campi, ma se non
    /// chiude il frontmatter la riga sotto ci entra davvero e l'istinto scade.
    #[test]
    fn a_prose_line_with_a_colon_closes_the_frontmatter_instead_of_being_read() {
        let text = "---\nid: vivo\nconfidence: 0.9\nmeasured: 2026-08-01\nexpires: 2026-12-31\n\
measure: comando di prova\nEvidenza raccolta il 21/08: il gancio nega la scrittura.\n\n\
    expires: 2020-01-01\nCORPO SEGRETO\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: una `is_frontmatter_key` sempre vera, o che accetta
        // gli spazi, fa leggere `expires: 2020-01-01` come campo
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0), "{}", out.text);
        assert!(out.text.contains("CORPO SEGRETO"), "{}", out.text);
    }

    /// La stessa trappola con una riga di corpo che comincia con una data: prima
    /// dei due punti c'è un solo token, ma non comincia con una lettera.
    #[test]
    fn a_body_line_that_starts_with_a_digit_closes_the_frontmatter() {
        let text = "---\nid: vivo\nconfidence: 0.9\nmeasured: 2026-08-01\nexpires: 2026-12-31\n\
measure: comando di prova\n2026-08-21: la misura e' stata rifatta.\n\n\
    expires: 2020-01-01\nCORPO SEGRETO\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: senza il controllo sulla prima lettera «2026-08-21»
        // passa per un campo, il parser tira dritto e l'istinto risulta scaduto
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0), "{}", out.text);
        assert!(out.text.contains("CORPO SEGRETO"), "{}", out.text);
    }

    /// Il rovescio: un campo che non è fra i cinque ma è scritto come un campo
    /// non deve chiudere il frontmatter. I trattini negli istinti veri ci sono
    /// (`fonte-dati`), e chiudere lì dentro perderebbe le date scritte sotto.
    #[test]
    fn an_unknown_field_with_a_dash_does_not_close_the_frontmatter() {
        let text = "---\nid: vivo\nfonte-dati: registro dei ganci\nconfidence: 0.9\n\
measured: 2026-08-01\nexpires: 2026-12-31\nmeasure: comando di prova\n---\nCORPO SEGRETO\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: un controllo che rifiuta `-` o `_` si ferma su
        // `fonte-dati` e non arriva mai a `measured`/`expires`
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0), "{}", out.text);
        assert!(out.text.contains("CORPO SEGRETO"), "{}", out.text);
    }

    /// `confidence: NaN` si legge come numero ma non si ordina: vale come
    /// illeggibile, non come un valore fra gli altri.
    #[test]
    fn a_non_finite_confidence_counts_as_unreadable() {
        let text = "---\nid: nan\nconfidence: NaN\nmeasured: 2026-08-01\nexpires: 2026-08-30\n\
measure: comando di prova\n---\nCORPO SEGRETO\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: accettare qualunque `f64` mette NaN in classifica,
        // dove ogni confronto è falso e l'ordine diventa quello di arrivo
        assert!(!out.text.contains("CORPO SEGRETO"), "{}", out.text);
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(
            out.text.contains("istinto senza misura: nan — confidence non leggibile: NaN"),
            "{}",
            out.text
        );
    }

    /// Le quattro forme storte che il confronto lessicografico non regge: il
    /// controllo è lungo dieci, coi trattini al quarto e al settimo byte e cifre
    /// in tutti gli altri, e ognuna di queste ne viola uno solo.
    #[test]
    fn a_date_that_is_not_iso_never_passes_for_a_valid_one() {
        for bad in ["2026/08/01", "2026-08/01", "20260801", "2026-08-011"] {
            let text = instinct("storto", "2026-08-01", bad, 0.9, "CORPO SEGRETO");
            let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
            // rotto così → rosso: un `||` al posto di un `&&` fa bastare una
            // condizione sola, e la data storta torna a contare come valida
            assert_eq!((out.live, out.expired, out.undated), (0, 0, 1), "«{bad}»: {}", out.text);
            assert!(out.text.contains(&format!("data non ISO: {bad}")), "{}", out.text);
        }
    }

    /// La data storta può essere `measured`, non solo `expires`: è la prima delle
    /// due a essere controllata, e la causa dichiarata deve nominare lei.
    #[test]
    fn a_measured_that_is_not_iso_is_named_as_the_cause() {
        let text = instinct("storto", "2026-8-1", "2026-12-31", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: saltare il controllo su `measured` fa cadere il
        // caso nel ramo «entrambe valide», e l'istinto torna vivo
        assert!(!out.text.contains("CORPO SEGRETO"), "{}", out.text);
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(out.text.contains("data non ISO: 2026-8-1"), "{}", out.text);
    }

    /// Un istinto scaduto senza riga `measure` lo dice, invece di tacere sulla
    /// sola cosa che serve a chi deve rimisurarlo.
    #[test]
    fn an_expired_instinct_without_a_measure_says_so() {
        let text = "---\nid: muto\nconfidence: 0.9\nmeasured: 2026-01-01\nexpires: 2026-02-01\n\
---\nCORPO\n"
            .to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        assert!(out.text.contains("da rimisurare: misura non dichiarata"), "{}", out.text);
    }

    /// Revisione: un `.md` trovato ma illeggibile spariva dai tre conteggi
    /// invece di finire in «senza misura».
    #[test]
    fn an_unreadable_file_counts_as_undated_instead_of_vanishing() {
        let out = select(&[("rotto.md".to_string(), None)], "2026-08-15");
        // rotto così → rosso: ignorare il ramo `None` lo farebbe sparire dal totale
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(out.text.contains("istinto senza misura: rotto.md — illeggibile"));
    }

    /// Nono istinto vivo: il tetto di conteggio (`MAX_LIVE_COUNT`) taglia
    /// prima del tetto in byte, perché i corpi qui sono piccoli.
    #[test]
    fn a_ninth_live_instinct_is_cut_by_the_count_cap() {
        let items: Vec<(String, Option<String>)> = (0..9)
            .map(|i| {
                let confidence = 0.9 - (i as f64) * 0.01; // decrescente per ordine noto
                let body = format!("CORPO-{i}");
                (
                    format!("f{i}.md"),
                    Some(instinct(&format!("i{i}"), "2026-08-01", "2026-08-30", confidence, &body)),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: togliere il controllo `included.len() >= MAX_LIVE_COUNT` inietta il nono
        assert!(out.text.contains("CORPO-0"), "il primo per confidence deve entrare");
        assert!(!out.text.contains("CORPO-8"), "il nono deve restare fuori");
        assert_eq!((out.excluded_by_cap, out.excluded_by_count, out.excluded_by_bytes), (1, 1, 0));
        // rotto così → rosso: stampare `included.len()` invece di `live_count`
        // farebbe dire «Vivi: 8» a fronte di nove istinti vivi
        assert!(out.text.contains("Vivi: 9 ·"), "{}", out.text);
        // rotto così → rosso: scrivere la soglia a mano nella stringa la fa
        // divergere da `MAX_LIVE_COUNT` alla prima modifica della costante
        assert!(out.text.contains(&format!(
            "tagliati dal tetto: 1 (1 oltre le prime {MAX_LIVE_COUNT} per confidence)"
        )), "{}", out.text);
    }

    /// Il caso vero del tetto in byte: nessun corpo sfonda da solo, ma insieme
    /// sì. Senza l'accumulatore ogni corpo passerebbe il controllo e tutti e
    /// tre entrerebbero.
    #[test]
    fn small_bodies_that_together_break_the_byte_cap_are_cut() {
        let items: Vec<(String, Option<String>)> = ["primo", "secondo", "terzo"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    format!("f{i}.md"),
                    Some(live_of_size(id, 0.9 - (i as f64) * 0.01, 1_400)),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: `separator + body.len()` senza `bytes_so_far`, o
        // l'azzeramento dell'accumulatore, fanno entrare anche il terzo
        assert!(out.text.contains("id: primo"), "{}", out.text);
        assert!(out.text.contains("id: secondo"));
        assert!(!out.text.contains("id: terzo"), "1.400 × 3 sfonda i 3.500 byte");
        assert_eq!((out.excluded_by_cap, out.excluded_by_bytes), (1, 1));
        assert!(out.text.contains(&format!(
            "tagliati dal tetto: 1 (1 oltre il tetto di {MAX_LIVE_BYTES} byte)"
        )), "{}", out.text);
    }

    /// I separatori fra un corpo e l'altro sono dentro il tetto: cinque corpi
    /// da 700 byte fanno esattamente 3.500 byte di solo testo, e sono gli otto
    /// byte dei quattro separatori a farne restare fuori uno.
    #[test]
    fn the_separators_count_against_the_byte_cap() {
        let items: Vec<(String, Option<String>)> = (0..5)
            .map(|i| {
                (
                    format!("f{i}.md"),
                    Some(live_of_size(&format!("n{i}"), 0.9 - (i as f64) * 0.01, 700)),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: `separator` fisso a 0 fa entrare anche il quinto
        assert!(out.text.contains("id: n3"), "i primi quattro devono entrare");
        assert!(!out.text.contains("id: n4"), "il quinto non ci sta per otto byte");
        assert_eq!(out.excluded_by_bytes, 1);
    }

    /// Un corpo lungo quanto il tetto ci sta: il confronto è `>`, non `>=`.
    #[test]
    fn a_body_exactly_at_the_byte_cap_still_fits() {
        let text = live_of_size("esatto", 0.9, MAX_LIVE_BYTES);
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: `>=` al posto di `>` lascia fuori il caso limite
        assert!(out.text.contains("id: esatto"), "{}", out.text);
        assert_eq!(out.excluded_by_cap, 0);
    }

    /// Il cuore della revisione: chi non ci sta si salta, non chiude l'elenco.
    /// Un corpo enorme in mezzo alla classifica non deve portare fuori i
    /// piccoli che vengono dopo e che avrebbero ancora spazio.
    #[test]
    fn a_body_over_the_cap_is_skipped_and_the_next_ones_still_enter() {
        let first = live_of_size("primo", 0.99, 200);
        let huge = live_of_size("grosso", 0.90, MAX_LIVE_BYTES + 1);
        let last = live_of_size("terzo", 0.80, 200);
        let out = select(
            &[
                ("a.md".into(), Some(first)),
                ("b.md".into(), Some(huge)),
                ("c.md".into(), Some(last)),
            ],
            "2026-08-15",
        );
        // rotto così → rosso: un `break` al posto del `continue` butta fuori
        // anche il terzo, che nel budget rimasto ci stava
        assert!(out.text.contains("id: primo"));
        assert!(!out.text.contains("id: grosso"), "il corpo enorme non entra");
        assert!(out.text.contains("id: terzo"), "il piccolo dietro deve entrare lo stesso");
        assert_eq!((out.excluded_by_cap, out.excluded_by_bytes), (1, 1));
    }

    /// Due cause diverse nello stesso blocco: una per byte, una per conteggio.
    /// La testa deve dire quanti per ciascuna, non una causa sola per tutti.
    #[test]
    fn the_head_line_gives_the_true_cause_for_each_group() {
        let mut items: Vec<(String, Option<String>)> =
            vec![("a.md".into(), Some(live_of_size("grosso", 0.99, MAX_LIVE_BYTES + 100)))];
        for i in 0..9 {
            items.push((
                format!("f{i}.md"),
                Some(live_of_size(&format!("n{i}"), 0.9 - (i as f64) * 0.01, 200)),
            ));
        }
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: una causa sola per tutto il gruppo attribuisce
        // al nono piccolo il tetto in byte, che non è la sua causa
        assert_eq!(
            (out.excluded_by_cap, out.excluded_by_bytes, out.excluded_by_count),
            (2, 1, 1)
        );
        assert!(out.text.contains(&format!(
            "tagliati dal tetto: 2 (1 oltre il tetto di {MAX_LIVE_BYTES} byte, 1 oltre le prime {MAX_LIVE_COUNT} per confidence)"
        )), "{}", out.text);
        assert!(!out.text.contains("id: grosso"));
        assert!(out.text.contains("id: n7"), "gli otto piccoli entrano");
        assert!(!out.text.contains("id: n8"), "il nono piccolo è di troppo");
    }

    /// La coda «Da rimisurare» sta dentro il tetto come tutto il resto: 40
    /// istinti scaduti facevano 10.053 byte fuori da ogni limite.
    #[test]
    fn the_remeasure_tail_cannot_grow_past_the_cap() {
        let items: Vec<(String, Option<String>)> = (0..40)
            .map(|i| {
                (
                    format!("f{i}.md"),
                    Some(expired(&format!("vecchio-{i}"), "CORPO")),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: rendere la coda intera la fa arrivare a 10.053 byte.
        // Il metro è la quota della coda, non il tetto del blocco: con quello
        // c'erano quattro volte lo spazio necessario e ogni formula passava.
        let tail = tail_of(&out);
        assert!(
            tail.len() <= MAX_REMEASURE_BYTES,
            "coda di {} byte oltre la quota di {MAX_REMEASURE_BYTES}: {tail}",
            tail.len()
        );
        assert!(capped_bytes(&out) <= MAX_LIVE_BYTES, "{}", out.text);
        assert_eq!(out.expired, 40);
        assert!(out.text.contains("## Da rimisurare"));
        assert!(out.text.contains("non elencate"), "le righe tagliate si dichiarano");
        assert!(out.text.contains("istinto scaduto: vecchio-0"), "le prime si leggono");
    }

    /// Una riga che riempie la quota **al byte** entra: il confronto è `>`, non
    /// `>=`. Titolo (18) + riga (734) + riserva (48) = 800 esatti.
    #[test]
    fn a_tail_line_that_exactly_fills_the_quota_still_enters() {
        let lines = vec![tail_line(0, 734), tail_line(1, 10)];
        let out = render_capped_list(REMEASURE_TITLE, &lines, MAX_REMEASURE_BYTES);
        // rotto così → rosso: `>=` lascia fuori proprio il caso limite
        assert!(out.contains("riga-0:"), "{out}");
        assert!(!out.contains("riga-1:"), "{out}");
        assert!(out.ends_with("\n… e altre 1 non elencate"), "{out}");
        assert!(out.len() <= MAX_REMEASURE_BYTES, "coda di {} byte", out.len());
    }

    /// Il byte del ritorno a capo fra due righe si paga sulla quota: qui la
    /// seconda riga sfonda **solo** per quello — 318 + 1 + 434 + 48 = 801.
    #[test]
    fn the_separator_byte_counts_against_the_tail_quota() {
        let lines = vec![tail_line(0, 300), tail_line(1, 434)];
        let out = render_capped_list(REMEASURE_TITLE, &lines, MAX_REMEASURE_BYTES);
        // rotto così → rosso: togliere lo stacco dal conto, o moltiplicarlo
        // invece di sommarlo, fa entrare una riga in più di un byte
        assert!(out.contains("riga-0:"), "{out}");
        assert!(!out.contains("riga-1:"), "{out}");
    }

    /// La riserva per la riga di conto serve a non sfondare proprio nel caso in
    /// cui la quota esiste: quattro righe restano fuori e il totale resta sotto.
    #[test]
    fn the_reserved_room_for_the_overflow_line_keeps_the_tail_in_the_quota() {
        let lines: Vec<String> = (0..5).map(|i| tail_line(i, 400)).collect();
        let out = render_capped_list(REMEASURE_TITLE, &lines, MAX_REMEASURE_BYTES);
        // rotto così → rosso: senza la riserva entra una seconda riga e la coda
        // arriva a 846 byte; un conto sbagliato dice «e altre 5» o «e altre 6»;
        // senza lo stacco la riga di conto si incolla all'ultima elencata
        assert!(out.contains("riga-0:"), "{out}");
        assert!(!out.contains("riga-1:"), "{out}");
        assert!(out.ends_with("\n… e altre 4 non elencate"), "{out}");
        assert!(out.len() <= MAX_REMEASURE_BYTES, "coda di {} byte", out.len());
    }

    /// Quando non entra nemmeno una riga, la riga di conto segue subito il
    /// titolo: il titolo finisce già con una riga vuota.
    #[test]
    fn nothing_fits_and_the_count_line_follows_the_title() {
        let out = render_capped_list(REMEASURE_TITLE, &[tail_line(0, 900)], MAX_REMEASURE_BYTES);
        // rotto così → rosso: uno stacco messo a prescindere aggiunge una riga
        // vuota di troppo davanti alla sola riga rimasta
        assert_eq!(out, format!("{REMEASURE_TITLE}… e altre 1 non elencate"));
    }

    /// Una coda che ci sta tutta non dichiara niente di nascosto, e ogni riga
    /// resta la sua: 18 + 200 + 1 + 200 + 1 + 200 = 620 byte.
    #[test]
    fn a_tail_that_fits_whole_says_nothing_about_lines_left_out() {
        let lines: Vec<String> = (0..3).map(|i| tail_line(i, 200)).collect();
        let out = render_capped_list(REMEASURE_TITLE, &lines, MAX_REMEASURE_BYTES);
        // rotto così → rosso: `<=` aggiunge «… e altre 0 non elencate», e un
        // contatore che non avanza incolla le righe una all'altra
        assert_eq!(out, format!("{REMEASURE_TITLE}{}\n{}\n{}", lines[0], lines[1], lines[2]));
    }

    /// La coda toglie budget ai corpi vivi, non se ne prende uno suo: il tetto
    /// vale sul blocco emesso, coda compresa.
    #[test]
    fn the_tail_takes_its_bytes_from_the_live_budget() {
        let big = live_of_size("quasi-pieno", 0.9, MAX_LIVE_BYTES - 100);
        let stale = expired("vecchio", "CORPO");
        let out = select(
            &[("a.md".into(), Some(big)), ("b.md".into(), Some(stale))],
            "2026-08-15",
        );
        // rotto così → rosso: un tetto che guarda solo i corpi vivi farebbe
        // entrare il corpo grosso *e* la coda, sfondando il blocco
        assert!(!out.text.contains("id: quasi-pieno"), "col peso della coda non ci sta più");
        assert!(out.text.contains("istinto scaduto: vecchio"));
        assert!(capped_bytes(&out) <= MAX_LIVE_BYTES, "{}", out.text);
    }

    /// I due byte di riga vuota fra i corpi e la coda escono dallo stesso budget:
    /// il tetto vale sul blocco emesso, distacco compreso. Si prova sul byte —
    /// un corpo grande esattamente quanto il budget entra, uno di un byte più
    /// grande no.
    #[test]
    fn the_blank_line_before_the_tail_is_charged_to_the_live_budget() {
        let stale = expired("vecchio", "CORPO");
        let alone = select(&[("b.md".into(), Some(stale.clone()))], "2026-08-15");
        let budget = MAX_LIVE_BYTES - (tail_of(&alone).len() + 2);

        let fits = select(
            &[
                ("a.md".into(), Some(live_of_size("giusto", 0.9, budget))),
                ("b.md".into(), Some(stale.clone())),
            ],
            "2026-08-15",
        );
        // rotto così → rosso: contare la coda due volte restringe il budget e
        // lascia fuori un corpo che ci stava
        assert!(fits.text.contains("id: giusto"), "{}", fits.text);
        assert_eq!(fits.excluded_by_bytes, 0);

        let over = select(
            &[
                ("a.md".into(), Some(live_of_size("troppo", 0.9, budget + 1))),
                ("b.md".into(), Some(stale)),
            ],
            "2026-08-15",
        );
        // rotto così → rosso: scalare il distacco invece di sommarlo allarga il
        // budget di quattro byte, e il blocco emesso sfonda il tetto
        assert!(!over.text.contains("id: troppo"), "{}", over.text);
        assert_eq!(over.excluded_by_bytes, 1);
        assert!(capped_bytes(&fits) <= MAX_LIVE_BYTES, "{}", fits.text);
    }

    /// Fra due corpi iniettati c'è una riga vuota, e davanti al primo no: la
    /// riga dei conteggi ha già il suo distacco.
    #[test]
    fn injected_bodies_are_separated_and_none_opens_the_block() {
        let high = instinct("alto", "2026-08-01", "2026-08-30", 0.9, "ALTO");
        let low = instinct("basso", "2026-08-01", "2026-08-30", 0.5, "BASSO");
        let out = select(
            &[("a.md".into(), Some(low.clone())), ("b.md".into(), Some(high.clone()))],
            "2026-08-15",
        );
        // rotto così → rosso: uno stacco messo a prescindere apre il blocco con
        // una riga vuota di troppo
        assert!(out.text.contains(&format!("senza misura: 0\n\n{high}")), "{}", out.text);
        // rotto così → rosso: uno stacco che non scatta mai, o che scatta solo
        // sul primo, incolla il secondo corpo alla fine del primo
        assert!(out.text.contains(&format!("{high}\n\n{low}")), "{}", out.text);
    }

    /// Senza niente iniettato la coda non vuole un distacco davanti: la riga
    /// vuota dipende da quello che è entrato, non dal numero dei vivi.
    #[test]
    fn no_blank_line_is_added_when_nothing_was_injected() {
        let huge = live_of_size("troppo-grosso", 0.9, MAX_LIVE_BYTES * 2);
        let stale = expired("vecchio", "CORPO");
        let out = select(
            &[("a.md".into(), Some(huge)), ("b.md".into(), Some(stale))],
            "2026-08-15",
        );
        // rotto così → rosso: `live_count > 0` al posto di `!included.is_empty()`
        // aggiunge un distacco davanti a una sezione che non segue niente
        assert!(out.text.contains("\n\n## Da rimisurare"), "{}", out.text);
        assert!(!out.text.contains("\n\n\n"), "riga vuota di troppo: {}", out.text);
        assert_eq!(out.live, 1);
    }

    /// `confidence: 0,95` (virgola invece di punto) cadeva a 0.0 in silenzio:
    /// l'istinto finiva ultimo e la testa diceva che l'aveva tagliato il tetto.
    #[test]
    fn an_unreadable_confidence_counts_as_undated_with_its_true_cause() {
        let text = "---\nid: virgola\nconfidence: 0,95\nmeasured: 2026-08-01\nexpires: 2026-08-30\nmeasure: comando di prova\n---\nCORPO SEGRETO\n".to_string();
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: `value.parse().ok()` con `unwrap_or(0.0)` lo
        // inietta come se avesse confidence zero, senza dirlo a nessuno
        assert!(!out.text.contains("CORPO SEGRETO"));
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(
            out.text.contains("istinto senza misura: virgola — confidence non leggibile: 0,95"),
            "{}",
            out.text
        );
    }

    /// Nessuna riga `confidence` non è un errore: l'istinto vive e va in fondo
    /// alla classifica. È il caso che distingue «assente» da «illeggibile».
    #[test]
    fn a_missing_confidence_stays_live_at_the_bottom() {
        let without = "---\nid: senza\nmeasured: 2026-08-01\nexpires: 2026-08-30\nmeasure: comando di prova\n---\nBODY-WITHOUT\n".to_string();
        let with = instinct("con", "2026-08-01", "2026-08-30", 0.1, "BODY-WITH");
        let out = select(
            &[("a.md".into(), Some(without)), ("b.md".into(), Some(with))],
            "2026-08-15",
        );
        assert_eq!((out.live, out.undated), (2, 0));
        let pos_with = out.text.find("BODY-WITH\n").expect("BODY-WITH presente");
        let pos_without = out.text.find("BODY-WITHOUT").expect("BODY-WITHOUT presente");
        assert!(pos_with < pos_without, "chi non dichiara confidence sta in fondo");
    }

    /// Il guasto del 24/08/2026: il tetto tagliava sei istinti su otto e chi
    /// leggeva non sapeva nemmeno che esistessero. Un corpo che non entra deve
    /// lasciare almeno il suo innesco, col nome per andarselo a leggere.
    #[test]
    fn an_instinct_cut_by_the_byte_cap_still_leaves_its_trigger() {
        let small = live_with_trigger("piccolo", 0.99, "quando succede il primo caso", 200);
        let huge = live_with_trigger(
            "zsh-variabile",
            0.90,
            "quando un elenco passa da una variabile",
            MAX_LIVE_BYTES + 1,
        );
        let out = select(
            &[("a.md".into(), Some(small)), ("b.md".into(), Some(huge))],
            "2026-08-15",
        );
        // rotto così → rosso: non raccogliere i tagliati, o non emettere la
        // sezione, riporta il blocco a com'era quando la trappola ha morso
        assert!(!out.text.contains("id: zsh-variabile"), "il corpo tagliato non si inietta");
        assert!(out.text.contains("id: piccolo"), "{}", out.text);
        assert!(
            out.text.contains("- zsh-variabile — quando un elenco passa da una variabile"),
            "{}",
            out.text
        );
        // rotto così → rosso: elencare anche chi è entrato intero ripete l'innesco
        assert!(!out.text.contains("- piccolo —"), "chi entra intero non si ripete: {}", out.text);
        assert!(out.text.contains("## Trappole di cui non è entrato il corpo"), "{}", out.text);
    }

    /// Anche chi è tagliato dal tetto di conteggio lascia l'innesco: la causa
    /// del taglio non cambia cosa deve sapere chi legge.
    #[test]
    fn an_instinct_cut_by_the_count_cap_also_leaves_its_trigger() {
        let items: Vec<(String, Option<String>)> = (0..9)
            .map(|i| {
                (
                    format!("f{i}.md"),
                    Some(live_with_trigger(
                        &format!("n{i}"),
                        0.9 - (i as f64) * 0.01,
                        &format!("quando capita il caso {i}"),
                        200,
                    )),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: raccogliere i tagliati solo nel ramo dei byte
        // lascia muto proprio il nono, che è quello che nessuno vedrà mai
        assert_eq!((out.excluded_by_count, out.excluded_by_bytes), (1, 0));
        assert!(out.text.contains("- n8 — quando capita il caso 8"), "{}", out.text);
    }

    /// Un tagliato senza riga `trigger:` si nomina lo stesso: sapere che esiste
    /// un istinto che non è entrato vale più del silenzio.
    #[test]
    fn a_cut_instinct_without_a_trigger_is_named_anyway() {
        let huge = live_of_size("muto", 0.9, MAX_LIVE_BYTES + 1);
        let out = select(&[("a.md".into(), Some(huge))], "2026-08-15");
        // rotto così → rosso: saltare chi non dichiara l'innesco lo fa sparire
        assert!(out.text.contains("- muto — innesco non dichiarato"), "{}", out.text);
    }

    /// Niente tagliati, niente sezione: un titolo senza righe sotto costerebbe
    /// byte a ogni sessione per non dire niente.
    #[test]
    fn no_trigger_section_when_nothing_was_cut() {
        let text = instinct("vivo", "2026-08-01", "2026-08-30", 0.9, "CORPO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: rendere la sezione a prescindere stampa il titolo
        assert!(!out.text.contains(TRIGGER_TITLE), "{}", out.text);
        assert!(!out.text.contains("\n\n\n"), "riga vuota di troppo: {}", out.text);
    }

    /// Gli inneschi si pagano **in aggiunta** al tetto: la parte sotto
    /// `MAX_LIVE_BYTES` resta dentro, e il blocco emesso è più lungo di così.
    /// È il costo dichiarato della riparazione, e va misurato, non stimato.
    #[test]
    fn the_trigger_lines_are_charged_outside_the_byte_cap() {
        // Otto corpi da 430 byte riempiono il tetto quasi al millimetro
        // (8 × 430 + 7 stacchi = 3.454 su 3.500): se la sezione contasse dentro
        // il budget non ci sarebbe posto, e la si vedrebbe sparire.
        let items: Vec<(String, Option<String>)> = (0..9)
            .map(|i| {
                (
                    format!("f{i}.md"),
                    Some(live_with_trigger(
                        &format!("n{i}"),
                        0.9 - (i as f64) * 0.01,
                        &format!("quando capita il caso {i}"),
                        430,
                    )),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        // rotto così → rosso: contare la sezione dentro `live_budget` la fa
        // rientrare nel tetto, e il costo sparisce dalla misura
        assert!(capped_bytes(&out) <= MAX_LIVE_BYTES, "{}", out.text);
        let triggers = triggers_of(&out);
        assert!(!triggers.is_empty());
        assert!(
            capped_bytes(&out) + triggers.len() > MAX_LIVE_BYTES,
            "gli inneschi devono aggiungersi al tetto, non starci dentro"
        );
        assert!(triggers.len() <= MAX_TRIGGER_BYTES, "sezione di {} byte", triggers.len());
    }

    /// Il criterio di scelta: la dimensione non è un vantaggio. Il secondo per
    /// confidence ci starebbe in un blocco vuoto, quindi a fermarlo è solo lo
    /// spazio preso dal primo — e da lì l'elenco si chiude. Senza questa regola
    /// il terzo, che vale meno ma è corto, gli passerebbe davanti.
    #[test]
    fn a_shorter_body_does_not_overtake_a_worthier_one_that_lost_only_the_room() {
        let first = live_with_trigger("primo", 0.99, "quando capita il primo caso", 2_000);
        let second = live_with_trigger("secondo", 0.90, "quando capita il secondo caso", 2_000);
        let third = live_with_trigger("terzo", 0.80, "quando capita il terzo caso", 300);
        let out = select(
            &[
                ("a.md".into(), Some(first)),
                ("b.md".into(), Some(second)),
                ("c.md".into(), Some(third)),
            ],
            "2026-08-15",
        );
        // rotto così → rosso: un `continue` che non chiude l'elenco fa entrare
        // il terzo (2.000 + 2 + 300 sta nei 3.500) davanti al secondo, che vale
        // di più ed è escluso solo perché è più lungo
        assert!(out.text.contains("id: primo"), "{}", out.text);
        assert!(!out.text.contains("id: secondo"), "{}", out.text);
        assert!(!out.text.contains("id: terzo"), "{}", out.text);
        assert_eq!((out.excluded_by_bytes, out.excluded_by_count), (2, 0));
        // i due esclusi restano riconoscibili dall'innesco
        assert!(out.text.contains("- secondo — quando capita il secondo caso"), "{}", out.text);
        assert!(out.text.contains("- terzo — quando capita il terzo caso"), "{}", out.text);
    }

    /// La sezione degli inneschi non cresce senza freno: è la malattia che la
    /// coda «Da rimisurare» aveva già avuto, e la cura è la stessa.
    #[test]
    fn the_trigger_section_cannot_grow_past_its_quota() {
        let items: Vec<(String, Option<String>)> = (0..30)
            .map(|i| {
                (
                    format!("f{i:02}.md"),
                    Some(live_with_trigger(
                        &format!("istinto-numero-{i:02}"),
                        0.9,
                        "quando capita un caso con un innesco lungo come quelli veri, che di byte ne prendono più di cento",
                        400,
                    )),
                )
            })
            .collect();
        let out = select(&items, "2026-08-15");
        let triggers = triggers_of(&out);
        // rotto così → rosso: una sezione senza quota arriva a migliaia di byte
        assert!(
            triggers.len() <= MAX_TRIGGER_BYTES,
            "sezione di {} byte oltre la quota di {MAX_TRIGGER_BYTES}",
            triggers.len()
        );
        assert!(triggers.contains("non elencate"), "le righe tagliate si dichiarano: {triggers}");
    }
}
