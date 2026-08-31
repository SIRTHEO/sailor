//! Il conteggio esatto di token e contesto, dalla risposta di un motore.
//!
//! `None` è una risposta legittima — un numero che quel motore non dice —
//! `0` non lo è mai per un campo mancante: qui non si inventa nulla.

use crate::catalog::Model;

/// I token misurati per una singola chiamata. `total_tokens` può essere
/// noto anche quando `prompt_tokens`/`completion_tokens` non lo sono (è il
/// caso di Codex, che sull'uscita dice solo il totale).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// Il costo in USD, quando il motore stesso lo dichiara (OpenRouter lo
    /// fa per ogni risposta, anche a zero sui modelli gratuiti).
    pub cost_usd: Option<f64>,
}

impl TokenUsage {
    /// Dal corpo JSON di una risposta `chat/completions` di OpenRouter.
    /// Un campo mancante o non numerico resta `None`, non fa fallire tutto
    /// il parsing: un corpo di errore (429, chiave non valida, ecc.) deve
    /// poter tornare un `TokenUsage` vuoto invece di un panico.
    pub fn from_openrouter_body(body: &str) -> TokenUsage {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
            return TokenUsage::default();
        };
        let usage = parsed.get("usage");
        let field_u64 = |key: &str| usage.and_then(|u| u.get(key)).and_then(|v| v.as_u64());
        TokenUsage {
            prompt_tokens: field_u64("prompt_tokens"),
            completion_tokens: field_u64("completion_tokens"),
            total_tokens: field_u64("total_tokens"),
            cost_usd: usage.and_then(|u| u.get("cost")).and_then(|v| v.as_f64()),
        }
    }

    /// Dall'uscita testuale di `codex exec`: riusa `parse_codex_tokens`
    /// invece di riscrivere il parsing (`"tokens used"` seguito dal numero
    /// con il punto come separatore delle migliaia). Codex non separa
    /// prompt e completamento, e non dichiara un costo: qui restano `None`.
    pub fn from_codex_output(output: &str) -> TokenUsage {
        let raw = parse_codex_tokens(output);
        TokenUsage { total_tokens: raw.parse().ok(), ..TokenUsage::default() }
    }
}

/// Il quadro completo di una chiamata: quanto è entrato, quanto è uscito,
/// quanto resta della finestra del modello, quanto è costata.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ContextAccounting {
    pub usage: TokenUsage,
    /// La finestra del modello usato: `None` se il modello non è nel
    /// catalogo (es. Codex, che non è un modello OpenRouter).
    pub context_length: Option<u64>,
    /// `context_length - total_tokens`. `None` se manca anche solo uno dei
    /// due addendi — un resto calcolato su un totale ignoto sarebbe un
    /// numero inventato con la faccia di una misura.
    pub remaining: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl ContextAccounting {
    /// Combina l'uso misurato con il modello del catalogo che l'ha servito
    /// (se lo si conosce). Se `usage.cost_usd` è già dichiarato dal motore
    /// (OpenRouter lo fa sempre) si usa quello; altrimenti si calcola dal
    /// listino del modello, quando il listino c'è.
    pub fn compute(usage: TokenUsage, model: Option<&Model>) -> ContextAccounting {
        let context_length = model.and_then(|m| m.context_length);
        let remaining = match (context_length, usage.total_tokens) {
            (Some(ctx), Some(total)) => Some(ctx.saturating_sub(total)),
            _ => None,
        };
        let cost_usd = usage.cost_usd.or_else(|| compute_cost(&usage, model));
        ContextAccounting { usage, context_length, remaining, cost_usd }
    }
}

