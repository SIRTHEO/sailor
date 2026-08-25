//! Quando una consegna smette di essere un ordine e torna a essere il piano di
//! quel giorno.
//!
//! IL DIFETTO. Una consegna **deve** dire cosa fare dopo: è il punto di ripresa,
//! e senza quella sezione chi arriva ricostruisce da capo. Il guasto è che
//! continua a dirlo mesi dopo, **con lo stesso tono di voce del primo giorno**.
//! La misura del 24/08/2026 (`docs/2026-08-24-quante-memorie-mentono.md`) ha
//! verificato 184 affermazioni su dodici memorie: il racconto regge quasi
//! sempre — i commit citati esistono, i numeri di allora erano giusti — e le
//! trenta affermazioni false stanno quasi tutte **nella coda del documento**,
//! cioè esattamente dove guarda per primo chi riprende un lavoro.
//!
//! COSA FA QUESTO MODULO, e cosa non fa. Non cancella niente e non giudica il
//! merito: mette **sopra** la sezione operativa un rilievo che dice tre cose —
//! quanti giorni ha, che era il piano di quel giorno e non un ordine per oggi,
//! e quale consegna l'ha raccolta quando qualcuno l'ha scritto. Il testo resta
//! parola per parola: chi legge decide, ma decide sapendo l'età di ciò che sta
//! leggendo.
//!
//! COME SI RICONOSCE UNA CONSEGNA SUPERATA, in ordine di forza e con quanto
//! vale ciascun criterio sul corpus vero, misurato il 24/08/2026 su 80
//! consegne:
//! 1. **un'altra memoria la nomina con `[[…]]` dicendo di raccoglierla** — la
//!    prova più forte, perché è qualcuno che *sa*. Ne copre **1 su 80**: oggi
//!    quel gesto non si fa quasi mai, ed è la ragione per cui `commands/
//!    handoff.md` adesso lo prescrive al punto 4. Il criterio è debole di
//!    copertura, non di qualità, e si popola in avanti;
//! 2. **la sessione che l'ha scritta è chiusa** — nessuno sta più portando
//!    avanti quel piano. Ne copre **78 su 80**.
//!
//! **L'ETÀ NON È UN CRITERIO**, e qui non lo diventa di straforo: entra nel
//! testo del rilievo perché chi legge deve saperla, ma non decide chi viene
//! marcato. Una soglia in giorni sarebbe un numero inventato — e il mandato che
//! ha aperto questo lavoro chiedeva di dirlo invece di inventarla.
//!
//! L'ETÀ SI PRENDE DAL FRONTMATTER, MAI DALL'ORA DEL FILE. Una memoria si
//! riscrive — un'ancora ritimbrata, una riga d'indice — e l'ultima modifica del
//! disco direbbe «di oggi» su un piano di due settimane fa. Vale la stessa
//! regola già scritta per le voci di coda, e per la stessa ragione: una misura
//! presa sull'`mtime` il 21/08/2026 dichiarò «17 su 17 mai riprese», comprese
//! le nove chiuse.

use crate::memory_anchor::frontmatter;
use crate::regen_block::{quoted, RegenBlock};
use crate::stale_facts::Date;

/// L'etichetta del blocco rigenerabile scritto dentro una consegna.
///
/// Diversa da quella della coda di proposito: i due meccanismi possono marcare
/// documenti che vivono nella stessa cartella, e due blocchi con la stessa
/// etichetta si cancellerebbero a vicenda senza dare errore.
pub const BLOCK_TAG: &str = "freschezza-consegna";

/// I titoli sotto cui una consegna scrive il passo successivo.
///
/// L'elenco è a mano e **misurato**, non dedotto: sulle 84 consegne scritte
/// fino al 21/08/2026, 68 hanno una di queste sezioni — l'81%. Vive qui e non
/// nel braccio che legge il punto di ripresa perché adesso lo guardano in due,
/// e due copie della stessa lista divergono alla prima aggiunta.
pub const RESUME_HEADINGS: &[&str] = &[
    "prossimi passi",
    "prossimo passo",
    "da qui",
    "ripartenza",
    "punto di ripresa",
];

