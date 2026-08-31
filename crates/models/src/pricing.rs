//! Il listino: quanto costa un milione di token, per modello.
//!
//! **È LA FONTE DI VERITÀ SUL COSTO.** Un costo che arriva dallo stesso posto da
//! cui arriva la spesa non è una verifica di niente: se un motore dichiara
//! quanto ha fatto pagare, quel numero si registra a parte come confronto e il
//! conto lo fa questo listino.
//!
//! **NE ESISTONO DUE, E IL SECONDO SOVRASCRIVE IL PRIMO.** Quello spedito col
//! prodotto è incorporato nel binario ([`BUILTIN`], [`shipped`]); quello di casa
//! — `$SAILOR_HOME/pricing.json` — si riscrive con un editor di testo e vince
//! per `id`, perché i prezzi cambiano più spesso di quanto si ricompili.
//!
//! **PERCHÉ NON BASTAVA QUELLO DI CASA, ED È IL GUASTO 35.** Fino al 31/08/2026
//! il listino esisteva **solo** lì, e in git c'era un esempio dichiarato non
//! verificato. Su una macchina dove quel file mancava ogni `cost_micros` restava
//! `None`, ogni corsa risultava costata zero, e un flusso con `spend_cap_micros`
//! girava fino in fondo senza nessun errore: un freno che non frena. La cura non
//! è ammorbidire il tetto — è spedire il listino, come si spediscono i
//! descrittori degli strumenti e i flussi di sistema.
//!
//! **È LA STESSA MALATTIA DEL GUASTO 18**, e vale la pena vederla una volta
//! sola: Sailor aveva il dato in casa propria e non lo usava. Il listino c'era e
//! non viaggiava col prodotto; la dotazione c'era e non arrivava ai motori.
//! Finché un dato vive solo nella casa di chi esegue, il prodotto sta usando la
//! casa del vicino.
//!
//! **MAI 0 PER UN PREZZO CHE MANCA.** È la stessa regola già scritta in
//! `catalog::Model`: `0.0` resta ai modelli davvero gratuiti. Una voce senza
//! prezzo, o un modello che nel listino non c'è, lascia il costo **sconosciuto**
//! — non a zero, che sarebbe una sottostima con la faccia di una misura.
//!
//! Puro: testo dentro, valori fuori. Chi legge il file da disco sta altrove,
//! come per `catalog` e `usage`.

/// Il listino che il prodotto si porta dietro.
///
/// Incorporato nel binario, non cercato in una cartella di installazione: è lo
/// stesso schema di `toolbox::descriptor::BUILTIN` e dei flussi di sistema, e
/// per la stessa ragione — un binario copiato altrove continua a rispondere, e
/// non c'è nessun percorso da indovinare. Resta comunque un dato: chi lo vuole
/// diverso riscrive la voce per `id` nel proprio file di casa.
pub const BUILTIN: &str = include_str!("../pricing.default.json");

/// Come si dice a chi legge da dove viene un listino che nessuno ha scritto su
/// questa macchina. Non è un percorso e non deve sembrarlo: chi andasse a
/// cercarlo su disco non troverebbe niente.
pub const BUILTIN_SOURCE: &str = "incorporato";

/// Il listino spedito col prodotto.
///
/// **PANICA SE NON SI LEGGE, DI PROPOSITO.** Un listino incorporato malformato
/// non è una condizione del mondo — è un difetto di compilazione, e cadere in
/// silenzio su un listino vuoto rimetterebbe in piedi esattamente il guasto 35.
/// La prova `the_shipped_price_list_is_readable` lo prende prima che esca di
/// qui.
pub fn shipped() -> PriceList {
    PriceList::parse(BUILTIN).expect("il listino spedito col prodotto si legge")
}