/// Il costo dal listino del modello, quando il motore non l'ha già detto.
/// Serve entrambi i pezzi (prompt e completamento, prezzo e conteggio):
/// manca uno solo, il costo resta sconosciuto.
fn compute_cost(usage: &TokenUsage, model: Option<&Model>) -> Option<f64> {
    let model = model?;
    let prompt_tokens = usage.prompt_tokens? as f64;
    let completion_tokens = usage.completion_tokens? as f64;
    let price_in = model.price_per_million_input?;
    let price_out = model.price_per_million_output?;
    Some((prompt_tokens * price_in + completion_tokens * price_out) / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, Modality};

    // Corpo vero, catturato il 27/08/2026 su nvidia/nemotron-3-super-120b-a12b:free
    // (chiave letta da file, mai stampata; qui non ne resta traccia).
    const OPENROUTER_OK: &str = r#"{"id":"gen-1787833447-8R8Z6Ce3NQrwbjknBeMw","usage":{"prompt_tokens":27,"completion_tokens":20,"total_tokens":47,"cost":0,"is_byok":false}}"#;

    #[test]
    fn reads_all_three_token_counts_and_the_cost() {
        let usage = TokenUsage::from_openrouter_body(OPENROUTER_OK);
        assert_eq!(usage.prompt_tokens, Some(27));
        assert_eq!(usage.completion_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(47));
        assert_eq!(usage.cost_usd, Some(0.0));
    }

    #[test]
    fn a_429_body_has_no_usage_at_all_not_a_panic() {
        let body = r#"{"error":{"code":429,"message":"limite"}}"#;
        let usage = TokenUsage::from_openrouter_body(body);
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn garbage_input_gives_an_empty_usage_not_a_panic() {
        let usage = TokenUsage::from_openrouter_body("non è json");
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn codex_output_reuses_the_shared_parser() {
        let output = "roba varia\ntokens used\n13.910\naltra roba";
        let usage = TokenUsage::from_codex_output(output);
        assert_eq!(usage.total_tokens, Some(13910));
        assert_eq!(usage.prompt_tokens, None);
        assert_eq!(usage.completion_tokens, None);
        assert_eq!(usage.cost_usd, None);
    }

    #[test]
    fn codex_output_without_the_marker_is_unknown_not_zero() {
        let usage = TokenUsage::from_codex_output("nessuna riga utile qui");
        assert_eq!(usage.total_tokens, None);
    }

    fn sample_model(id: &str) -> Model {
        let catalog = Catalog::parse(include_str!("../tests/fixtures/catalog-sample.json")).unwrap();
        catalog.find(id).unwrap().clone()
    }

    #[test]
    fn computes_remaining_context_against_the_model_window() {
        let model = sample_model("nvidia/nemotron-3-super-120b-a12b:free"); // 262144
        let usage = TokenUsage { total_tokens: Some(47), ..TokenUsage::default() };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.context_length, Some(262144));
        assert_eq!(acc.remaining, Some(262144 - 47));
    }

    #[test]
    fn remaining_is_none_when_the_model_is_unknown_codex_case() {
        let usage = TokenUsage { total_tokens: Some(13910), ..TokenUsage::default() };
        let acc = ContextAccounting::compute(usage, None);
        assert_eq!(acc.context_length, None);
        assert_eq!(acc.remaining, None, "un resto su una finestra ignota sarebbe un numero inventato");
    }

    #[test]
    fn remaining_is_none_when_total_tokens_is_unknown() {
        let model = sample_model("nvidia/nemotron-3-super-120b-a12b:free");
        let acc = ContextAccounting::compute(TokenUsage::default(), Some(&model));
        assert_eq!(acc.remaining, None);
    }

    #[test]
    fn prefers_the_engines_own_declared_cost_over_a_computed_one() {
        let model = sample_model("qwen/qwen3.8-flash");
        let usage = TokenUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(100),
            cost_usd: Some(0.4242), // valore volutamente diverso dal calcolo, per provare che vince
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, Some(0.4242));
    }

    #[test]
    fn computes_cost_from_the_price_list_when_the_engine_says_nothing() {
        let model = sample_model("qwen/qwen3.8-flash"); // 0.15 / 0.47 USD per milione
        let usage = TokenUsage {
            prompt_tokens: Some(1_000_000),
            completion_tokens: Some(1_000_000),
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        let cost = acc.cost_usd.unwrap();
        assert!((cost - 0.62).abs() < 1e-9, "atteso 0.15+0.47=0.62, letto {cost}");
    }

    #[test]
    fn cost_is_unknown_when_token_counts_are_only_partial() {
        let model = sample_model("qwen/qwen3.8-flash");
        let usage = TokenUsage { prompt_tokens: Some(10), ..TokenUsage::default() }; // manca completion
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, None);
    }

    #[test]
    fn accepts_modality_check_is_reused_correctly() {
        let model = sample_model("thinkingmachines/inkling:free");
        assert!(model.accepts(Modality::Audio));
    }
}

