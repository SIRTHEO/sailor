//! Le ore dichiarate nel frontmatter della coda dei guasti sono stimate a
//! mente, non lette dall'orologio: misurate 21/08/2026, nove ore su tutta la
//! coda dichiarate posteriori all'ultima scrittura del proprio file, scarti da
//! +4 a +67 minuti. Chi scriveva: «non mi ero posto la domanda».
//!
//! Si giudica **qualunque campo** del frontmatter il cui valore comincia con
//! una data riconosciuta, non solo `quando:`/`presa:`/`chiusa-il:`: un campo
//! nuovo inventato sul momento (`riscritta:`) nasce fuori sorveglianza se il
//! controllo è agganciato a una lista chiusa di nomi.

use crate::memory_anchor::frontmatter;
use crate::stale_facts::Date;
use regex::Regex;
use std::sync::OnceLock;

/// Il tempo che passa fra leggere l'orologio e chiudere la scrittura del
/// file: uno scarto entro questa soglia non è una stima, è la normale durata
/// del gesto. Le stime vere misurate in coda partono da +4 minuti, quindi
/// questa tolleranza non ne copre nessuna.
pub const TOLERANCE_MINUTES: i64 = 5;

/// Un'ora di calendario ridotta a ciò che serve: confrontarla con «adesso».
#[derive(Clone, Copy, Debug)]
pub struct Timestamp {
    date: Date,
    hour: i64,
    minute: i64,
}

impl Timestamp {
    /// `None` se la data non esiste o l'ora è fuori dai limiti di un giorno.
    pub fn new(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> Option<Timestamp> {
        let date = Date::new(year, month, day)?;
        if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
            return None;
        }
        Some(Timestamp { date, hour, minute })
    }

    fn minutes(&self) -> i64 {
        self.date.days() * 1440 + self.hour * 60 + self.minute
    }

    /// La forma «pronta da ricopiare» nel primo dei due formati riconosciuti.
    pub fn iso(&self) -> String {
        format!("{} {:02}:{:02}", self.date.iso(), self.hour, self.minute)
    }
}

fn iso_dt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2})").unwrap())
}

fn slash_dt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d{1,2})/(\d{1,2})/(\d{4}), ore (\d{1,2}):(\d{2})").unwrap())
}

/// Toglie le virgolette che racchiudono un valore YAML, se ci sono.
fn unquote(value: &str) -> &str {
    let v = value.trim();
    let bytes = v.as_bytes();
    if v.len() >= 2 && (bytes[0] == b'"' || bytes[0] == b'\'') && bytes[v.len() - 1] == bytes[0] {
        return &v[1..v.len() - 1];
    }
    v
}

/// Un'ora, se il valore **comincia** con una delle due forme riconosciute.
///
/// L'ancoraggio è solo a sinistra apposta: `presa: 2026-08-21 11:02 dal
/// macchinista…` porta un'attribuzione dopo l'ora e va giudicato lo stesso.
/// Una data in mezzo a una frase — «riaperta il 21/08 alle 09:50» — non
/// comincia il valore e non è comunque una delle due forme: resta cronaca, e
/// il controllo tace, che è l'unico verso senza falsi allarmi qui.
fn parse_timestamp(value: &str) -> Option<Timestamp> {
    let v = unquote(value);
    let n = |c: &regex::Captures, i: usize| c.get(i).unwrap().as_str().parse::<i64>().unwrap_or(-1);
    if let Some(c) = iso_dt_re().captures(v) {
        return Timestamp::new(n(&c, 1), n(&c, 2), n(&c, 3), n(&c, 4), n(&c, 5));
    }
    if let Some(c) = slash_dt_re().captures(v) {
        return Timestamp::new(n(&c, 3), n(&c, 2), n(&c, 1), n(&c, 4), n(&c, 5));
    }
    None
}

fn iso_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d{4})-(\d{2})-(\d{2})").unwrap())
}

fn slash_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d{1,2})/(\d{1,2})/(\d{4})").unwrap())
}

