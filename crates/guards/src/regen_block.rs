//! Il blocco che si riscrive da capo a ogni passata, dentro un documento che
//! qualcun altro ha scritto a mano.
//!
//! DA DOVE VIENE. Questa non è una cosa nuova: è la parte comune tirata fuori
//! da `queue_overlap`, che il 24/08/2026 era l'unico posto della casa dove un
//! rilievo si toglieva e si rimetteva dentro un markdown. Quando è servito lo
//! stesso gesto sulle consegne — stesso taglio, stessi a capo, posizione
//! diversa — si è estratto invece di ricopiare: due copie di questa
//! trasformazione divergono alla prima correzione, e la seconda copia se ne
//! accorge mesi dopo.
//!
//! IL DIFETTO CHE RISOLVE. Un rilievo su un documento datato — «questo è
//! vecchio», «un altro dice il contrario» — non può diventare un documento
//! nuovo: ne nascerebbe uno per ogni passata, e nessuno li leggerebbe più. Va
//! **dentro** il documento di cui parla, dove lo trova solo chi quel documento
//! lo apre davvero. Ma allora deve poter sparire e rinascere senza lasciare
//! sedimento, altrimenti il file cresce di un avviso a ogni giro e gli avvisi
//! di ieri restano a mentire accanto a quelli di oggi.
//!
//! Da qui le due proprietà che questo modulo garantisce, e che i suoi casi
//! provano:
//! - **si toglie senza traccia**: `strip` riporta il testo com'era, a capo
//!   compresi;
//! - **due passate danno gli stessi byte**: chi rende il blocco toglie prima
//!   quello di ieri, quindi non si accoda a sé stesso.
//!
//! NON DECIDE DOVE VA. La posizione la sceglie chi lo usa, ed è una scelta di
//! merito che cambia da un corpus all'altro: nelle voci di coda il blocco sta
//! **sotto il frontmatter**, perché il selettore della coda legge `stato:` e
//! infilargli davanti del testo lo spegnerebbe; nelle consegne sta **sopra la
//! sezione operativa**, perché è lì che va l'occhio di chi riprende un lavoro.
//! Qui ci sono solo i delimitatori, il taglio e la resa.

/// Un blocco rigenerabile, riconosciuto dai suoi delimitatori.
///
/// L'etichetta entra nei commenti HTML — `<!-- freschezza-coda: inizio -->` — ed
/// è la sola cosa che distingue due blocchi diversi nello stesso file: due
/// meccanismi che marcano lo stesso documento con la stessa etichetta si
/// cancellerebbero a vicenda senza dare errore.
pub struct RegenBlock {
    tag: &'static str,
}

impl RegenBlock {
    pub const fn new(tag: &'static str) -> Self {
        Self { tag }
    }

    pub fn open(&self) -> String {
        format!("<!-- {}: inizio -->", self.tag)
    }

    pub fn close(&self) -> String {
        format!("<!-- {}: fine -->", self.tag)
    }

    /// Il testo senza il blocco, delimitatori compresi.
    ///
    /// Senza memoria di proposito: chi scrive il rilievo toglie prima quello di
    /// ieri, così una passata che non ha più niente da dire lascia il file
    /// pulito invece di lasciarci un avviso scaduto.
    pub fn strip(&self, text: &str) -> String {
        let open = self.open();
        let close = self.close();
        let Some(start) = text.find(&open) else {
            return text.to_string();
        };
        let Some(end_offset) = text[start..].find(&close) else {
            return text.to_string();
        };
        let end = start + end_offset + close.len();
        // Gli a capo attorno al blocco sono suoi: se ne restassero, ogni
        // passata che toglie e rimette il rilievo farebbe crescere il file di
        // una riga vuota — e due passate di seguito non darebbero più gli
        // stessi byte.
        let head = text[..start].trim_end_matches('\n');
        let tail = text[end..].trim_start_matches('\n');
        if head.is_empty() {
            return tail.to_string();
        }
        format!("{head}\n\n{tail}")
    }

    /// Il blocco pronto da infilare, delimitatori compresi.
    pub fn render(&self, body: &str) -> String {
        format!("{}\n{body}\n{}", self.open(), self.close())
    }
}

/// Le righe di un rilievo, citate e staccate l'una dall'altra.
///
/// Citate (`>`) di proposito: chi apre il file le vede staccate dal testo che
/// un umano ha scritto, e nessuno le scambia per l'affermazione del documento.
/// Niente righe, niente rilievo: `None` è la differenza fra «non ho niente da
/// dire» e «ho da dire il vuoto», e solo la prima lascia il file pulito.
pub fn quoted(lines: &[String]) -> Option<String> {
    (!lines.is_empty()).then(|| lines.join("\n>\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: RegenBlock = RegenBlock::new("prova");

    #[test]
    fn stripping_a_block_gives_back_the_original_bytes() {
        // Mutazione che lo rende rosso: togliere i `trim_matches` sugli a capo
        // attorno al blocco — il testo tornerebbe con una riga vuota in più.
        let bare = "---\nstato: aperta\n---\n\n# titolo\n\ncorpo\n";
        let marked = format!(
            "---\nstato: aperta\n---\n\n{}\n\n# titolo\n\ncorpo\n",
            B.render("> avviso")
        );
        assert_eq!(B.strip(&marked), bare);
        // E un testo che il blocco non ce l'ha torna identico a se stesso.
        assert_eq!(B.strip(bare), bare);
    }

    #[test]
    fn a_block_of_another_tag_is_not_mine() {
        // IL DIFETTO CHE QUESTO CASO ESISTE PER PRENDERE: due meccanismi che
        // marcano lo stesso documento si cancellerebbero a vicenda se il taglio
        // non guardasse l'etichetta.
        let altrui = "testo\n\n<!-- altro: inizio -->\n> non mio\n<!-- altro: fine -->\n";
        assert_eq!(B.strip(altrui), altrui);
    }

    #[test]
    fn an_open_without_its_close_is_not_a_block() {
        // Un delimitatore d'apertura sopravvissuto a una scrittura interrotta
        // non autorizza a tagliare fino in fondo al file.
        let monco = "testo\n\n<!-- prova: inizio -->\n> avviso a metà\n";
        assert_eq!(B.strip(monco), monco);
    }

    #[test]
    fn quoted_lines_are_separated_by_a_quoted_blank() {
        assert_eq!(quoted(&[]), None);
        let body = quoted(&["> uno".to_string(), "> due".to_string()]).unwrap();
        assert_eq!(body, "> uno\n>\n> due");
    }
}