/// I token dichiarati da `codex exec` nella sua uscita testuale.
///
/// Veniva da `notte`, rimosso dal repo il 29/08/2026. Sta qui perché leggere
/// quanto un motore dichiara di aver consumato è il mestiere di questo crate.
pub fn parse_codex_tokens(output: &str) -> String {
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "tokens used" {
            if let Some(num_line) = lines.next() {
                let cleaned: String = num_line.trim().chars().filter(|c| *c != '.').collect();
                if !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_digit()) {
                    return cleaned;
                }
            }
        }
    }
    "?".to_string()
}

// ── il consumo letto secondo ciò che un descrittore dichiara ──────────────

/// In che forma un motore dice il proprio consumo.
///
/// **PERCHÉ È UN DATO E NON UN RAMO PER FORNITORE.** `from_openrouter_body` e
/// `from_codex_output` qui sopra sono due letture scritte a mano per due
/// formati noti: aggiungerne una terza per ogni motore che nasce è esattamente
/// la strada che il vincolo di indipendenza dal modello vieta. Qui la forma è
/// dichiarata dal descrittore, e un motore che non esiste ancora si misura
/// scrivendo un file JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// L'uscita è un involucro JSON, e i puntatori sono cammini di chiavi.
    #[default]
    Json,
    /// L'uscita è testo, e i puntatori sono espressioni regolari con un gruppo
    /// di cattura.
    Text,
}

/// Dove sta un valore dentro ciò che il motore ha detto.
///
/// Le due forme non sono intercambiabili: un cammino di chiavi vale solo su un
/// corpo JSON, un'espressione regolare solo sul testo. Un puntatore della forma
/// sbagliata non trova niente e lascia il valore **sconosciuto** — che è il modo
/// giusto di sbagliare, perché nessun numero inventato prende il suo posto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pointer {
    /// Un cammino di chiavi dentro l'involucro: `["usage", "input_tokens"]`.
    Path(Vec<String>),
    /// Un'espressione regolare col numero (o il testo) nel primo gruppo.
    Pattern(String),
    /// **Il nome della prima chiave** dell'oggetto che sta a questo cammino.
    ///
    /// Esiste per un caso vero e per niente raro: certi motori non mettono il
    /// nome del modello in un campo, lo usano come *chiave* di un oggetto —
    /// `{"modelUsage": {"claude-opus-5[1m]": {...}}}`. Senza questa forma quel
    /// nome è inarrivabile, e senza il nome nessuna voce di listino si può
    /// trovare: il costo resterebbe sconosciuto anche con un listino perfetto.
    ///
    /// **Si prende la prima e basta.** Se un motore ne nomina più d'uno la
    /// chiamata ha attraversato più modelli, e questa forma non sa dire come
    /// spartire il consumo fra loro: prende la prima chiave nell'ordine in cui
    /// il motore l'ha scritta (`serde_json` conserva l'ordine) e non indovina il
    /// resto. Chi ha bisogno di quel caso dichiara un cammino esatto.
    FirstKey(Vec<String>),
}