/// I titoli che aprono una sezione operativa: quella che dice cosa fare dopo.
///
/// Contiene [`RESUME_HEADINGS`] e ci aggiunge le forme che il censimento del
/// 24/08/2026 ha trovato sul parco intero — 96 memorie su 613 ne hanno una.
/// Il test `every_resume_heading_is_operative_too` impedisce alle due liste di
/// divergere: una sezione che il presidio della consegna legge come punto di
/// ripresa e questo modulo non riconoscesse resterebbe senza rilievo.
pub const OPERATIVE_HEADINGS: &[&str] = &[
    "prossimi passi",
    "prossimo passo",
    "da qui",
    "ripartenza",
    "punto di ripresa",
    "cosa resta",
    "resta da fare",
    "da fare",
    "next steps",
];

/// Le code che rovesciano il senso di un titolo che comincia come operativo.
///
/// «Cosa resta **vero**» è una constatazione, non un ordine: è il titolo di
/// `cartelle-orfane-non-sono-lavoro-in-bilico`, dove sotto ci sono quattro
/// misure e nessun compito. Il selettore che guarda solo il prefisso la
/// contava fra le memorie da disarmare — un falso positivo trovato leggendo il
/// corpus, non immaginandolo.
const NOT_ORDERS_AFTER_ALL: &[&str] = &["vero", "valido", "in piedi"];

/// Vero per un titolo che apre una sezione operativa.
///
/// «Azioni pendenti» si riconosce a parte perché in mezzo ci finisce di tutto —
/// «azioni **ancora** pendenti», «azioni pendenti su Theo» — e allungare
/// l'elenco con ogni variante è il modo in cui una lista smette di essere
/// leggibile.
fn is_operative_title(title: &str) -> bool {
    let starts = OPERATIVE_HEADINGS.iter().any(|h| title.starts_with(h))
        || (title.starts_with("azioni") && title.contains("pendenti"));
    starts && !NOT_ORDERS_AFTER_ALL.iter().any(|w| title.contains(w))
}

/// Il titolo ridotto a ciò che si confronta: senza cancelletti, senza enfasi,
/// minuscolo.
fn normalise_title(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix('#')?;
    Some(
        rest.trim_start_matches('#')
            .trim()
            .trim_matches(|c| matches!(c, '*' | '_' | ' '))
            .to_lowercase(),
    )
}

/// L'indice della riga che apre la sezione operativa, se c'è.
///
/// I blocchi recintati si saltano: una consegna che incolla il testo di un
/// altro documento — e succede, le consegne si citano fra loro — porterebbe
/// dentro un titolo che non è suo, e il rilievo finirebbe in mezzo a un
/// esempio.
///
/// **Cosa non prende, dichiarato**: una sezione operativa scritta senza titolo,
/// per esempio una riga in grassetto in mezzo al testo. Ne esiste almeno una
/// nota, ed è la più pericolosa del campione del 24/08/2026. Nessun filtro di
/// forma la prende, e fingere il contrario sarebbe peggio che dirlo.
pub fn operative_line(text: &str) -> Option<usize> {
    let mut fenced = false;
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if normalise_title(line).is_some_and(|t| is_operative_title(&t)) {
            return Some(i);
        }
    }
    None
}

/// Vero per il nome di file di una consegna: il punto di ripresa di una
/// sessione, l'unico documento che ha titolo di dire cosa fare dopo.
///
/// Il nome, e non il `type:` del frontmatter: `type: project` sta anche sulle
/// memorie di progetto che consegne non sono, mentre il prefisso lo impone
/// `commands/handoff.md` da sempre ed è come le consegne si citano fra loro.
pub fn is_handoff(file_name: &str) -> bool {
    ["consegna-", "handoff-", "punti-di-ripresa"]
        .iter()
        .any(|p| file_name.starts_with(p))
}

