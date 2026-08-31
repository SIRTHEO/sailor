//! Il prices locale: quanto costa un milione di token, per modello.
//!
//! **È LA FONTE DI VERITÀ SUL COSTO, E STA IN UN FILE.** Un costo che arriva
//! dallo stesso posto da cui arriva la spesa non è una verifica di niente: se
//! un motore dichiara quanto ha fatto pagare, quel numero si registra a parte
//! come confronto e il conto lo fa questo prices. E sta in un file perché i
//! prezzi cambiano più spesso di quanto si ricompili: `$SAILOR_HOME/pricing.json`
//! si riscrive con un editor di testo.
//!
//! **MAI 0 PER UN PREZZO CHE MANCA.** È la stessa regola già scritta in
//! `catalog::Model`: `0.0` resta ai modelli davvero gratuiti. Una voce senza
//! prezzo, o un modello che nel prices non c'è, lascia il costo **sconosciuto**
//! — non a zero, che sarebbe una sottostima con la faccia di una misura.
//!
//! Puro: testo dentro, valori fuori. Chi legge il file da disco sta altrove,
//! come per `catalog` e `usage`.

/// Quanto costa un modello, per milione di token. Ogni prezzo è facoltativo:
/// un prices può conoscere l'ingresso e non la cache, ed è un'informazione
/// diversa da «la cache è gratis».
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Price {
    pub id: String,
    /// Gli altri nomi con cui lo stesso modello si presenta. Un motore a riga
    /// di comando nomina il modello come gli pare — `sonnet`, `claude-sonnet-5`,
    /// `anthropic/claude-sonnet-5` — e chi tiene il prices sa quali di questi
    /// nomi sono la stessa cosa. Il codice non lo indovina.
    pub aliases: Vec<String>,
    pub input_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
    /// Quanto costa **leggere** dalla cache. Di norma una frazione
    /// dell'ingresso.
    pub cached_per_million: Option<f64>,
    /// Quanto costa **scrivere** nella cache. Di norma **più** dell'ingresso, ed
    /// è la voce che sorprende: su una chiamata misurata il 30/08/2026 era il
    /// 96% della spesa, con due token d'ingresso e quattro d'uscita.
    pub cache_write_per_million: Option<f64>,
    /// Quanto costa scrivere in una cache **a lunga durata**, dove il fornitore
    /// ne offre più d'una. Assente vuol dire «non so», e allora quei token
    /// restano non prezzati invece di essere contati al prezzo breve.
    pub cache_write_long_per_million: Option<f64>,
}

/// Il prices intero, con la valuta dichiarata una volta sola.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceList {
    /// La valuta di tutti i prezzi. Dichiararla per voce permetterebbe di
    /// sommare euro e dollari nella stessa colonna senza che nessuno se ne
    /// accorga.
    pub currency: String,
    /// Di quando sono i prezzi, come l'ha scritto chi tiene il file. Non entra
    /// in nessun conto: serve a chi guarda una cifra e vuole sapere se il
    /// prices con cui è stata calcolata è di ieri o dell'anno scorso.
    pub dated: Option<String>,
    pub entries: Vec<Price>,
}

impl Default for PriceList {
    fn default() -> Self {
        PriceList {
            currency: "USD".to_owned(),
            dated: None,
            entries: Vec::new(),
        }
    }
}