/// Che cosa un descrittore dichiara di saper leggere dall'uscita del proprio
/// motore. Ogni puntatore è facoltativo: un motore che dice solo il totale
/// dichiara solo `total_tokens`, e gli altri restano sconosciuti.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Declared {
    pub read: Shape,
    pub input_tokens: Option<Pointer>,
    pub output_tokens: Option<Pointer>,
    /// I token d'ingresso **letti dalla cache**, che hanno un prezzo per
    /// milione tutto loro — spesso un ordine di grandezza sotto. Sommarli agli
    /// altri renderebbe la misura falsa proprio dove conta.
    pub cached_tokens: Option<Pointer>,
    /// I token d'ingresso **scritti nella cache**, che è tutt'altra cosa dal
    /// leggerla: la scrittura costa **più** dell'ingresso normale, la lettura
    /// molto meno.
    ///
    /// **PERCHÉ HA UN CAMPO SUO, MISURATO IL 30/08/2026.** Una chiamata a
    /// `claude -p` con due token d'ingresso e quattro d'uscita è costata 0,1285
    /// dollari dichiarati: 12.347 token di scrittura in cache erano il **96%**
    /// di quella cifra. Contarli come ingresso normale, o non contarli affatto,
    /// sbagliava il conto di 24 volte — sempre verso il basso, cioè sempre nella
    /// direzione che tranquillizza.
    pub cache_write_tokens: Option<Pointer>,
    /// I token scritti in una cache **a lunga durata**, che ha un prezzo suo,
    /// più alto. Chi non la distingue lascia questo campo vuoto e paga il
    /// prezzo di scrittura breve su tutto — sbagliando in modo dichiarato.
    pub cache_write_long_tokens: Option<Pointer>,
    pub total_tokens: Option<Pointer>,
    /// **QUANTI TURNI HA FATTO LA CHIAMATA**, cioè quante volte il modello è
    /// tornato a parlare dentro una sola invocazione.
    ///
    /// **PERCHÉ È UNA VOCE E NON UNA CURIOSITÀ.** Misurato il 31/08/2026: un
    /// flusso di quattro passi consuma 2,23 volte la cache letta di una sola
    /// sessione che fa lo stesso lavoro, e fa 2,07 volte i suoi turni — 62
    /// contro 30. Per turno legge **l'8% in più**, non il doppio. Il costo di
    /// una catena di passi non è quanto contesto porta ciascuno: è quante volte
    /// ciascuno ci ripassa sopra. Senza questo numero nel deposito, ogni
    /// proposta per far costare meno un flusso è una scommessa, perché la cosa
    /// che decide il conto non è misurata.
    pub turns: Option<Pointer>,
    /// Il costo che il motore dichiara di suo. Si registra come **confronto**,
    /// mai al posto del conto fatto sul listino locale.
    pub cost: Option<Pointer>,
    /// Dove il motore nomina il modello che ha davvero servito la chiamata. È
    /// l'unico legame onesto fra una riga di comando e una voce di listino.
    pub model: Option<Pointer>,
    /// Dove sta il testo della risposta dentro l'involucro.
    ///
    /// **SERVE PERCHÉ L'USCITA DEL PASSO NON DEVE CAMBIARE.** Chiedere a un
    /// motore `--output-format json` per farsi dire i token gli fa avvolgere
    /// anche la risposta: senza questo puntatore un passo a valle riceverebbe
    /// l'involucro invece del testo, e un flusso con `allow_extra: false` sulla
    /// propria forma diventerebbe rosso per una misura che non ha chiesto.
    pub answer: Option<Pointer>,
}

impl Declared {
    /// Vero se non dichiara nessun puntatore: c'è il blocco, ma non dice dove
    /// guardare niente.
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cached_tokens.is_none()
            && self.cache_write_tokens.is_none()
            && self.cache_write_long_tokens.is_none()
            && self.total_tokens.is_none()
            && self.cost.is_none()
            && self.model.is_none()
            && self.answer.is_none()
    }
}

/// Il consumo letto da una singola uscita, più ciò che serve a legarlo a un
/// listino e a lasciare intatta l'uscita del passo.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_write_long_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// Quanti turni ha fatto la chiamata, quando il motore lo dichiara.
    pub turns: Option<u64>,
    /// Il costo dichiarato dal motore, nella sua unità (di norma USD).
    pub declared_cost: Option<f64>,
    /// Il modello che il motore dice di aver usato.
    pub model: Option<String>,
    /// Il testo della risposta estratto dall'involucro, quando il descrittore
    /// dice dove sta.
    pub answer: Option<String>,
}

/// Legge il consumo dall'uscita di un motore secondo ciò che il suo descrittore
/// dichiara. Funzione pura: testo dentro, valori fuori, nessun nome di
/// fornitore.
///
/// Ogni campo che non si trova resta `None`. Non esiste nessun percorso, in
/// nessuna forma, che restituisca `0` per un valore mancante.
pub fn read_declared(said: &str, declared: &Declared) -> Reading {
    match declared.read {
        Shape::Json => read_from_json(said, declared),
        Shape::Text => read_from_text(said, declared),
    }
}

/// Un solo testo, letto dove il puntatore dice.
///
/// **LA FORMA LA DICE IL PUNTATORE, E NON SI CHIEDE DUE VOLTE.** Un cammino di
/// chiavi vale su un corpo JSON, un'espressione regolare sul testo: è la stessa
/// corrispondenza che `Pointer` dichiara di sé. Chiedere anche la forma
/// permetterebbe di rispondere in modo incoerente, e quella incoerenza non
/// darebbe un errore — darebbe un valore sconosciuto senza motivo visibile.
///
/// Serve a leggere ciò che un motore dichiara di sé **fuori dal consumo**: per
/// primo l'identificativo della sessione che ha appena aperto, che è l'unico
/// modo di riprenderla per i motori che lo coniano da sé.
pub fn read_text(said: &str, pointer: &Pointer) -> Option<String> {
    match pointer {
        Pointer::Pattern(pattern) => first_group(said, pattern),
        path => {
            let body = serde_json::from_str::<serde_json::Value>(said.trim()).ok()?;
            read_name(&body, Some(path))
        }
    }
}