/// La data dichiarata da un valore di frontmatter, **senza pretendere l'ora**.
///
/// Vive qui e non altrove perché questo modulo è già il proprietario di «cosa
/// dichiara il frontmatter della coda»: un secondo lettore delle stesse due
/// forme divergerebbe alla prima correzione fatta a uno solo dei due.
///
/// Perché non basta [`parse_timestamp`]: metà delle voci scrive
/// `quando: 2026-08-24 (mattina)`, che non porta nessun `HH:MM` e che a un
/// controllo sull'ora è invisibile. Per l'età di una voce la sola data basta,
/// e pretendere l'ora avrebbe reso «senza data» proprio le voci più vecchie.
///
/// L'ancoraggio a sinistra resta quello di [`parse_timestamp`], e per la stessa
/// ragione: una data in mezzo a una frase è cronaca, non una dichiarazione.
pub fn declared_date(value: &str) -> Option<Date> {
    let v = unquote(value);
    let n = |c: &regex::Captures, i: usize| c.get(i).unwrap().as_str().parse::<i64>().unwrap_or(-1);
    if let Some(c) = iso_date_re().captures(v) {
        return Date::new(n(&c, 1), n(&c, 2), n(&c, 3));
    }
    if let Some(c) = slash_date_re().captures(v) {
        return Date::new(n(&c, 3), n(&c, 2), n(&c, 1));
    }
    None
}

/// Nome e valore di una riga `chiave: valore`, se la riga ne porta una.
fn field(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    let key = line[..idx].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, line[idx + 1..].trim()))
}

fn plural(n: i64) -> &'static str {
    if n == 1 {
        "minuto"
    } else {
        "minuti"
    }
}

fn violation_message(field: &str, declared: Timestamp, now: Timestamp, delta: i64) -> String {
    format!(
        "Il campo `{field}:` dichiara {} — {delta} {} nel futuro rispetto ad adesso: \
         quell'ora non era ancora arrivata quando il file si sta scrivendo, perché non \
         si legge l'orologio, si stima. Scrivi {}.",
        declared.iso(),
        plural(delta),
        now.iso(),
    )
}