/// Quando è stata scritta una consegna, e quanto è affidabile la risposta.
///
/// Sono due domande diverse che il 24/08/2026 sono state confuse, con un
/// esempio che è la ragione per cui questo tipo esiste: il rilievo scriveva
/// «questa consegna è del 2026-08-24» su una consegna del **21/08**, perché
/// quel giorno qualcuno aveva corretto il file e `metadata.modified` era
/// diventato l'ora della correzione. Il frontmatter non è più affidabile
/// dell'`mtime` per questa domanda: è l'ultima volta che qualcuno **ha
/// toccato** il documento, non l'ultima volta che ha misurato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// La data di scrittura, letta dal nome del file — dove la mette
    /// `commands/handoff.md` e dove nessuna riscrittura la sposta.
    Certain(Date),
    /// Solo l'ultima riscrittura, da `metadata.modified`: si usa quando il nome
    /// non porta una data, e il rilievo dice che è quella, non altro.
    Rewritten(Date),
}

impl Written {
    pub fn date(&self) -> Date {
        match self {
            Written::Certain(d) | Written::Rewritten(d) => *d,
        }
    }
}

/// Quanto può stare avanti una data di scrittura rispetto a oggi prima di
/// essere letta come dell'anno scorso.
///
/// Un giorno, e viene da un caso vero: `consegna-19-08-analisi-cicd-e-coda`
/// porta `modified: 2026-08-18T22:49Z`, che in ora locale è **il 19 alle
/// 00:49**. Con `modified` come riferimento e tolleranza zero, il 19/08
/// risultava «nel futuro» e la consegna finiva datata **2025**: 370 giorni
/// invece di 5. Da qui le due correzioni insieme — il riferimento è oggi, non
/// l'ultima riscrittura, e un giorno di scarto non fa un anno.
const FUTURE_TOLERANCE_DAYS: i64 = 1;

/// La data che il nome del file dichiara, nelle due forme che questo parco usa.
///
/// `handoff-suite-redesign-2026-06-09.md` porta l'anno; `consegna-21-08-…` no,
/// e allora l'anno è quello di **oggi**, scalato di uno se la data cadrebbe nel
/// futuro: una consegna del 30/12 letta il 5 gennaio è dell'anno prima. Il
/// riferimento è oggi e non l'ultima riscrittura perché oggi non mente —
/// nessuna consegna è scritta domani, mentre `modified` può stare da una parte
/// o dall'altra della mezzanotte per il solo fuso orario.
pub fn name_date(file_name: &str, reference: Option<Date>) -> Option<Date> {
    let digits: Vec<&str> = file_name
        .trim_end_matches(".md")
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    // Forma piena in coda: …-2026-06-09
    if let [.., y, m, d] = digits.as_slice() {
        if y.len() == 4 {
            if let (Ok(y), Ok(m), Ok(d)) = (y.parse(), m.parse(), d.parse()) {
                return Date::new(y, m, d);
            }
        }
    }
    // Forma corta in testa: consegna-21-08-…
    let (day, month) = (digits.first()?, digits.get(1)?);
    if day.len() != 2 || month.len() != 2 {
        return None;
    }
    let (day, month) = (day.parse().ok()?, month.parse().ok()?);
    let reference = reference?;
    let candidate = Date::new(reference.year, month, day)?;
    if candidate.days() - reference.days() > FUTURE_TOLERANCE_DAYS {
        return Date::new(reference.year - 1, month, day);
    }
    Some(candidate)
}

/// Quando la consegna è stata scritta: il nome se lo dice, l'ultima riscrittura
/// altrimenti — e chi legge sa quale delle due sta guardando.
///
/// `today` serve solo a dare l'anno ai nomi che portano il solo giorno e mese.
pub fn written(file_name: &str, text: &str, today: Option<Date>) -> Option<Written> {
    match name_date(file_name, today) {
        Some(d) => Some(Written::Certain(d)),
        None => declared_date(text).map(Written::Rewritten),
    }
}