fn read_from_json(said: &str, declared: &Declared) -> Reading {
    // Un involucro illeggibile non è un guasto: è un motore che ha risposto in
    // chiaro dove ci si aspettava JSON — quota finita, errore di rete, un
    // avvertimento sulla prima riga. Tutto resta sconosciuto, e la chiamata si
    // registra lo stesso.
    let Ok(body) = serde_json::from_str::<serde_json::Value>(said.trim()) else {
        return Reading::default();
    };
    let number = |pointer: &Option<Pointer>| walk(&body, pointer.as_ref()).and_then(as_tokens);
    Reading {
        input_tokens: number(&declared.input_tokens),
        output_tokens: number(&declared.output_tokens),
        cached_tokens: number(&declared.cached_tokens),
        cache_write_tokens: number(&declared.cache_write_tokens),
        cache_write_long_tokens: number(&declared.cache_write_long_tokens),
        total_tokens: number(&declared.total_tokens),
        turns: number(&declared.turns),
        declared_cost: walk(&body, declared.cost.as_ref()).and_then(as_money),
        model: read_name(&body, declared.model.as_ref()),
        answer: walk(&body, declared.answer.as_ref()).and_then(as_text),
    }
}

/// Un testo che può stare in un campo **o essere il nome di una chiave**.
fn read_name(body: &serde_json::Value, pointer: Option<&Pointer>) -> Option<String> {
    match pointer? {
        Pointer::FirstKey(keys) => {
            let mut here = body;
            for key in keys {
                here = here.get(key)?;
            }
            here.as_object()?.keys().next().cloned()
        }
        other => walk(body, Some(other)).and_then(as_text),
    }
}

fn read_from_text(said: &str, declared: &Declared) -> Reading {
    let capture = |pointer: &Option<Pointer>| match pointer.as_ref() {
        Some(Pointer::Pattern(pattern)) => first_group(said, pattern),
        // Un cammino di chiavi su un'uscita testuale: chi ha scritto il
        // descrittore ha sbagliato forma, e il valore resta sconosciuto.
        _ => None,
    };
    Reading {
        input_tokens: capture(&declared.input_tokens).and_then(|text| digits(&text)),
        output_tokens: capture(&declared.output_tokens).and_then(|text| digits(&text)),
        cached_tokens: capture(&declared.cached_tokens).and_then(|text| digits(&text)),
        cache_write_tokens: capture(&declared.cache_write_tokens).and_then(|text| digits(&text)),
        cache_write_long_tokens: capture(&declared.cache_write_long_tokens)
            .and_then(|text| digits(&text)),
        total_tokens: capture(&declared.total_tokens).and_then(|text| digits(&text)),
        turns: capture(&declared.turns).and_then(|text| digits(&text)),
        declared_cost: capture(&declared.cost).and_then(|text| text.trim().parse().ok()),
        model: capture(&declared.model),
        answer: capture(&declared.answer),
    }
}

/// Scende un cammino di chiavi. Un cammino vuoto vale il corpo intero: è il
/// modo di dire «la risposta è tutto quello che ha detto».
fn walk<'a>(body: &'a serde_json::Value, pointer: Option<&Pointer>) -> Option<&'a serde_json::Value> {
    let Pointer::Path(keys) = pointer? else {
        // Un'espressione regolare dichiarata su una lettura JSON: forma
        // sbagliata, valore sconosciuto.
        return None;
    };
    let mut here = body;
    for key in keys {
        here = here.get(key)?;
    }
    Some(here)
}

/// Un conteggio di token. Si accetta anche la stringa, perché un motore che
/// scrive numeri oltre 2^53 li manda come testo per non perderli.
fn as_tokens(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(digits))
}