/// Che cosa questo listino sa dire del nome di un modello.
///
/// **TRE ESITI E NON DUE, PERCHÉ SONO DUE RIPARAZIONI DIVERSE.** Un nome che il
/// listino non conosce si ripara aggiungendo una voce — o un alias, se è lo
/// stesso modello con un altro nome. Una voce che c'è ma non ha i prezzi si
/// ripara scrivendo i prezzi. Chiamarli tutti e due «sconosciuto» manderebbe a
/// cercare nel posto sbagliato, e il costo resta sconosciuto in tutti e due i
/// casi — che è precisamente ciò che rende la differenza invisibile a chi
/// guarda solo il numero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// C'è, e ha i due prezzi che servono a fare un conto.
    Priced,
    /// C'è, ma le manca l'ingresso o l'uscita: il costo resterà sconosciuto lo
    /// stesso, e nessuno se ne accorgerebbe guardando il listino da lontano.
    ListedWithoutPrice,
    /// Nessuna voce porta questo nome, né come `id` né come alias.
    Absent,
}

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

    /// Che cosa si sa del costo di questo nome, prima di spendere.
    ///
    /// Serve a `sailor flow check` e a `sailor flow cost`: chi non ha un prezzo
    /// per un modello deve **saperlo**, non scoprirlo con uno zero. Vedi
    /// [`Known`].
    pub fn knows(&self, name: &str) -> Known {
        match self.find(name) {
            None => Known::Absent,
            Some(entry) => {
                if entry.input_per_million.is_some() && entry.output_per_million.is_some() {
                    Known::Priced
                } else {
                    Known::ListedWithoutPrice
                }
            }
        }
    }

    /// Questo listino, con sopra quello scritto in casa dall'utente.
    ///
    /// **SI SOSTITUISCE UNA VOCE INTERA, NON I SUOI CAMPI.** È la disciplina dei
    /// descrittori e dei flussi di sistema: un `id` già presente viene
    /// rimpiazzato da quello di casa, alias compresi. Fondere campo per campo
    /// lascerebbe in piedi un alias spedito che chi riscrive la voce credeva di
    /// aver tolto, e lo scoprirebbe solo trovando prezzato un nome che non
    /// aveva più dichiarato.
    ///
    /// **LE VOCI DI CASA VENGONO PRIMA, E L'ORDINE NON È ESTETICO.** [`find`]
    /// prende la prima che risponde: un alias di casa che collide con l'`id` di
    /// una voce spedita deve vincere, altrimenti l'utente avrebbe riscritto un
    /// nome e continuerebbe a essere servito dall'altro.
    ///
    /// **DUE VALUTE NON SI MESCOLANO, E QUESTO È IL BRACCIO CHE CONTA.** Se il
    /// file di casa dichiara una valuta diversa da quella spedita, le voci
    /// spedite **non entrano**: il listino di casa vale da solo. Sommare euro e
    /// dollari nella stessa colonna è precisamente ciò che questa struttura
    /// evita dichiarando la valuta una volta sola, e una fusione ingenua lo
    /// rifarebbe da dietro — senza che nessuno lo veda, perché il totale esce
    /// lo stesso.
    ///
    /// [`find`]: PriceList::find
    pub fn overridden_by(self, home: PriceList) -> PriceList {
        if !same_currency(&self.currency, &home.currency) {
            return home;
        }
        let taken: Vec<String> = home
            .entries
            .iter()
            .map(|entry| entry.id.trim().to_lowercase())
            .collect();
        let mut entries = home.entries;
        entries.extend(
            self.entries
                .into_iter()
                .filter(|entry| !taken.contains(&entry.id.trim().to_lowercase())),
        );
        PriceList {
            currency: home.currency,
            // La data è quella di chi ha scritto per ultimo: chi guarda una
            // cifra vuole sapere quanto è vecchio il listino che ha in mano, e
            // quello di casa è il solo che qualcuno abbia toccato.
            dated: home.dated.or(self.dated),
            entries,
        }
    }
}