/// L'ultima riscrittura dichiarata dal frontmatter, mai l'ora del file.
///
/// Si legge `metadata.modified`. **Non è la data di scrittura** — vedi
/// [`Written`] — ed è il ripiego, non la fonte. `None` quando manca o non si
/// legge: «non lo so» non è «è vecchia», e il rilievo lo dirà invece di stimare.
pub fn declared_date(text: &str) -> Option<Date> {
    let front = frontmatter(text)?;
    let value = front
        .lines()
        .find_map(|l| l.trim().strip_prefix("modified:"))?
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\''));
    let n = |a: usize, b: usize| value.get(a..b).and_then(|s| s.parse::<i64>().ok());
    Date::new(n(0, 4)?, n(5, 7)?, n(8, 10)?)
}

/// Gli otto caratteri con cui una sessione si firma, dal frontmatter.
///
/// Otto e non l'identificativo intero perché è così che si firmano i nomi delle
/// consegne e i marcatori di stato: confrontare forme diverse della stessa
/// identità è il modo di non trovare mai una corrispondenza.
pub fn origin_session(text: &str) -> Option<String> {
    let front = frontmatter(text)?;
    let value = front
        .lines()
        .find_map(|l| l.trim().strip_prefix("originSessionId:"))?
        .trim();
    (value.len() >= 8).then(|| value[..8].to_string())
}

/// I verbi con cui una consegna dichiara di raccoglierne un'altra.
///
/// Un rimando `[[…]]` da solo non basta e non deve bastare: le consegne si
/// citano di continuo per dire «vedi anche», e leggere ogni citazione come un
/// sorpasso marcherebbe come superata anche la consegna che qualcuno sta
/// citando **perché è ancora valida**.
pub const COLLECT_VERBS: &[&str] = &[
    "raccogl",
    "raccolt",
    "riprend",
    "ripres",
    "prosegu",
    "supera",
    "superat",
    "sostituis",
    "sostituit",
    "chiude",
    "chiusa da",
    "continua",
];

/// Vero se questa riga dichiara di raccogliere la consegna che nomina.
pub fn collects(line: &str, name: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains(&format!("[[{}]]", name.to_lowercase()))
        && COLLECT_VERBS.iter().any(|v| lower.contains(v))
}

/// Quel che si sa di una consegna al momento di decidere se marcarla.
#[derive(Debug, Default, Clone)]
pub struct Freshness {
    /// Da quanti giorni, e da quale data contati — la data entra nel rilievo
    /// insieme ai giorni, perché un numero che nessuno può rifare non è una
    /// misura.
    pub age: Option<(i64, Written)>,
    /// La sessione che l'ha scritta non è più viva.
    pub session_closed: bool,
    /// La consegna che ha dichiarato di raccoglierla.
    pub collector: Option<String>,
}

/// La data com'è scritta nel rilievo, con detto da dove viene.
///
/// «Ultima riscrittura» non è un dettaglio da nascondere: è la differenza fra
/// una data che chi legge può usare e una che lo manderebbe fuori strada.
fn stamp(when: Written) -> String {
    match when {
        Written::Certain(d) => format!("scritta il {}", d.iso()),
        Written::Rewritten(d) => format!("ultima riscrittura {}, data di scrittura ignota", d.iso()),
    }
}

