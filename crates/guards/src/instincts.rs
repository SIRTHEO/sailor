//! Un istinto vive solo se porta chi lo regge: `measured`, `expires`, `measure`.
//! Senza scadenza un istinto scritto una volta varrebbe per sempre — qui decide
//! `select()`, pura: riceve i testi e la data di oggi, sceglie chi si inietta
//! intero, chi è scaduto e chi non ha mai avuto una misura.
//!
//! Il parser del frontmatter è uno scanner riga per riga apposta, non un parser
//! YAML: i cinque campi che contano sono righe piatte fra due `---`, e un
//! valore con `:` dentro (un comando, un orario) resta intero perché si separa
//! solo sul primo.

/// L'intestazione fissa del blocco iniettato, invariata dallo script che sostituisce.
const HEADER: &str = "# Learned instincts (from your own past session observations)\n\nHeuristics with a confidence score, not hard rules. Apply when the trigger matches.\n\n";

/// I soli campi del frontmatter che contano per la decadenza.
#[derive(Default)]
struct Frontmatter {
    id: Option<String>,
    confidence: Option<f64>,
    measured: Option<String>,
    expires: Option<String>,
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
fn parse_frontmatter(text: &str) -> Frontmatter {
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
            "confidence" => fm.confidence = value.parse().ok(),
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
fn is_iso_date(s: &str) -> bool {
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
}

/// Sceglie quali istinti iniettare interi e quali ridurre a una riga.
///
/// `today` e le date del frontmatter sono confrontate come stringhe ISO
/// (`YYYY-MM-DD`): l'ordine lessicografico coincide con quello cronologico.
/// Un testo `None` è un file trovato ma non leggibile: conta come «senza
/// misura» invece di sparire dai tre numeri, che così restano esaustivi.
pub fn select(instincts: &[(String, Option<String>)], today: &str) -> Rendered {
    let mut live: Vec<(f64, String)> = Vec::new();
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
        match dating(&fm) {
            Dating::Both(_measured, expires) if today <= expires => {
                live_count += 1;
                live.push((fm.confidence.unwrap_or(0.0), text.clone()));
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

    // Confidence decrescente; a parità l'ordine di arrivo (sort stabile).
    live.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut text = String::new();
    text.push_str(HEADER);
    text.push_str(&format!(
        "Vivi: {live_count} · scaduti: {expired_count} · senza misura: {undated_count}\n\n"
    ));
    for (i, (_, body)) in live.iter().enumerate() {
        if i > 0 {
            text.push_str("\n\n");
        }
        text.push_str(body);
    }
    if !remeasure.is_empty() {
        if live_count > 0 {
            text.push_str("\n\n");
        }
        text.push_str("## Da rimisurare\n\n");
        text.push_str(&remeasure.join("\n"));
    }

    Rendered {
        text,
        live: live_count,
        expired: expired_count,
        undated: undated_count,
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

    #[test]
    fn a_live_instinct_is_injected_whole() {
        let text = instinct("vivo", "2026-08-01", "2026-08-30", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-15");
        // rotto così → rosso: cambiare `<=` in `<` esclude il giorno di scadenza
        assert!(out.text.contains("CORPO SEGRETO"));
        assert_eq!((out.live, out.expired, out.undated), (1, 0, 0));
    }

    #[test]
    fn an_instinct_expired_by_one_day_gives_only_the_line() {
        let text = instinct("scaduto", "2026-08-01", "2026-08-10", 0.9, "CORPO SEGRETO");
        let out = select(&[("f.md".into(), Some(text))], "2026-08-11");
        // rotto così → rosso: togliere il ramo `expired_count += 1` inietta il corpo
        assert!(!out.text.contains("CORPO SEGRETO"));
        assert!(out.text.contains("istinto scaduto: scaduto (misurato il 2026-08-01, scaduto il 2026-08-10)"));
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
        let pos_alto = out.text.find("ALTO").expect("ALTO presente");
        let pos_basso = out.text.find("BASSO").expect("BASSO presente");
        assert!(pos_alto < pos_basso, "atteso ALTO prima di BASSO: {}", out.text);
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

    /// Revisione: un `.md` trovato ma illeggibile spariva dai tre conteggi
    /// invece di finire in «senza misura».
    #[test]
    fn an_unreadable_file_counts_as_undated_instead_of_vanishing() {
        let out = select(&[("rotto.md".to_string(), None)], "2026-08-15");
        // rotto così → rosso: ignorare il ramo `None` lo farebbe sparire dal totale
        assert_eq!((out.live, out.expired, out.undated), (0, 0, 1));
        assert!(out.text.contains("istinto senza misura: rotto.md — illeggibile"));
    }
}