/// Due valute sono la stessa se si scrivono uguali a meno di maiuscole e spazi:
/// la stessa tolleranza che [`PriceList::find`] applica ai nomi dei modelli.
fn same_currency(one: &str, other: &str) -> bool {
    one.trim().to_lowercase() == other.trim().to_lowercase()
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

    /// Un modello davvero gratuito ha `0.0` dichiarato, ed è diverso da un
    /// prezzo mancante: il suo costo si calcola e viene zero. La distinzione sta
    /// nel lettore, non nel file — per questo si prova su un listino scritto
    /// qui, e non su quello spedito, che non ha nessun modello gratuito e non
    /// deve guadagnarne uno finto per far passare una prova.
    #[test]
    fn a_declared_zero_is_a_measure_and_a_missing_price_is_not() {
        let prices = PriceList::parse(
            r#"{"models":[{"id":"gratis","input_per_million":0.0,"output_per_million":0.0}]}"#,
        )
        .unwrap();
        let a_million_each_side = TokenCounts {
            input: Some(1_000_000),
            output: Some(1_000_000),
            ..TokenCounts::default()
        };
        assert_eq!(
            cost_micros(a_million_each_side, prices.find("gratis").unwrap().micros()),
            Some(0),
            "zero dichiarato è una misura; zero inventato no"
        );
        assert_eq!(
            cost_micros(a_million_each_side, PriceList::default().find("gratis").map(Price::micros).unwrap_or_default()),
            None,
            "un modello che il listino non conosce non costa zero: non si sa"
        );
    }
}

#[cfg(test)]
mod the_shipped_price_list {
    use super::*;

    /// Il listino spedito col prodotto deve essere leggibile da questo codice.
    /// [`shipped`] panica se non lo è, quindi questa prova è il posto in cui quel
    /// panico viene alla luce prima di uscire dal repository.
    #[test]
    fn the_shipped_price_list_is_readable() {
        let prices = shipped();
        assert_eq!(prices.currency, "USD");
        assert!(
            prices.dated.is_some(),
            "un listino senza data non dice a chi lo legge quanto è vecchio"
        );
    }

    /// **LA PROVA DEL GUASTO 35, E VALE PIÙ DI TUTTE LE ALTRE QUI DENTRO.**
    ///
    /// Su una macchina appena installata non c'è nessun `~/.config/sailor/pricing.json`.
    /// Prima del 01/09/2026 quello era l'unico listino esistente: `cost_micros`
    /// restava `None` per ogni chiamata, ogni corsa risultava costata zero, e un
    /// flusso con un tetto di spesa girava fino in fondo senza che il tetto
    /// scattasse mai — in silenzio. Qui il listino spedito, e nient'altro, deve
    /// bastare a dare un prezzo al motore che si usa di più.
    ///
    /// **SVUOTA `models` in `pricing.default.json` e questa diventa rossa**: è
    /// il difetto originale, rimesso esattamente dov'era.
    #[test]
    fn the_shipped_list_alone_prices_the_engine_used_most() {
        let prices = shipped();
        let entry = prices
            .find("claude-opus-5")
            .expect("il listino spedito conosce il modello con cui Sailor lavora");
        let a_thousand_each_side = TokenCounts {
            input: Some(1_000),
            output: Some(1_000),
            cached: Some(1_000),
            ..TokenCounts::default()
        };
        assert!(
            cost_micros(a_thousand_each_side, entry.micros()).is_some(),
            "senza il listino spedito il costo di ogni chiamata resta sconosciuto"
        );
    }

    /// **IL BRACCIO CHE LEGA IL LISTINO SPEDITO A UNA MISURA VERA.**
    ///
    /// Il 30/08/2026 una chiamata a `claude -p "rispondi solo: ok"` ha dichiarato
    /// 0,128541 dollari con **2** token d'ingresso e **4** d'uscita: il resto
    /// erano 9.922 token letti dalla cache e **12.347 scritti** in una cache a
    /// lunga durata. Il listino spedito deve rifare quel conto alla micro-unità,
    /// partendo dai soli conteggi. Se una delle cinque voci di prezzo fosse
    /// sbagliata, il numero uscirebbe lo stesso — verosimile e falso — e questa è
    /// l'unica prova che lo vede.
    ///
    /// Il nome cercato è quello che il motore **dichiara di suo**,
    /// `claude-opus-5[1m]`: se il listino spedito perdesse quell'alias, il costo
    /// tornerebbe sconosciuto senza che nessun prezzo sia cambiato.
    #[test]
    fn the_shipped_list_reproduces_a_real_call_to_the_micro_unit() {
        let prices = shipped();
        let entry = prices
            .find("claude-opus-5[1m]")
            .expect("il listino spedito conosce il nome che il motore dichiara");
        let measured = TokenCounts {
            input: Some(2),
            output: Some(4),
            cached: Some(9_922),
            cache_write: None,
            cache_write_long: Some(12_347),
        };
        assert_eq!(
            cost_micros(measured, entry.micros()),
            Some(128_541),
            "il conto sul listino spedito e quello del motore devono coincidere"
        );
    }