/// Il corpo del rilievo, o niente quando non c'è niente da dire.
///
/// Niente da dire vuol dire una cosa sola: **la sessione che l'ha scritta è
/// ancora viva e nessuno l'ha raccolta**, cioè quel piano è il lavoro di
/// adesso. Marcarlo lì sarebbe rumore sulla sola consegna che invece va letta
/// come sta scritta.
pub fn block_body(f: &Freshness) -> Option<String> {
    if !f.session_closed && f.collector.is_none() {
        return None;
    }
    let mut lines = Vec::new();
    // I giorni valgono solo insieme alla data da cui sono contati: senza,
    // «ferma da 12 giorni» è un numero che nessuno può rifare. E quando la data
    // manca si dice, non si stima — l'ora del file direbbe «di oggi» su un
    // piano di due settimane fa, riscritto per un'ancora ritimbrata.
    // Il testo cambia con l'età e con la fonte della data. Con l'età perché su
    // una consegna di oggi «era il piano di quel giorno» suonerebbe falso, e
    // una riga che suona falsa non la crede nessuno nemmeno dove dice il vero
    // («ferma da 1 giorni» fa lo stesso danno, in piccolo). Con la fonte perché
    // dire «questa consegna è del 24/08» su una consegna del 21 è un errore
    // vero, già capitato: quando la data è solo l'ultima riscrittura si scrive
    // che è quella, e chi legge sa quanto fidarsi.
    lines.push(match f.age {
        Some((0, when)) => format!(
            "> **NON È UN ORDINE PER CHI ARRIVA** — la sessione che ha scritto questa \
             consegna ({}) è chiusa. Quello che segue era **il suo piano**: rimisuralo \
             prima di eseguirlo.",
            stamp(when)
        ),
        Some((d, when)) => {
            let how_long = if d == 1 {
                "ferma da un giorno".to_string()
            } else {
                format!("ferma da {d} giorni")
            };
            format!(
                "> **NON È UN ORDINE PER OGGI** — questa consegna è {how_long} ({}), e la \
                 sessione che l'ha scritta è chiusa. Quello che segue era **il piano di quel \
                 giorno**: rimisuralo prima di eseguirlo.",
                stamp(when)
            )
        }
        // Senza una data leggibile non si stima: l'ora del file direbbe «di
        // oggi» su un piano di due settimane fa, riscritto per un'ancora
        // ritimbrata.
        None => "> **NON È UN ORDINE PER OGGI** — la sessione che ha scritto questa consegna è \
                 chiusa, e non se ne legge la data né dal nome né dal frontmatter. Quello che \
                 segue era il piano di allora: rimisuralo prima di eseguirlo."
            .to_string(),
    });
    if let Some(collector) = &f.collector {
        lines.push(format!(
            "> **L'ha raccolta `{collector}`**: se le due si contraddicono, vale quella."
        ));
    }
    quoted(&lines)
}

/// Il segnaposto della riga dove va infilato il blocco.
///
/// Serve perché l'inserimento lavora sulle righe e la resa sui byte: senza un
/// punto fermo, ricomporre il testo attorno al blocco vuol dire contare a mano
/// gli a capo, che è il conto che il 24/08/2026 faceva crescere le voci di coda
/// di una riga vuota a ogni passata. Non può comparire in un documento vero:
/// se comparisse, il primo `replacen` prenderebbe quello e il rilievo finirebbe
/// nel posto sbagliato — per questo è fatto di caratteri che nessuno digita.
const PLACEHOLDER: &str = "\u{0}rilievo\u{0}";