fn as_money(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// Il testo di un valore. Un numero o un booleano **non** diventano testo qui:
/// un puntatore che finisce su un valore non testuale è un puntatore sbagliato,
/// e restituire la sua stampa nasconderebbe l'errore dentro una risposta
/// plausibile.
fn as_text(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Il primo gruppo di cattura, se l'espressione è valida e combacia.
///
/// Un'espressione scritta male non fa fallire niente: quel valore resta
/// sconosciuto, come se il motore non l'avesse detto. È la stessa regola di
/// tutto il resto — un descrittore sbagliato peggiora la misura, non rompe la
/// chiamata che stava misurando.
fn first_group(said: &str, pattern: &str) -> Option<String> {
    let regex = regex::Regex::new(pattern).ok()?;
    let captures = regex.captures(said)?;
    captures.get(1).map(|group| group.as_str().to_owned())
}

/// Un intero scritto con i separatori delle migliaia. `13.910` e `13,910` sono
/// tutti e due tredicimilanovecentodieci: quale separatore usi un motore
/// dipende dalla lingua della macchina su cui gira, e non è una cosa che chi
/// scrive un descrittore debba indovinare. Un testo che non è fatto di sole
/// cifre resta `None` — mai uno zero di ripiego.
fn digits(text: &str) -> Option<u64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, '.' | ',' | '_' | ' ' | '\u{202f}' | '\u{a0}'))
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    cleaned.parse().ok()
}

#[cfg(test)]
mod declared_tests {
    use super::*;

    fn path(keys: &[&str]) -> Option<Pointer> {
        Some(Pointer::Path(keys.iter().map(|k| (*k).to_owned()).collect()))
    }