impl PriceList {
    /// Legge il file. Una voce malformata si scarta da sola, come nel catalogo:
    /// un prezzo scritto male non deve togliere il prices a tutti gli altri.
    pub fn parse(text: &str) -> Result<PriceList, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(text).map_err(|error| format!("il prices non è JSON: {error}"))?;
        let currency = parsed
            .get("currency")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("USD")
            .to_owned();
        let dated = parsed
            .get("dated")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let entries = parsed
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(parse_price).collect())
            .unwrap_or_default();
        Ok(PriceList {
            currency,
            dated,
            entries,
        })
    }

    /// La voce che porta questo nome, per `id` o per alias. Il confronto ignora
    /// maiuscole e minuscole e gli spazi ai bordi: nessun fornitore promette di
    /// non cambiarle, ed è la stessa tolleranza che `unusable_when` applica già.
    ///
    /// Non c'è nessuna somiglianza approssimata, nessun prefisso, nessun «il
    /// più probabile». Un nome che il prices non conosce lascia il costo
    /// sconosciuto: indovinare qui vorrebbe dire far pagare a un modello il
    /// prezzo di un altro, e nessuno potrebbe accorgersene guardando la riga.
    pub fn find(&self, name: &str) -> Option<&Price> {
        let wanted = name.trim().to_lowercase();
        if wanted.is_empty() {
            return None;
        }
        self.entries.iter().find(|price| {
            price.id.trim().to_lowercase() == wanted
                || price
                    .aliases
                    .iter()
                    .any(|alias| alias.trim().to_lowercase() == wanted)
        })
    }
}

fn parse_price(value: &serde_json::Value) -> Option<Price> {
    let id = value.get("id")?.as_str()?.to_owned();
    if id.trim().is_empty() {
        return None;
    }
    let aliases = value
        .get("aliases")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(Price {
        id,
        aliases,
        input_per_million: money(value.get("input_per_million")),
        output_per_million: money(value.get("output_per_million")),
        cached_per_million: money(value.get("cached_per_million")),
        cache_write_per_million: money(value.get("cache_write_per_million")),
        cache_write_long_per_million: money(value.get("cache_write_long_per_million")),
    })
}

/// Un prezzo. Si accetta il numero e la stringa — un prices scritto a mano
/// finisce spesso con `"3.00"` — ma non un valore di altro tipo, e non un
/// numero negativo: un prezzo sotto zero è un errore di battitura, e sommarlo
/// farebbe scendere una spesa.
fn money(value: Option<&serde_json::Value>) -> Option<f64> {
    let value = value?;
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))?;
    if number.is_finite() && number >= 0.0 {
        Some(number)
    } else {
        None
    }
}

/// Un prezzo per milione espresso in **micro-unità di valuta**, come lo vuole
/// il deposito. `3.0` dollari per milione diventa `3_000_000`.
pub fn micros_per_million(price_per_million: f64) -> i64 {
    (price_per_million * 1_000_000.0).round() as i64
}

/// I prezzi di una voce, in micro-unità, pronti per la riga del deposito.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PriceMicros {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cached: Option<i64>,
    pub cache_write: Option<i64>,
    pub cache_write_long: Option<i64>,
}

impl Price {
    pub fn micros(&self) -> PriceMicros {
        PriceMicros {
            input: self.input_per_million.map(micros_per_million),
            output: self.output_per_million.map(micros_per_million),
            cached: self.cached_per_million.map(micros_per_million),
            cache_write: self.cache_write_per_million.map(micros_per_million),
            cache_write_long: self.cache_write_long_per_million.map(micros_per_million),
        }
    }
}

/// I conteggi di una chiamata, ciascuno al proprio nome.
///
/// **STANNO IN UNA STRUTTURA E NON IN CINQUE ARGOMENTI.** Sono tutti
/// `Option<u64>`: in fila su una firma, due scambiati per errore compilano
/// benissimo e sbagliano il conto per sempre, in silenzio. Con un nome per
/// campo lo scambio non si scrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenCounts {
    pub input: Option<u64>,
    pub output: Option<u64>,
    /// Letti dalla cache.
    pub cached: Option<u64>,
    /// Scritti nella cache, durata breve.
    pub cache_write: Option<u64>,
    /// Scritti nella cache, durata lunga.
    pub cache_write_long: Option<u64>,
}