/// Giudica il testo che sta per finire in un file della coda di bordo.
///
/// Cerca, in ogni campo del frontmatter YAML, un valore che comincia con una
/// data riconosciuta e nega la prima che risulta nel futuro oltre
/// [`TOLERANCE_MINUTES`]. Un'ora nel passato, per quanto lontana, non è mai
/// negata: una voce si riscrive ore dopo la sua nascita, ed è legittimo.
pub fn judge_frontmatter_times(text: &str, now: Timestamp, tolerance_minutes: i64) -> Option<String> {
    let front = frontmatter(text)?;
    let now_minutes = now.minutes();
    for line in front.lines() {
        let Some((key, value)) = field(line) else {
            continue;
        };
        let Some(declared) = parse_timestamp(value) else {
            continue;
        };
        let delta = declared.minutes() - now_minutes;
        if delta > tolerance_minutes {
            return Some(violation_message(key, declared, now, delta));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::new(2026, 8, 21, 11, 0).unwrap()
    }

    fn frontmatter_doc(body: &str) -> String {
        format!("---\n{body}\n---\n\ntesto del corpo, ignorato\n")
    }

    #[test]
    fn an_iso_future_time_is_rejected() {
        // Mutazione che lo rende rosso: cambiare `delta > tolerance_minutes`
        // in `delta > tolerance_minutes + 1000`. (Diceva «verde», ed era la
        // parola sbagliata: eseguita, la mutazione fa cadere sei casi.)
        let text = frontmatter_doc("quando: 2026-08-21 12:05");
        let msg = judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("2026-08-21 11:00"));
    }

    #[test]
    fn a_slash_future_time_is_rejected() {
        // Mutazione che lo rende rosso: far restituire sempre `None` da
        // `slash_dt_re()` invece della regex vera, così il secondo formato
        // smette di essere riconosciuto. (Diceva «verde», ed era la parola
        // sbagliata: eseguita, il caso cade.)
        let text = frontmatter_doc("presa: 21/08/2026, ore 12:05");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_some());
    }

    #[test]
    fn a_past_time_is_accepted() {
        // Mutazione che lo rende rosso: sostituire la condizione
        // `delta > tolerance_minutes` con `true`, cioè negare qualunque ora.
        let text = frontmatter_doc("chiusa-il: 2026-08-19 09:00");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_none());
    }

    #[test]
    fn a_time_inside_the_tolerance_is_accepted() {
        // Mutazione che lo rende rosso: cambiare `>` in `>=` nel confronto
        // dentro `judge_frontmatter_times`, così il confine stesso della
        // tolleranza viene negato.
        let text = frontmatter_doc("quando: 2026-08-21 11:05");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_none());
    }

    #[test]
    fn an_unreadable_date_is_silently_accepted() {
        // Mutazione che lo rende rosso: far restituire a `parse_timestamp` un
        // istante **futuro** fisso invece di fallire.
        //
        // Il «futuro» non è un dettaglio, ed è una correzione a me stesso: qui
        // c'era scritto «un valore qualunque», e con un istante passato il caso
        // resta verde — perché a negare è il verso, non il riconoscimento. Una
        // dichiarazione vaga sul punto che conta è lo stesso difetto che questo
        // commento denuncia due righe sotto, commesso nel gesto di ripararlo.
        //
        // NON PROVA L'ANCORAGGIO, e il suo commento diceva di sì: questo testo
        // non ha una sola cifra, quindi resta muto anche con le espressioni
        // disancorate. Il caso che l'ancoraggio lo prova davvero è quello qui
        // sotto — misurato da un revisore che ha eseguito la mutazione e ha
        // visto la batteria restare tutta verde.
        let text = frontmatter_doc("quando: boh, non me lo ricordo");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_none());
    }

    #[test]
    fn a_full_date_in_the_middle_of_a_value_is_not_a_declaration() {
        // L'ANCORAGGIO È IL CONFINE FRA CONTROLLO E MOLESTIA, e questo è
        // l'unico caso che lo prova: una data nel formato **completo**, futura,
        // in mezzo a una frase. Chi scrive «vedi anche il turno del 25» non sta
        // dichiarando quando scrive, e negarglielo insegna ad aggirare il
        // controllo — che è il difetto peggiore possibile qui.
        //
        // Mutazione che lo rende rosso: togliere `^` dalle due espressioni.
        let text = frontmatter_doc("nota: vedi anche 2026-08-25 09:00 per il turno successivo");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_none());

        // E lo stesso con l'altro formato, che è quello scritto a mano.
        let slash = frontmatter_doc("nota: come il 25/08/2026, ore 09:00 gia' fissato");
        assert!(judge_frontmatter_times(&slash, now(), TOLERANCE_MINUTES).is_none());
    }

    #[test]
    fn text_without_frontmatter_is_accepted() {
        // Mutazione che lo rende rosso: far restituire a `frontmatter()` il
        // testo intero quando non trova un `---` in testa, invece di `None`.
        assert!(judge_frontmatter_times(
            "# nota\n\nquando: 2026-08-21 12:05\n",
            now(),
            TOLERANCE_MINUTES
        )
        .is_none());
    }

    #[test]
    fn all_three_known_field_names_are_judged() {
        for field in ["quando", "presa", "chiusa-il"] {
            let text = frontmatter_doc(&format!("{field}: 2026-08-21 12:05"));
            assert!(
                judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_some(),
                "il campo {field} doveva essere negato"
            );
        }
    }

    #[test]
    fn a_field_name_invented_on_the_spot_is_judged_too() {
        // Il caso vero del 21/08/2026: `riscritta:` non esiste nel formato,
        // e un controllo agganciato ai tre nomi noti non l'avrebbe visto.
        let text = frontmatter_doc("riscritta: 2026-08-21 11:40");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_some());
    }

    #[test]
    fn a_date_told_as_chronicle_inside_a_sentence_is_not_flagged() {
        // Non comincia il valore e non è nella forma riconosciuta: cronaca su
        // un fatto passato, non una dichiarazione di quando si scrive.
        let text = frontmatter_doc("stato: riaperta il 21/08 alle 09:50, dopo controllo");
        assert!(judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).is_none());
    }

    #[test]
    fn a_leading_timestamp_with_trailing_attribution_is_still_judged() {
        // Il caso vero di `presa:`: l'ora apre il valore, il resto è
        // attribuzione. Deve restare giudicabile.
        let text = frontmatter_doc(
            "presa: 2026-08-21 12:05 dal macchinista di guardia (d9fed018) — presa nello stesso gesto",
        );
        let msg = judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES);
        assert!(msg.is_some());
    }

    #[test]
    fn the_message_carries_the_correct_hour_ready_to_copy() {
        let text = frontmatter_doc("chiusa-il: 2026-08-21 12:05");
        let msg = judge_frontmatter_times(&text, now(), TOLERANCE_MINUTES).unwrap();
        assert!(msg.contains("`chiusa-il:`"));
        assert!(msg.contains("2026-08-21 11:00"));
        assert!(msg.contains("65 minuti"));
    }
}