    /// Un involucro nella forma che un motore a riga di comando produce quando
    /// gli si chiede `--output-format json`. **Non è misurato su nessun motore
    /// vero**: è la forma su cui si prova il lettore, e la sua fedeltà a un
    /// fornitore preciso non è ciò che questa prova afferma.
    const WRAPPED: &str = r#"{
      "result": "la risposta vera",
      "model": "claude-sonnet-5",
      "total_cost_usd": 0.0421,
      "usage": {
        "input_tokens": 1200,
        "output_tokens": 340,
        "cache_read_input_tokens": 98000
      }
    }"#;

    fn wrapped_declaration() -> Declared {
        Declared {
            read: Shape::Json,
            input_tokens: path(&["usage", "input_tokens"]),
            output_tokens: path(&["usage", "output_tokens"]),
            cached_tokens: path(&["usage", "cache_read_input_tokens"]),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost: path(&["total_cost_usd"]),
            model: path(&["model"]),
            answer: path(&["result"]),
        }
    }

    #[test]
    fn reads_every_declared_pointer_out_of_a_json_envelope() {
        let reading = read_declared(WRAPPED, &wrapped_declaration());
        assert_eq!(reading.input_tokens, Some(1200));
        assert_eq!(reading.output_tokens, Some(340));
        assert_eq!(reading.cached_tokens, Some(98_000));
        assert_eq!(reading.declared_cost, Some(0.0421));
        assert_eq!(reading.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(reading.answer.as_deref(), Some("la risposta vera"));
    }

    /// IL BRACCIO CHE CONTA per il criterio 3: la cache ha un puntatore suo, e
    /// non finisce sommata all'ingresso. Se il lettore le confondesse questi
    /// due numeri sarebbero uguali.
    #[test]
    fn cache_tokens_never_end_up_inside_the_input_count() {
        let reading = read_declared(WRAPPED, &wrapped_declaration());
        assert_eq!(reading.input_tokens, Some(1200));
        assert_ne!(reading.input_tokens, reading.cached_tokens);
    }

    #[test]
    fn a_pointer_that_finds_nothing_leaves_the_value_unknown_never_zero() {
        let declared = Declared {
            read: Shape::Json,
            input_tokens: path(&["usage", "non_esiste"]),
            output_tokens: path(&["nemmeno", "questo"]),
            ..Declared::default()
        };
        let reading = read_declared(WRAPPED, &declared);
        assert_eq!(reading.input_tokens, None);
        assert_eq!(reading.output_tokens, None);
    }

    /// Un motore che ha risposto in chiaro dove ci si aspettava JSON — quota
    /// finita, avvertimento sulla prima riga — non è un guasto del lettore.
    #[test]
    fn plain_text_where_json_was_expected_is_all_unknown_not_a_panic() {
        let reading = read_declared("You've hit your weekly limit", &wrapped_declaration());
        assert_eq!(reading, Reading::default());
    }

    #[test]
    fn a_number_written_as_a_string_is_still_a_count() {
        let declared = Declared {
            read: Shape::Json,
            input_tokens: path(&["n"]),
            ..Declared::default()
        };
        assert_eq!(
            read_declared(r#"{"n":"9007199254740993"}"#, &declared).input_tokens,
            Some(9_007_199_254_740_993)
        );
    }

    /// Un puntatore che finisce su un numero non diventa il testo di quel
    /// numero: restituirlo nasconderebbe un descrittore sbagliato dentro una
    /// risposta plausibile.
    #[test]
    fn a_pointer_landing_on_a_number_is_not_read_as_text() {
        let declared = Declared {
            read: Shape::Json,
            model: path(&["usage", "input_tokens"]),
            ..Declared::default()
        };
        assert_eq!(read_declared(WRAPPED, &declared).model, None);
    }

    // ── la forma testuale ────────────────────────────────────────────

    /// L'uscita di `codex exec` come è scritta nella prova di `parse_codex_tokens`
    /// qui sopra: il numero sta sulla riga dopo il marcatore, coi punti come
    /// separatori delle migliaia.
    const CODEX_LIKE: &str = "roba varia\ntokens used\n13.910\naltra roba";

    #[test]
    fn reads_a_count_from_plain_text_with_a_declared_pattern() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern(r"tokens used\s*\n\s*([\d.,]+)".to_owned())),
            ..Declared::default()
        };
        let reading = read_declared(CODEX_LIKE, &declared);
        assert_eq!(reading.total_tokens, Some(13_910));
        assert_eq!(reading.input_tokens, None, "codex non separa i due lati");
        assert_eq!(reading.output_tokens, None);
    }

    #[test]
    fn a_pattern_that_does_not_match_leaves_it_unknown() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern(r"tokens used\s*\n\s*([\d.,]+)".to_owned())),
            ..Declared::default()
        };
        assert_eq!(read_declared("nessuna riga utile", &declared).total_tokens, None);
    }

    /// Un descrittore con un'espressione regolare scritta male peggiora la
    /// misura, non rompe la chiamata che stava misurando.
    #[test]
    fn a_broken_pattern_is_unknown_not_a_panic() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern("([non chiusa".to_owned())),
            ..Declared::default()
        };
        assert_eq!(read_declared(CODEX_LIKE, &declared).total_tokens, None);
    }

    /// Le due forme non si mescolano: un cammino di chiavi su un'uscita
    /// testuale, o un'espressione su un corpo JSON, non trovano niente.
    #[test]
    fn a_pointer_of_the_wrong_shape_finds_nothing() {
        let on_text = Declared {
            read: Shape::Text,
            total_tokens: path(&["usage", "input_tokens"]),
            ..Declared::default()
        };
        assert_eq!(read_declared(WRAPPED, &on_text).total_tokens, None);

        // IL BRACCIO CHE CONTA: l'espressione è scritta in modo da combaciare
        // con una CHIAVE vera del corpo. Se il codice trattasse un'espressione
        // come se fosse un cammino di una chiave sola, qui uscirebbe `Some(7)` —
        // un numero preso dal posto sbagliato, che nessuno riconoscerebbe come
        // sbagliato guardandolo.
        let on_json = Declared {
            read: Shape::Json,
            total_tokens: Some(Pointer::Pattern("n".to_owned())),
            ..Declared::default()
        };
        assert_eq!(read_declared(r#"{"n":7}"#, &on_json).total_tokens, None);
        assert_eq!(read_declared(WRAPPED, &on_json).total_tokens, None);
    }

    #[test]
    fn thousands_separators_do_not_change_the_number() {
        assert_eq!(digits("13.910"), Some(13_910));
        assert_eq!(digits("13,910"), Some(13_910));
        assert_eq!(digits(" 13 910 "), Some(13_910));
        assert_eq!(digits("13910"), Some(13_910));
    }

    #[test]
    fn something_that_is_not_a_number_is_unknown_not_zero() {
        assert_eq!(digits("molti"), None);
        assert_eq!(digits(""), None);
        assert_eq!(digits("-5"), None);
        assert_eq!(digits("12a"), None);
    }

    #[test]
    fn a_declaration_with_no_pointers_at_all_is_empty() {
        assert!(Declared::default().is_empty());
        assert!(!wrapped_declaration().is_empty());
    }
}