/// Il costo di una chiamata, in micro-unità di valuta, **in aritmetica intera**.
///
/// **PERCHÉ INTERA E NON IN VIRGOLA MOBILE.** Il costo finisce in una colonna
/// `INTEGER`, e un arrotondamento fatto in un punto diverso a ogni passaggio
/// farebbe venire due somme diverse dello stesso insieme di righe — cioè
/// esattamente il tipo di incoerenza per cui un verificatore respinge un
/// lavoro. Qui si arrotonda una volta sola, alla fine, alla micro-unità più
/// vicina.
///
/// **QUANDO IL COSTO RESTA SCONOSCIUTO.** Se un conteggio è noto ma il suo
/// prezzo no, il totale sarebbe una sottostima silenziosa: si restituisce
/// `None`. Se non si conosce né l'ingresso né l'uscita non c'è niente da
/// contare, e vale lo stesso. Un conteggio sconosciuto **non** vale zero: chi
/// legge deve poter distinguere «non ha usato la cache» da «non so se l'ha
/// usata», e questa funzione non decide per lui.
pub fn cost_micros(counts: TokenCounts, prices: PriceMicros) -> Option<i64> {
    // Serve almeno un lato misurato: un costo calcolato su nessun token è zero,
    // e uno zero qui sarebbe indistinguibile da una chiamata gratuita.
    if counts.input.is_none() && counts.output.is_none() {
        return None;
    }
    let mut total: i128 = 0;
    for (tokens, price) in [
        (counts.input, prices.input),
        (counts.output, prices.output),
        (counts.cached, prices.cached),
        (counts.cache_write, prices.cache_write),
        (counts.cache_write_long, prices.cache_write_long),
    ] {
        let Some(tokens) = tokens else { continue };
        // Il conteggio c'è e il prezzo no: ignorarlo abbasserebbe il totale di
        // una quantità che nessuno vedrebbe mancare.
        let price = price?;
        total += i128::from(tokens) * i128::from(price);
    }
    // Arrotondamento alla micro-unità: mezzo verso l'alto, una volta sola.
    let rounded = (total + 500_000) / 1_000_000;
    i64::try_from(rounded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        {
          "id": "claude-sonnet-5",
          "aliases": ["sonnet", "anthropic/claude-sonnet-5"],
          "input_per_million": 3.0,
          "output_per_million": 15.0,
          "cached_per_million": 0.3
        },
        { "id": "senza-prezzi" },
        { "id": "solo-ingresso", "input_per_million": "1.5" }
      ]
    }"#;

    #[test]
    fn reads_the_currency_the_date_and_every_entry() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(prices.currency, "USD");
        assert_eq!(prices.dated.as_deref(), Some("2026-08-29"));
        assert_eq!(prices.entries.len(), 3);
    }

    #[test]
    fn finds_a_model_by_its_id_and_by_every_alias() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(prices.find("claude-sonnet-5").unwrap().id, "claude-sonnet-5");
        assert_eq!(prices.find("sonnet").unwrap().id, "claude-sonnet-5");
        assert_eq!(
            prices.find("  ANTHROPIC/Claude-Sonnet-5 ").unwrap().id,
            "claude-sonnet-5"
        );
    }

    /// IL BRACCIO CHE CONTA: un nome che somiglia non è lo stesso nome. Se qui
    /// passasse un prefisso, un modello pagherebbe il prezzo di un altro senza
    /// che nessuno lo veda guardando la riga.
    #[test]
    fn a_name_that_merely_resembles_one_is_not_found() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert!(prices.find("claude-sonnet").is_none());
        assert!(prices.find("claude-sonnet-5-20260101").is_none());
        assert!(prices.find("").is_none());
    }

    #[test]
    fn a_missing_price_is_none_never_zero() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let entry = prices.find("senza-prezzi").unwrap();
        assert_eq!(entry.input_per_million, None);
        assert_eq!(entry.output_per_million, None);
        assert_eq!(entry.cached_per_million, None);
    }

    #[test]
    fn a_price_written_as_text_is_read_as_a_number() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(
            prices.find("solo-ingresso").unwrap().input_per_million,
            Some(1.5)
        );
    }

    #[test]
    fn a_negative_price_is_refused_not_summed() {
        let prices = PriceList::parse(r#"{"models":[{"id":"x","input_per_million":-2.0}]}"#).unwrap();
        assert_eq!(prices.find("x").unwrap().input_per_million, None);
    }

    /// I conteggi che quasi tutte queste prove usano. La cache **scritta** ha
    /// prove sue più sotto, perché è la voce che si dimenticava.
    fn counts(input: Option<u64>, output: Option<u64>, cached: Option<u64>) -> TokenCounts {
        TokenCounts {
            input,
            output,
            cached,
            ..TokenCounts::default()
        }
    }

    #[test]
    fn computes_the_cost_in_integer_micros_with_the_cache_priced_apart() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        // 1M ingresso a 3 $ + 1M uscita a 15 $ + 1M cache a 0,30 $ = 18,30 $
        let cost = cost_micros(
            counts(Some(1_000_000), Some(1_000_000), Some(1_000_000)),
            prices,
        );
        assert_eq!(cost, Some(18_300_000));
    }

    /// IL BRACCIO CHE CONTA per il criterio 3 del mandato: se la cache fosse
    /// contata al prezzo dell'ingresso, questo numero sarebbe dieci volte più
    /// grande. Un solo numero d'ingresso nasconderebbe proprio questa
    /// differenza.
    #[test]
    fn cached_tokens_are_priced_at_their_own_rate_not_at_the_input_rate() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        let with_cache = cost_micros(counts(Some(0), Some(0), Some(1_000_000)), prices).unwrap();
        let as_if_input = cost_micros(counts(Some(1_000_000), Some(0), None), prices).unwrap();
        assert_eq!(with_cache, 300_000, "1M di cache a 0,30 $");
        assert_eq!(as_if_input, 3_000_000, "1M d'ingresso a 3 $");
        assert!(with_cache * 5 < as_if_input, "la differenza è di un ordine di grandezza");
    }

    #[test]
    fn a_known_count_without_its_price_leaves_the_cost_unknown() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("solo-ingresso").unwrap().micros();
        assert_eq!(
            cost_micros(counts(Some(100), Some(100), None), prices),
            None,
            "l'uscita è misurata ma non ha prezzo: il totale sarebbe una sottostima"
        );
    }

    #[test]
    fn an_unknown_cached_count_is_not_treated_as_zero_cache() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        // Non sapere quanta cache ha letto non impedisce di contare il resto,
        // ma non aggiunge nulla: il costo è quello dei due lati noti.
        assert_eq!(
            cost_micros(counts(Some(1_000_000), Some(1_000_000), None), prices),
            Some(18_000_000)
        );
    }

    #[test]
    fn without_any_token_count_there_is_no_cost_not_a_zero() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        assert_eq!(cost_micros(counts(None, None, Some(10)), prices), None);
        assert_eq!(cost_micros(counts(None, None, None), prices), None);
    }

    #[test]
    fn rounds_to_the_nearest_micro_unit_once() {
        // 1 token a 1,50 $ per milione = 1,5 micro-unità → 2, non 1 né 1,5.
        let prices = PriceMicros {
            input: Some(micros_per_million(1.5)),
            output: Some(0),
            ..PriceMicros::default()
        };
        assert_eq!(cost_micros(counts(Some(1), Some(0), None), prices), Some(2));
    }

    #[test]
    fn a_price_list_that_is_not_json_is_an_error_not_an_empty_list() {
        assert!(PriceList::parse("non è json").is_err());
    }

    #[test]
    fn a_malformed_entry_is_dropped_without_taking_the_others_with_it() {
        let prices =
            PriceList::parse(r#"{"models":[{"nessun_id":true},{"id":"buono"}]}"#).unwrap();
        assert_eq!(prices.entries.len(), 1);
        assert!(prices.find("buono").is_some());
    }
}