/// La consegna col rilievo rigenerato sopra la sua sezione operativa.
///
/// Sopra e non sotto il frontmatter — che è dove sta nelle voci di coda —
/// perché qui la domanda è diversa: il frontmatter di una memoria lo legge chi
/// cerca, la sezione operativa la legge chi **agisce**, e il rilievo serve a
/// quest'ultimo nel momento esatto in cui sta per farlo.
///
/// Senza sezione operativa non si marca niente: non c'è nessun ordine da
/// disarmare, e un avviso in testa a un racconto sposta solo l'occhio.
pub fn with_block(text: &str, body: Option<&str>) -> String {
    let block = RegenBlock::new(BLOCK_TAG);
    let bare = block.strip(text);
    let Some(body) = body else {
        return bare;
    };
    let Some(at) = operative_line(&bare) else {
        return bare;
    };
    let mut out: Vec<&str> = Vec::new();
    for (i, line) in bare.lines().enumerate() {
        if i == at {
            out.push(PLACEHOLDER);
        }
        out.push(line);
    }
    let joined = out.join("\n");
    let rendered = joined.replacen(PLACEHOLDER, &format!("{}\n", block.render(body)), 1);
    // `lines()` mangia l'a capo finale: senza rimetterlo, ogni passata
    // toglierebbe un byte al file e due passate non darebbero gli stessi byte.
    if bare.ends_with('\n') {
        format!("{rendered}\n")
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(front: &str, body: &str) -> String {
        format!("---\n{front}\n---\n\n{body}\n")
    }

    #[test]
    fn every_resume_heading_is_operative_too() {
        // Le due liste non possono divergere: una sezione che il presidio della
        // consegna legge come punto di ripresa e questo modulo non riconoscesse
        // resterebbe senza rilievo, che è il caso peggiore — l'ordine più
        // vecchio è proprio quello che qualcuno andrà a eseguire.
        for h in RESUME_HEADINGS {
            assert!(OPERATIVE_HEADINGS.contains(h), "manca fra gli operativi: {h}");
        }
    }

    #[test]
    fn a_heading_is_found_in_bold_and_with_a_tail() {
        assert_eq!(operative_line("## Prossimi passi\n"), Some(0));
        assert_eq!(
            operative_line("corpo\n\n### **Cosa resta**, e di chi è\n"),
            Some(2)
        );
        assert_eq!(operative_line("## Azioni ancora pendenti\n"), Some(0));
        // Ma «azioni» da solo non basta: quello che conta è che siano pendenti.
        // Mutazione che lo rende rosso: `&&` al posto di `||` in
        // `is_operative_title`.
        assert_eq!(operative_line("## Azioni svolte oggi\n"), None);
        // E un titolo che non apre una sezione operativa non conta.
        assert_eq!(operative_line("## Gotcha / landmine\n"), None);
    }

    #[test]
    fn what_stays_true_is_not_an_order() {
        // IL CASO VERO che ha corretto il criterio: «Cosa resta vero» è il
        // titolo di `cartelle-orfane-non-sono-lavoro-in-bilico`, e sotto ci
        // sono quattro misure, non un compito. Mutazione che lo rende rosso:
        // togliere `NOT_ORDERS_AFTER_ALL` da `is_operative_title`.
        assert_eq!(operative_line("## Cosa resta vero\n"), None);
        // Ma la sezione che dice cosa resta da fare resta operativa.
        assert_eq!(operative_line("## Cosa resta aperto, e di chi è\n"), Some(0));
    }

    #[test]
    fn only_a_handoff_name_is_a_handoff() {
        assert!(is_handoff("consegna-24-08-riordino-aa0750a3.md"));
        assert!(is_handoff("handoff-bdg-2026-05-29.md"));
        assert!(is_handoff("punti-di-ripresa.md"));
        // Una memoria di fatto non lo è, nemmeno se parla di consegne.
        assert!(!is_handoff("consegnare-non-basta-serve-chi-raccoglie.md"));
        assert!(!is_handoff("tab-orca-si-accumulano.md"));
    }

    #[test]
    fn a_heading_inside_a_fence_is_not_a_heading() {
        // Mutazione che lo rende rosso: togliere il salto delle recinzioni da
        // `operative_line`. Una consegna che incolla il testo di un'altra
        // porterebbe dentro un titolo non suo, e il rilievo finirebbe in mezzo
        // a un esempio.
        let text = "corpo\n\n```\n## Prossimi passi\n```\n\n## Decisioni\n";
        assert_eq!(operative_line(text), None);
    }

    #[test]
    fn the_date_comes_from_the_frontmatter_and_nothing_stands_in_for_it() {
        let text = doc(
            "name: consegna-x\nmetadata:\n  modified: 2026-08-23T17:36:56.796Z",
            "# titolo",
        );
        assert_eq!(declared_date(&text), Date::new(2026, 8, 23));
        // Senza il campo non si stima: «non lo so» non è «è vecchia».
        assert_eq!(declared_date(&doc("name: x", "# titolo")), None);
        // E una data impossibile non è una data.
        assert_eq!(
            declared_date(&doc("metadata:\n  modified: 2026-02-31T00:00:00Z", "x")),
            None
        );
    }

    #[test]
    fn a_session_signature_is_eight_characters() {
        let text = doc(
            "metadata:\n  originSessionId: f6ff0d4f-3e27-462b-9066-5b0305bc2461",
            "# titolo",
        );
        assert_eq!(origin_session(&text).as_deref(), Some("f6ff0d4f"));
        assert_eq!(origin_session(&doc("metadata:\n  type: project", "x")), None);
    }

    #[test]
    fn a_mention_is_not_a_takeover() {
        // IL DIFETTO CHE QUESTO CASO ESISTE PER PRENDERE: le consegne si citano
        // di continuo per dire «vedi anche». Leggere ogni `[[…]]` come un
        // sorpasso marcherebbe come superata anche la consegna citata perché è
        // ancora valida.
        assert!(!collects(
            "vedi anche [[consegna-a]] per il contesto",
            "consegna-a"
        ));
        assert!(collects(
            "Raccoglie [[consegna-a]], che si chiude qui",
            "consegna-a"
        ));
        // E il verbo non vale su un'altra consegna nominata nella stessa riga.
        assert!(!collects("raccoglie [[consegna-b]]", "consegna-a"));
    }

    #[test]
    fn the_writing_date_comes_from_the_name_not_from_a_later_rewrite() {
        // IL CASO VERO, segnalato il 24/08/2026 mentre il rilievo veniva
        // copiato sulle consegne: `consegna-21-08-capitano-permessi-e-nucleo`
        // era stata corretta quel giorno, quindi `modified` diceva 24/08 e il
        // rilievo scriveva «questa consegna è del 24/08» su un piano del 21.
        // Mutazione che lo rende rosso: far tornare a `written` il solo
        // `declared_date`.
        let text = doc(
            "name: x\nmetadata:\n  modified: 2026-08-24T15:00:00.000Z",
            "# titolo",
        );
        let today = Date::new(2026, 8, 24);
        let w = written(
            "consegna-21-08-capitano-permessi-e-nucleo-8f0b883e.md",
            &text,
            today,
        );
        assert_eq!(w, Some(Written::Certain(Date::new(2026, 8, 21).unwrap())));
        assert!(stamp(w.unwrap()).contains("scritta il 2026-08-21"));

        // La forma con l'anno per esteso in coda al nome.
        assert_eq!(
            written("handoff-suite-redesign-2026-06-09.md", &text, today),
            Some(Written::Certain(Date::new(2026, 6, 9).unwrap()))
        );
        // Un nome senza data: si ripiega sull'ultima riscrittura, e il rilievo
        // dichiara che è quella.
        let w = written("consegna-senza-data.md", &text, today).unwrap();
        assert_eq!(w, Written::Rewritten(Date::new(2026, 8, 24).unwrap()));
        assert!(stamp(w).contains("data di scrittura ignota"));
        // Senza nessuna delle due non si inventa niente.
        assert_eq!(
            written("consegna-senza-data.md", "niente frontmatter", today),
            None
        );
    }

    #[test]
    fn a_short_name_takes_the_year_that_does_not_fall_in_the_future() {
        // Una consegna del 30/12 letta il 5 gennaio è dell'anno prima: prendere
        // l'anno di oggi a occhi chiusi la daterebbe fra undici mesi.
        // Mutazione che lo rende rosso: togliere lo scalo di un anno da
        // `name_date`.
        assert_eq!(
            name_date("consegna-30-12-una-cosa.md", Date::new(2027, 1, 5)),
            Date::new(2026, 12, 30)
        );
        // E una data del tutto impossibile non è una data.
        assert_eq!(name_date("consegna-32-13-x.md", Date::new(2027, 1, 5)), None);
    }

    #[test]
    fn a_day_ahead_is_a_time_zone_not_a_year() {
        // IL CASO VERO che ha corretto il criterio, trovato marcando il parco:
        // `consegna-19-08-analisi-cicd-e-coda` risultava «ferma da 370 giorni»
        // perché la sua data cadeva un giorno avanti al riferimento e veniva
        // letta come dell'anno prima. Mutazione che lo rende rosso: portare
        // `FUTURE_TOLERANCE_DAYS` a zero.
        assert_eq!(
            name_date("consegna-19-08-analisi-cicd-e-coda.md", Date::new(2026, 8, 18)),
            Date::new(2026, 8, 19)
        );
        // Ma una data avanti di mesi resta dell'anno prima: la tolleranza non
        // si mangia il caso che lo scalo esiste per prendere.
        assert_eq!(
            name_date("consegna-19-12-x.md", Date::new(2026, 8, 18)),
            Date::new(2025, 12, 19)
        );
    }

    #[test]
    fn a_session_still_alive_gets_no_notice() {
        // La sola consegna che va letta come sta scritta è quella che qualcuno
        // sta ancora portando avanti.
        let alive = Freshness {
            age: Some((40, Written::Certain(Date::new(2026, 7, 15).unwrap()))),
            session_closed: false,
            collector: None,
        };
        assert!(block_body(&alive).is_none());
        // Ma se qualcuno l'ha raccolta, il rilievo esce anche a sessione viva.
        let taken = Freshness {
            collector: Some("consegna-b.md".to_string()),
            ..alive
        };
        let body = block_body(&taken).unwrap();
        assert!(body.contains("consegna-b.md"));
    }

    #[test]
    fn the_notice_states_the_age_and_says_when_it_does_not_know_it() {
        let when = Written::Certain(Date::new(2026, 8, 12).unwrap());
        let dated = Freshness {
            age: Some((12, when)),
            session_closed: true,
            collector: None,
        };
        let body = block_body(&dated).unwrap();
        assert!(body.contains("ferma da 12 giorni"));
        assert!(body.contains("scritta il 2026-08-12"));
        assert!(body.contains("il piano di quel giorno"));
        // Un giorno solo non si scrive «da 1 giorni»: chi legge un difetto di
        // forma smette di credere anche al resto della riga.
        let yesterday = Freshness { age: Some((1, when)), ..dated.clone() };
        assert!(block_body(&yesterday).unwrap().contains("ferma da un giorno"));
        // E su una consegna scritta oggi non si dice «era il piano di quel
        // giorno»: suonerebbe falso, e una riga che suona falsa non la crede
        // nessuno nemmeno dove dice il vero. Resta solo la ragione vera — la
        // sessione è chiusa.
        let same_day = Freshness { age: Some((0, when)), ..dated.clone() };
        let body = block_body(&same_day).unwrap();
        assert!(body.contains("NON È UN ORDINE PER CHI ARRIVA"));
        assert!(!body.contains("quel giorno"));
        assert!(body.contains("2026-08-12"));

        let undated = Freshness {
            session_closed: true,
            ..Default::default()
        };
        let body = block_body(&undated).unwrap();
        assert!(body.contains("non se ne legge la data"));
        // Mutazione che lo rende rosso: far stimare l'età dall'ora del file.
        assert!(!body.contains("giorni"));
    }

    #[test]
    fn the_notice_sits_above_the_operative_section_and_regenerates_in_place() {
        let text = doc(
            "name: consegna-x\nmetadata:\n  modified: 2026-08-12T09:00:00Z",
            "# titolo\n\n## Stato\n\nfatto tutto\n\n## Prossimi passi\n\n- spingi il ramo",
        );
        let body = block_body(&Freshness {
            age: Some((12, Written::Certain(Date::new(2026, 8, 12).unwrap()))),
            session_closed: true,
            collector: None,
        })
        .unwrap();
        let once = with_block(&text, Some(&body));

        // Sopra la sezione operativa, non sopra il documento.
        let notice = once.find("NON È UN ORDINE").unwrap();
        let section = once.find("## Prossimi passi").unwrap();
        let state = once.find("## Stato").unwrap();
        assert!(state < notice && notice < section);
        // Il frontmatter resta intatto e primo: lo legge un programma.
        assert!(once.starts_with("---\nname: consegna-x\n"));
        // Il testo dell'autore non si tocca.
        assert!(once.contains("- spingi il ramo"));

        // Idempotente: due passate danno gli stessi byte.
        assert_eq!(with_block(&once, Some(&body)), once);
        // E si toglie senza lasciare traccia.
        assert_eq!(with_block(&once, None), text);
    }

    #[test]
    fn without_an_operative_section_nothing_gets_marked() {
        // Non c'è nessun ordine da disarmare: un avviso in testa a un racconto
        // sposta solo l'occhio di chi legge.
        let text = doc("name: consegna-x", "# titolo\n\n## Gotcha\n\nuna trappola");
        let body = block_body(&Freshness {
            session_closed: true,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(with_block(&text, Some(&body)), text);
    }
}