    /// Ogni voce spedita ha i due prezzi che servono a fare un conto. Una voce a
    /// metà nel listino di casa è una scelta di chi lo scrive; nel listino
    /// spedito sarebbe un costo sconosciuto che nessuno ha deciso, e si
    /// scoprirebbe solo guardando una spesa che non torna.
    #[test]
    fn every_shipped_entry_can_actually_price_a_call() {
        for entry in &shipped().entries {
            assert_eq!(
                shipped().knows(&entry.id),
                Known::Priced,
                "la voce spedita «{}» non ha i prezzi per fare un conto",
                entry.id
            );
        }
    }

    /// I nomi non si ripetono, né fra gli `id` né fra gli alias. [`PriceList::find`]
    /// prende la prima voce che risponde: un nome dichiarato due volte farebbe
    /// pagare a un modello il prezzo di un altro, e la riga nel deposito
    /// sembrerebbe giusta.
    #[test]
    fn no_shipped_name_is_declared_twice() {
        let mut seen: Vec<String> = Vec::new();
        for entry in &shipped().entries {
            for name in std::iter::once(&entry.id).chain(entry.aliases.iter()) {
                let name = name.trim().to_lowercase();
                assert!(!seen.contains(&name), "il nome «{name}» è dichiarato due volte");
                seen.push(name);
            }
        }
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

/// Il listino di casa sopra quello spedito, con la disciplina dei descrittori.
#[cfg(test)]
mod the_home_list_wins {
    use super::*;

    const SHIPPED: &str = r#"{
      "currency": "USD",
      "dated": "2026-01-01",
      "models": [
        { "id": "uno", "aliases": ["primo"], "input_per_million": 5.0, "output_per_million": 25.0 },
        { "id": "due", "input_per_million": 1.0, "output_per_million": 2.0 }
      ]
    }"#;

    fn shipped_sample() -> PriceList {
        PriceList::parse(SHIPPED).unwrap()
    }

    /// Quello che c'è solo nel listino spedito continua a rispondere: il file di
    /// casa aggiunge e corregge, non azzera.
    #[test]
    fn what_only_the_shipped_list_declares_survives_the_override() {
        let home = PriceList::parse(r#"{"currency":"USD","models":[{"id":"uno","input_per_million":9.0,"output_per_million":9.0}]}"#).unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("due").unwrap().input_per_million, Some(1.0));
    }

    /// **UNA VOCE SI SOSTITUISCE INTERA, ALIAS COMPRESI.** Il prezzo di casa
    /// vince, e l'alias che il listino spedito dichiarava sparisce con la voce
    /// che lo portava: chi riscrive `uno` senza `primo` ha detto che quel nome
    /// non è più suo.
    #[test]
    fn an_id_already_there_is_replaced_whole_not_merged_field_by_field() {
        let home = PriceList::parse(
            r#"{"currency":"USD","models":[{"id":"uno","input_per_million":9.0,"output_per_million":9.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("uno").unwrap().input_per_million, Some(9.0));
        assert_eq!(
            merged.find("primo"),
            None,
            "l'alias della voce sostituita è sparito con lei"
        );
    }

    /// Un alias di casa che collide con l'`id` di una voce spedita deve vincere:
    /// [`PriceList::find`] prende la prima che risponde, e le voci di casa
    /// stanno davanti apposta.
    #[test]
    fn a_home_alias_beats_a_shipped_id_with_the_same_name() {
        let home = PriceList::parse(
            r#"{"currency":"USD","models":[{"id":"mio","aliases":["due"],"input_per_million":7.0,"output_per_million":7.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("due").unwrap().id, "mio");
    }

    /// **IL BRACCIO CHE CONTA: DUE VALUTE NON SI MESCOLANO.** Un listino di casa
    /// in euro non deve tirarsi dietro le voci spedite in dollari, perché
    /// finirebbero sommate nella stessa colonna e il totale uscirebbe lo stesso —
    /// verosimile e senza senso. Togli il controllo sulla valuta in
    /// `overridden_by` e `due` torna a rispondere: è precisamente il difetto.
    #[test]
    fn a_home_list_in_another_currency_stands_alone() {
        let home = PriceList::parse(
            r#"{"currency":"EUR","models":[{"id":"uno","input_per_million":4.0,"output_per_million":4.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.currency, "EUR");
        assert_eq!(merged.find("uno").unwrap().input_per_million, Some(4.0));
        assert_eq!(
            merged.find("due"),
            None,
            "una voce in dollari è entrata in un listino in euro"
        );
    }

    /// La stessa valuta scritta in un altro modo resta la stessa valuta: la
    /// tolleranza è quella che `find` applica già ai nomi dei modelli.
    #[test]
    fn the_same_currency_written_differently_still_merges() {
        let home =
            PriceList::parse(r#"{"currency":" usd ","models":[{"id":"tre"}]}"#).unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert!(merged.find("due").is_some());
    }

    /// La data è di chi ha scritto per ultimo, e non sparisce se il file di casa
    /// non ne dichiara una: chi guarda una cifra vuole sapere di quando sono i
    /// prezzi con cui è stata calcolata.
    #[test]
    fn the_date_comes_from_whoever_wrote_last_and_never_vanishes() {
        let dated = PriceList::parse(r#"{"currency":"USD","dated":"2026-09-01","models":[]}"#).unwrap();
        assert_eq!(
            shipped_sample().overridden_by(dated).dated.as_deref(),
            Some("2026-09-01")
        );
        let undated = PriceList::parse(r#"{"currency":"USD","models":[]}"#).unwrap();
        assert_eq!(
            shipped_sample().overridden_by(undated).dated.as_deref(),
            Some("2026-01-01")
        );
    }
}

/// Chi non ha un prezzo per un modello deve **saperlo**, non scoprirlo con uno
/// zero: le tre risposte di [`PriceList::knows`].
#[cfg(test)]
mod knowing_what_is_not_priced {
    use super::*;

    const LIST: &str = r#"{
      "currency": "USD",
      "models": [
        { "id": "intero", "aliases": ["scorciatoia"], "input_per_million": 5.0, "output_per_million": 25.0 },
        { "id": "a-meta", "input_per_million": 5.0 }
      ]
    }"#;

    #[test]
    fn a_fully_priced_entry_is_priced_by_its_id_and_by_its_alias() {
        let prices = PriceList::parse(LIST).unwrap();
        assert_eq!(prices.knows("intero"), Known::Priced);
        assert_eq!(prices.knows("scorciatoia"), Known::Priced);
    }

    /// **I DUE MODI DI RESTARE SENZA PREZZO NON SI CONFONDONO**, perché sono due
    /// riparazioni diverse: uno si ripara aggiungendo una voce, l'altro
    /// scrivendo i prezzi in quella che c'è già. Fai rispondere `Absent` anche al
    /// secondo caso e chi legge va a riscrivere una voce che esiste.
    #[test]
    fn an_entry_without_prices_is_not_the_same_as_a_name_nobody_declared() {
        let prices = PriceList::parse(LIST).unwrap();
        assert_eq!(prices.knows("a-meta"), Known::ListedWithoutPrice);
        assert_eq!(prices.knows("mai-visto"), Known::Absent);
        // E il costo resta sconosciuto in tutti e due i casi: è per questo che
        // la differenza non si vede guardando il numero.
        let counts = TokenCounts {
            input: Some(10),
            output: Some(10),
            ..TokenCounts::default()
        };
        assert_eq!(
            cost_micros(counts, prices.find("a-meta").unwrap().micros()),
            None
        );
    }
}