#[cfg(test)]
mod the_shipped_example {
    use super::*;

    /// L'esempio spedito col prodotto deve essere leggibile da questo codice.
    /// Un esempio che il programma stesso rifiuterebbe manderebbe chi lo copia
    /// a cercare l'errore nel posto sbagliato — e lo troverebbe solo scoprendo
    /// che il costo resta sempre sconosciuto, senza mai sapere perché.
    #[test]
    fn the_shipped_example_parses_and_prices_what_it_declares() {
        let prices = PriceList::parse(include_str!("../pricing.example.json"))
            .expect("l'esempio si legge");
        assert_eq!(prices.currency, "USD");
        let sonnet = prices.find("sonnet").expect("l'alias funziona");
        assert_eq!(sonnet.id, "claude-sonnet-5");
        let a_thousand_each_side = TokenCounts {
            input: Some(1_000),
            output: Some(1_000),
            cached: Some(1_000),
            ..TokenCounts::default()
        };
        assert!(cost_micros(a_thousand_each_side, sonnet.micros()).is_some());
        // Un modello davvero gratuito ha 0.0 dichiarato, ed è diverso da un
        // prezzo mancante: il suo costo si calcola e viene zero.
        let free = prices.find("un-modello-gratuito").unwrap();
        assert_eq!(
            cost_micros(
                TokenCounts {
                    input: Some(1_000_000),
                    output: Some(1_000_000),
                    ..TokenCounts::default()
                },
                free.micros()
            ),
            Some(0),
            "zero dichiarato è una misura; zero inventato no"
        );
    }

    /// **IL BRACCIO CHE VALE PIÙ DI TUTTI, e viene da una misura vera.**
    ///
    /// Il 30/08/2026 una chiamata a `claude -p "rispondi solo: ok"` ha
    /// dichiarato 0,128541 dollari con **2** token d'ingresso e **4** d'uscita.
    /// Il resto erano 9.922 token letti dalla cache e **12.347 scritti** in una
    /// cache a lunga durata. Questa prova rifà quel conto: se la scrittura in
    /// cache non entra nel calcolo, il costo scende a un quarantesimo, e
    /// nessuno se ne accorge perché il numero c'è ed è verosimile.
    #[test]
    fn writing_the_cache_is_what_that_call_actually_cost() {
        let opus = PriceList::parse(
            r#"{"currency":"USD","models":[{
                 "id":"claude-opus-5",
                 "input_per_million":5.0,
                 "output_per_million":25.0,
                 "cached_per_million":0.5,
                 "cache_write_long_per_million":10.0
               }]}"#,
        )
        .unwrap();
        let prices = opus.find("claude-opus-5").unwrap().micros();
        let measured = TokenCounts {
            input: Some(2),
            output: Some(4),
            cached: Some(9_922),
            cache_write: None,
            cache_write_long: Some(12_347),
        };

        let cost = cost_micros(measured, prices).expect("il conto si fa");
        // Il motore aveva dichiarato 0,128541 $: qui vengono 128.541
        // micro-unità, cioè la stessa cifra al micro.
        assert_eq!(cost, 128_541, "il conto nostro combacia con quello del motore");

        let without_the_write = cost_micros(
            TokenCounts {
                cache_write_long: None,
                ..measured
            },
            prices,
        )
        .unwrap();
        // 2 token d'ingresso + 4 d'uscita + 9.922 letti dalla cache = 5.071
        // micro-unità, cioè mezzo centesimo invece di tredici.
        assert_eq!(without_the_write, 5_071);
        assert!(
            cost > without_the_write * 25,
            "dimenticare la cache scritta sbaglia di oltre venticinque volte"
        );
    }

    /// Un conteggio di cache scritta senza il suo prezzo lascia il costo
    /// **sconosciuto**, invece di contare quei token come gratis. È la stessa
    /// regola degli altri lati, e vale la pena provarla sul lato nuovo: è quello
    /// dove un listino vecchio non ha ancora la voce.
    #[test]
    fn a_cache_write_without_its_price_leaves_the_cost_unknown() {
        let without_write_price = PriceList::parse(
            r#"{"models":[{"id":"x","input_per_million":5.0,"output_per_million":25.0}]}"#,
        )
        .unwrap();
        let prices = without_write_price.find("x").unwrap().micros();
        assert_eq!(
            cost_micros(
                TokenCounts {
                    input: Some(10),
                    output: Some(10),
                    cache_write: Some(10_000),
                    ..TokenCounts::default()
                },
                prices
            ),
            None,
            "10.000 token scritti senza prezzo: il totale sarebbe una sottostima muta"
        );
    }
}
