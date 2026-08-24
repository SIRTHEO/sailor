//! Accorcia l'uscita di un comando prima che entri nel contesto, e lo dichiara.
//!
//! PERCHÉ ESISTE, misurato il 24/08/2026 su 976 transcript di sette giorni:
//! 42.732 chiamate a Bash hanno riportato 54,6 MB di uscita, e il 94,5% di
//! quello che entra nel contesto da uno strumento viene di lì, non dai file
//! letti. Il peso sta tutto nella coda: la mediana è 463 B, ma il 6,6% delle
//! chiamate porta il 45% dei byte.
//!
//! COSA HA CAMBIATO IL PROGETTO, misurato sullo stesso corpus: le uscite che
//! ripetono la stessa riga (compilazioni, installazioni) valgono l'8% dei byte
//! grossi e le barre di avanzamento **zero** — lo strumento Bash non è un
//! terminale, e nessun programma le stampa. Il 69% è invece uscita eterogenea
//! di `grep`, `cat`, `sed`: leggere file per comando. Quindi qui non si
//! riconosce il comando (il primo token è `cd` nel 12% dei casi e non dice
//! niente): si guarda la **forma** del testo.
//!
//! IL VERSO IN CUI SBAGLIARE È LA PRUDENZA. Un'uscita di un comando fallito
//! passa intera fino a `error_cap`, quattro volte il tetto normale: chi legge
//! un fallimento deve vedere tutto. Sotto `cap` il filtro è l'identità, byte
//! per byte, e non aggiunge nemmeno una riga. E quando taglia lo dice in prima
//! riga con quanto ha tolto, dove sta l'intero e cosa manca — una riduzione
//! silenziosa viene letta come un'uscita completa, e chi la legge conclude il
//! falso su un lavoro che non ha visto.
//!
//! «Fallito» si riconosce dal codice di uscita quando chi invoca lo passa, e
//! altrimenti da un esito nelle ultime dieci righe: vedi `looks_failed`, dove
//! sta il numero che ha bocciato il criterio più ovvio.

use std::borrow::Cow;

/// I tetti del filtro. Cambiano da riga di comando, non da questo file.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Sotto questa misura l'uscita passa immutata.
    pub cap: usize,
    /// Un'uscita che contiene un esito negativo passa intera fino a qui.
    pub error_cap: usize,
    /// Byte tenuti in testa quando si taglia.
    pub head: usize,
    /// Byte tenuti in coda quando si taglia.
    pub tail: usize,
    /// Da quante righe consecutive uguali (a meno dei numeri) parte il collasso.
    pub run: usize,
    /// Quante righe con un esito si ripescano dal mezzo tagliato.
    pub verdicts: usize,
    /// Quante righe finali si guardano per capire se il comando è fallito.
    pub tail_verdict: usize,
    /// Oltre questa lunghezza una riga sola viene accorciata nel mezzo.
    pub line_cap: usize,
}

/// 4.000 byte ≈ 1.000 token: è quanto vale una risposta a un comando; sopra,
/// quello che arriva è un documento. `error_cap` a 16.000: un fallimento passa
/// intero quattro volte oltre il tetto normale, e oltre quella misura si taglia
/// tenendo tutte le righe d'esito.
///
/// I numeri vengono da una misura, non dal gusto: sul corpus vero il tetto è
/// l'unica leva che sposta il risultato — a 8.000 il massimo teorico di
/// risparmio era il 13% dei byte da comandi, a 4.000 arriva al 25%.
///
/// Testa 1.500 e coda 2.000: più coda che testa, perché l'esito sta in fondo.
pub const DEFAULT: Limits = Limits {
    cap: 4_000,
    error_cap: 16_000,
    head: 1_500,
    tail: 2_000,
    run: 5,
    verdicts: 40,
    tail_verdict: 10,
    // Una riga di 2.000 byte è già mezza schermata. Sopra, di solito, è un
    // documento stampato per intero su una riga sola: sul corpus del 24/08 le
    // righe più lunghe di 2.000 byte valgono il 10,9% dei byte grossi, e 103
    // uscite su 2.823 sono fatte per più di metà da una riga sola.
    line_cap: 2_000,
};

/// L'esito del filtro: il corpo tagliato e i numeri per dichiararlo.
#[derive(Debug, Clone)]
pub struct Filtered {
    /// Il testo da mandare in contesto, senza l'intestazione.
    pub body: String,
    /// Byte in ingresso.
    pub original_bytes: usize,
    /// Righe in ingresso.
    pub original_lines: usize,
    /// Righe che non compaiono nel corpo.
    pub dropped_lines: usize,
    /// Righe con un esito ripescate dal mezzo.
    pub rescued_verdicts: usize,
    /// Righe con un esito che non ci stavano nemmeno nel ripescaggio.
    pub lost_verdicts: usize,
    /// Righe singole accorciate nel mezzo perché troppo lunghe.
    pub shortened_lines: usize,
}

impl Filtered {
    /// Il filtro ha toccato qualcosa?
    pub fn trimmed(&self) -> bool {
        self.dropped_lines > 0 || self.body.len() < self.original_bytes
    }

    /// La riga che dichiara il taglio. `None` quando non c'è stato taglio:
    /// un'intestazione su un'uscita intatta sarebbe rumore che si paga a ogni
    /// comando.
    pub fn header(&self, archive: Option<&str>) -> Option<String> {
        if !self.trimmed() {
            return None;
        }
        let saved = self.original_bytes.saturating_sub(self.body.len());
        let percent = if self.original_bytes > 0 {
            saved * 100 / self.original_bytes
        } else {
            0
        };
        let mut head = format!(
            "[filtro-uscite] {} B → {} B (−{}%): {} righe di {} non mostrate",
            thousands(self.original_bytes),
            thousands(self.body.len()),
            percent,
            thousands(self.dropped_lines),
            thousands(self.original_lines),
        );
        if self.rescued_verdicts > 0 {
            head.push_str(&format!(
                ", {} con un esito ripescate dal mezzo",
                self.rescued_verdicts
            ));
        }
        if self.shortened_lines > 0 {
            head.push_str(&format!(
                ", {} righe accorciate nel mezzo",
                self.shortened_lines
            ));
        }
        if self.lost_verdicts > 0 {
            head.push_str(&format!(
                ". ATTENZIONE: altre {} righe con un esito NON sono qui",
                self.lost_verdicts
            ));
        }
        match archive {
            Some(path) => head.push_str(&format!(". Uscita intera: {path}")),
            None => head.push_str(". L'uscita intera non è stata conservata"),
        }
        Some(head)
    }

    /// Corpo e intestazione insieme, pronti da stampare.
    pub fn render(&self, archive: Option<&str>) -> String {
        match self.header(archive) {
            Some(h) => format!("{h}\n{}", self.body),
            None => self.body.clone(),
        }
    }
}

/// Una riga che porta un esito — errore, fallimento, verdetto di una batteria.
///
/// L'elenco è generoso di proposito: un falso positivo tiene una riga in più,
/// un falso negativo butta l'unica riga che serviva. Si guarda in minuscolo,
/// così `ERROR` e `Error` cadono nello stesso caso.
pub fn is_verdict(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "error",
        "errore",
        "fail",
        "fallit",
        "panic",
        "fatal",
        "exception",
        "traceback",
        "assert",
        "denied",
        "negato",
        "bloccato",
        "refus",
        "rifiut",
        "abort",
        "timeout",
        "killed",
        "segmentation",
        "not found",
        "no such",
        "cannot",
        "can't",
        "unable to",
        "missing",
        "warning",
        "avviso",
        "attenzione",
        "exit code",
        "exit status",
        "test result",
        "unresolved",
        "undefined",
        "conflict",
        "rossa",
        "rosso",
        "✗",
        "✘",
        "❌",
        "⚠",
    ];
    let low = line.to_ascii_lowercase();
    MARKERS.iter().any(|m| low.contains(m))
}

/// Questa uscita è di un comando fallito?
///
/// IL SEGNALE STA IN CODA, e non è un dettaglio di taratura: un comando che
/// fallisce lo dice nelle ultime righe. Guardare invece «c'è la parola errore
/// da qualche parte» dava fallito anche un `grep -rn error` su un sorgente che
/// di errori parla e basta — misurato sul corpus del 24/08/2026: con quel
/// criterio l'88% dei byte grossi passava intatto e il filtro toglieva il 2%.
///
/// Quando chi invoca conosce il codice di uscita, quello vince: è il fatto,
/// questo è un indizio.
pub fn looks_failed(lines: &[&str], limits: &Limits, exit_code: Option<i32>) -> bool {
    if let Some(code) = exit_code {
        return code != 0;
    }
    lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(limits.tail_verdict)
        .any(|l| is_verdict(l))
}

/// La forma di una riga, spogliata dei numeri: due righe di avanzamento che
/// differiscono solo per un contatore hanno lo stesso scheletro.
fn skeleton(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_number = false;
    for c in line.trim().chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
                in_number = true;
            }
        } else {
            in_number = false;
            out.push(c);
        }
        if out.len() >= 200 {
            break;
        }
    }
    out
}

/// Di una riga riscritta dal carrello (`\r`) resta quello che il terminale
/// mostrerebbe: l'ultimo pezzo. Una barra di avanzamento salvata su file
/// diventa migliaia di righe quasi identiche, e questo la riduce a una.
fn last_carriage_segment(line: &str) -> &str {
    match line.rsplit('\r').find(|s| !s.trim().is_empty()) {
        Some(s) => s,
        None => "",
    }
}

/// Accorcia una riga sola troppo lunga, tenendone principio e fine.
///
/// Serve per un caso che il taglio a righe non vede: un JSON, un `strings`, una
/// riga di prova che stampa mezzo file. Il taglio cade su un confine di
/// carattere, mai in mezzo a un accento.
fn shorten_line(line: &str, limits: &Limits) -> Option<String> {
    if line.len() <= limits.line_cap {
        return None;
    }
    let head = floor_boundary(line, limits.line_cap * 2 / 3);
    let tail = ceil_boundary(line, line.len() - limits.line_cap / 3);
    Some(format!(
        "{} [filtro-uscite: {} B nel mezzo di questa riga non mostrati] {}",
        &line[..head],
        thousands(tail - head),
        &line[tail..]
    ))
}

/// Il confine di carattere alla posizione data o subito prima.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Il confine di carattere alla posizione data o subito dopo.
fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Collassa le corse di righe consecutive con lo stesso scheletro.
///
/// Solo consecutive: due righe uguali lontane fra loro possono essere due fatti
/// distinti, e toglierne una cambierebbe il senso. Una riga con un esito non
/// entra mai in una corsa.
fn collapse_runs(lines: &[&str], limits: &Limits) -> (Vec<Cow<'static, str>>, usize) {
    let mut out: Vec<Cow<'static, str>> = Vec::with_capacity(lines.len());
    let mut dropped = 0usize;
    let mut i = 0usize;
    while i < lines.len() {
        let line = last_carriage_segment(lines[i]);
        if is_verdict(line) {
            out.push(Cow::Owned(line.to_string()));
            i += 1;
            continue;
        }
        let shape = skeleton(line);
        let mut j = i + 1;
        while j < lines.len() {
            let next = last_carriage_segment(lines[j]);
            if is_verdict(next) || skeleton(next) != shape {
                break;
            }
            j += 1;
        }
        let run = j - i;
        if run >= limits.run {
            out.push(Cow::Owned(line.to_string()));
            out.push(Cow::Owned(format!(
                "[filtro-uscite] … altre {} righe della stessa forma non mostrate …",
                thousands(run - 2)
            )));
            out.push(Cow::Owned(last_carriage_segment(lines[j - 1]).to_string()));
            dropped += run - 2;
        } else {
            for k in i..j {
                out.push(Cow::Owned(last_carriage_segment(lines[k]).to_string()));
            }
        }
        i = j;
    }
    (out, dropped)
}

/// Quante righe dall'inizio (o dalla fine) stanno in `budget` byte.
fn lines_within(lines: &[Cow<'static, str>], budget: usize, from_end: bool) -> usize {
    let mut used = 0usize;
    let mut n = 0usize;
    for idx in 0..lines.len() {
        let line = if from_end {
            &lines[lines.len() - 1 - idx]
        } else {
            &lines[idx]
        };
        let cost = line.len() + 1;
        if used + cost > budget {
            break;
        }
        used += cost;
        n += 1;
    }
    n
}

/// Il filtro, quando il codice di uscita non si conosce.
pub fn filter(input: &str, limits: &Limits) -> Filtered {
    filter_with_exit(input, limits, None)
}

/// Il filtro. Prudente per costruzione: sotto `cap` non tocca niente, e
/// un'uscita di un comando fallito passa intera fino a `error_cap`.
pub fn filter_with_exit(input: &str, limits: &Limits, exit_code: Option<i32>) -> Filtered {
    let original_bytes = input.len();
    let original_lines = input.lines().count();
    let intact = |body: String| Filtered {
        body,
        original_bytes,
        original_lines,
        dropped_lines: 0,
        rescued_verdicts: 0,
        lost_verdicts: 0,
        shortened_lines: 0,
    };

    if original_bytes <= limits.cap {
        return intact(input.to_string());
    }
    let raw: Vec<&str> = input.lines().collect();
    if original_bytes <= limits.error_cap && looks_failed(&raw, limits, exit_code) {
        return intact(input.to_string());
    }

    // Un filtro che allunga è un filtro che ha sbagliato: succede quando le
    // corse sono tante e cortissime, e le righe che dichiarano il collasso
    // costano più di quelle che tolgono. In quel caso passa l'originale.
    let shorter_or_intact = |f: Filtered| {
        if f.body.len() >= original_bytes {
            intact(input.to_string())
        } else {
            f
        }
    };

    let (mut collapsed, collapsed_dropped) = collapse_runs(&raw, limits);
    // Il taglio dentro la riga viene dopo il collasso: prima cambierebbe gli
    // scheletri, e due righe lunghe diverse si somiglierebbero solo perché
    // tagliate nello stesso punto.
    let mut shortened = 0usize;
    for line in collapsed.iter_mut() {
        if let Some(short) = shorten_line(line, limits) {
            *line = Cow::Owned(short);
            shortened += 1;
        }
    }
    let collapsed_bytes: usize = collapsed.iter().map(|l| l.len() + 1).sum();
    if collapsed_bytes <= limits.cap {
        let body = collapsed
            .iter()
            .map(|l| l.as_ref())
            .collect::<Vec<_>>()
            .join("\n");
        return shorter_or_intact(Filtered {
            body,
            original_bytes,
            original_lines,
            dropped_lines: collapsed_dropped,
            rescued_verdicts: 0,
            lost_verdicts: 0,
            shortened_lines: shortened,
        });
    }

    // Testa e coda intere, il mezzo via — tranne le righe che portano un esito.
    let head_n = lines_within(&collapsed, limits.head, false);
    let tail_n = lines_within(&collapsed, limits.tail, true).min(collapsed.len() - head_n);
    let middle = &collapsed[head_n..collapsed.len() - tail_n];
    let verdicts: Vec<&Cow<'static, str>> = middle.iter().filter(|l| is_verdict(l)).collect();
    let rescued = verdicts.len().min(limits.verdicts);
    let lost = verdicts.len() - rescued;

    let mut body: Vec<String> = Vec::with_capacity(head_n + tail_n + rescued + 3);
    body.extend(collapsed[..head_n].iter().map(|l| l.to_string()));
    body.push(format!(
        "[filtro-uscite] … {} righe di mezzo non mostrate …",
        thousands(middle.len().saturating_sub(rescued))
    ));
    if rescued > 0 {
        body.push("[filtro-uscite] righe con un esito, ripescate dal mezzo:".to_string());
        body.extend(verdicts.iter().take(rescued).map(|l| l.to_string()));
        body.push("[filtro-uscite] … fine del ripescaggio, riprende la coda …".to_string());
    }
    body.extend(
        collapsed[collapsed.len() - tail_n..]
            .iter()
            .map(|l| l.to_string()),
    );

    shorter_or_intact(Filtered {
        body: body.join("\n"),
        original_bytes,
        original_lines,
        dropped_lines: collapsed_dropped + middle.len().saturating_sub(rescued),
        rescued_verdicts: rescued,
        lost_verdicts: lost,
        shortened_lines: shortened,
    })
}

/// 128400 → «128.400». I byte si leggono a colpo d'occhio solo così.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize, text: &str) -> String {
        (0..n)
            .map(|i| format!("{text} {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// La proprietà su cui poggia tutta la prudenza: sotto il tetto il filtro è
    /// l'identità, byte per byte, e non aggiunge nemmeno l'intestazione.
    ///
    /// MUTANTE: cambiato `<=` in `<` sul confronto con `cap`, resta verde;
    /// tolto del tutto il ritorno anticipato, va in rosso qui.
    #[test]
    fn short_output_passes_untouched() {
        let out = "riga uno\nriga due\n";
        let f = filter(out, &DEFAULT);
        assert_eq!(f.body, out);
        assert_eq!(f.render(Some("/tmp/x")), out);
        assert!(!f.trimmed());
        assert_eq!(f.header(None), None);
    }

    /// Il caso per cui il filtro esiste: un elenco lunghissimo e ripetitivo.
    #[test]
    fn a_long_repetitive_output_is_collapsed_and_declared() {
        let out = lines(4000, "  Compiling qualcosa v1.0.0");
        let f = filter(&out, &DEFAULT);
        assert!(f.body.len() < out.len() / 10, "{} B", f.body.len());
        assert!(f.dropped_lines > 3000, "{}", f.dropped_lines);
        let header = f.header(Some("/tmp/intero.txt")).expect("deve dichiarare");
        assert!(header.contains("filtro-uscite"), "{header}");
        assert!(header.contains("non mostrate"), "{header}");
        assert!(header.contains("/tmp/intero.txt"), "{header}");
    }

    /// L'altra direzione, quella che conta: un'uscita che fallisce arriva
    /// intera, e sopra il tetto normale.
    ///
    /// MUTANTE: tolto il ramo `has_verdict`, questo va in rosso.
    #[test]
    fn an_output_with_an_error_passes_whole() {
        let mut out = lines(400, "  Compiling qualcosa v1.0.0");
        out.push_str("\nerror[E0308]: mismatched types\n");
        assert!(out.len() > DEFAULT.cap && out.len() < DEFAULT.error_cap);
        let f = filter(&out, &DEFAULT);
        assert_eq!(f.body, out);
        assert!(!f.trimmed());
        assert!(f.header(None).is_none());
    }

    /// Un'uscita enorme si taglia anche quando fallisce, ma l'errore sepolto in
    /// mezzo viene ripescato e la perdita dichiarata. Il rumore qui è tutto
    /// diverso apposta: se fosse ripetitivo lo toglierebbe il collasso, e il
    /// ripescaggio non verrebbe mai messo alla prova.
    #[test]
    fn a_huge_failing_output_keeps_the_verdict_lines() {
        // Ogni riga ha una forma sua: i numeri non bastano a renderle diverse,
        // perché lo scheletro li ignora apposta.
        let alfabeto = ["alfa", "beta", "gamma", "delta", "epsilon", "zeta", "eta"];
        let mut rows: Vec<String> = (0..1200)
            .map(|i| {
                let a = alfabeto[i % alfabeto.len()];
                let b = alfabeto[(i / 7) % alfabeto.len()];
                let c = alfabeto[(i / 49) % alfabeto.len()];
                format!("percorso/{a}/{b}-{c}-{i}.ts contiene {a} di {c} e {b}")
            })
            .collect();
        rows.insert(600, "error: il difetto vero, sepolto in mezzo".to_string());
        let out = rows.join("\n");
        assert!(out.len() > DEFAULT.error_cap);
        let f = filter(&out, &DEFAULT);
        assert!(
            f.body.contains("il difetto vero"),
            "l'errore è stato buttato: {}",
            &f.body[..400.min(f.body.len())]
        );
        assert_eq!(f.rescued_verdicts, 1);
        assert!(f.body.len() < out.len() / 2);
    }

    /// Il collasso da solo può bastare: quando l'uscita ripetitiva scende sotto
    /// il tetto, l'errore resta dov'era senza bisogno di ripescarlo.
    #[test]
    fn a_repetitive_failing_output_keeps_the_error_in_place() {
        let mut parts = vec![lines(2000, "riga di rumore tutta uguale")];
        parts.push("error: il difetto vero, sepolto in mezzo".to_string());
        parts.push(lines(2000, "altro rumore tutto uguale"));
        let out = parts.join("\n");
        assert!(out.len() > DEFAULT.error_cap);
        let f = filter(&out, &DEFAULT);
        assert!(f.body.contains("il difetto vero"), "{}", f.body);
        assert!(f.trimmed());
        assert!(f.body.len() < out.len() / 10);
    }

    /// La distinzione che vale il filtro intero: un `grep` che trova la parola
    /// «error» in un sorgente non è un comando fallito, e si taglia.
    ///
    /// MUTANTE: rimesso il vecchio criterio (`raw.iter().any(is_verdict)`),
    /// questo va in rosso e il filtro torna a togliere il 2%.
    #[test]
    fn a_grep_that_finds_the_word_error_is_not_a_failure() {
        let out = (0..600)
            .map(|i| format!("src/modulo{i}/file.ts:{i}: throw new Error('qualcosa')"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.len() > DEFAULT.cap);
        let f = filter(&out, &DEFAULT);
        assert!(f.trimmed(), "un elenco lungo di occorrenze va tagliato");
    }

    /// L'uscita di una batteria rossa finisce con il verdetto: passa intera.
    #[test]
    fn a_verdict_in_the_last_lines_keeps_the_output_whole() {
        let mut out = lines(200, "test qualcosa::caso ... ok");
        out.push_str("\ntest result: FAILED. 199 passed; 1 failed\n");
        assert!(out.len() > DEFAULT.cap && out.len() < DEFAULT.error_cap);
        assert_eq!(filter(&out, &DEFAULT).body, out);
    }

    /// Il codice di uscita, quando c'è, batte l'indizio del testo — in tutte e
    /// due le direzioni.
    #[test]
    fn the_exit_code_wins_over_the_text() {
        let mut muto = lines(300, "riga che non dice niente di suo");
        muto.push_str("\nfine\n");
        assert!(muto.len() > DEFAULT.cap && muto.len() < DEFAULT.error_cap);
        // Senza codice di uscita si taglia: niente esito in coda.
        assert!(filter(&muto, &DEFAULT).trimmed());
        // Con un codice diverso da zero passa intera anche se il testo tace.
        assert_eq!(filter_with_exit(&muto, &DEFAULT, Some(1)).body, muto);

        let mut parla = lines(300, "riga che non dice niente di suo");
        parla.push_str("\nwarning: attenzione a questa\n");
        // Con zero, un avviso in coda non salva più l'uscita dal taglio.
        assert!(filter_with_exit(&parla, &DEFAULT, Some(0)).trimmed());
    }

    /// Un'uscita che il filtro non sa accorciare esce com'è entrata, non più
    /// lunga: le righe che dichiarano un collasso costano, e su un testo fatto
    /// di corse cortissime costavano più di quanto togliessero (una volta su
    /// 2.823, misurata sul corpus del 24/08).
    #[test]
    fn the_filter_never_makes_the_output_longer() {
        // Corse da cinque righe di due caratteri: il collasso scatta ovunque e
        // ogni marcatore è più lungo delle righe che sostituisce.
        let out = (0..4000)
            .map(|i| format!("{}", (i / 5) % 10))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.len() > DEFAULT.cap);
        let f = filter(&out, &DEFAULT);
        assert!(
            f.body.len() <= out.len(),
            "{} > {}",
            f.body.len(),
            out.len()
        );
    }

    /// Una riga sola che vale mezza uscita si accorcia nel mezzo, e il taglio
    /// cade su un confine di carattere — non a metà di un accento.
    #[test]
    fn one_endless_line_is_shortened_in_the_middle() {
        let single = format!("inizio {} fine", "però ".repeat(2000));
        let f = filter(&single, &DEFAULT);
        assert!(f.body.starts_with("inizio "), "{}", &f.body[..40]);
        assert!(f.body.ends_with("fine"), "{}", &f.body[f.body.len() - 40..]);
        assert!(f.body.contains("nel mezzo di questa riga non mostrati"));
        assert_eq!(f.shortened_lines, 1);
        assert!(f.body.len() < single.len() / 3);
        let header = f.header(None).unwrap();
        assert!(header.contains("1 righe accorciate"), "{header}");
    }

    /// Il carrello: una barra di avanzamento salvata su file si riduce a quello
    /// che il terminale mostrerebbe.
    #[test]
    fn carriage_returns_keep_only_the_last_frame() {
        assert_eq!(last_carriage_segment("10%\r50%\r100%"), "100%");
        assert_eq!(last_carriage_segment("solo questa"), "solo questa");
        assert_eq!(last_carriage_segment("finito\r"), "finito");
    }

    /// Lo scheletro ignora i numeri ma non il resto: due righe di comandi
    /// diversi non si collassano fra loro.
    #[test]
    fn the_skeleton_ignores_numbers_only() {
        assert_eq!(skeleton("test 12 ok"), skeleton("test 4711 ok"));
        assert_ne!(skeleton("test 12 ok"), skeleton("prova 12 ok"));
    }

    /// Le corse corte non si toccano: sotto `run` righe uguali restano tutte.
    #[test]
    fn a_short_run_is_not_collapsed() {
        let raw = ["a 1", "a 2", "a 3"];
        let (out, dropped) = collapse_runs(&raw, &DEFAULT);
        assert_eq!(dropped, 0);
        assert_eq!(out.len(), 3);
    }

    /// Una riga con un esito non entra mai in una corsa collassata, nemmeno se
    /// ha la stessa forma delle vicine.
    #[test]
    fn a_verdict_line_never_disappears_into_a_run() {
        let mut raw: Vec<&str> = vec!["passo 1"; 50];
        raw.push("error: qui si è rotto");
        raw.extend(std::iter::repeat("passo 2").take(50));
        let (out, _) = collapse_runs(&raw, &DEFAULT);
        assert!(out.iter().any(|l| l.contains("qui si è rotto")));
    }

    /// I marcatori di esito si riconoscono in qualunque cassa.
    #[test]
    fn verdict_markers_are_case_insensitive() {
        assert!(is_verdict("ERROR: boom"));
        assert!(is_verdict("test result: FAILED. 3 passed; 1 failed"));
        assert!(is_verdict("warning: unused variable"));
        assert!(!is_verdict("   Compiling serde v1.0.0"));
        assert!(!is_verdict("total 48"));
    }

    /// I numeri dell'intestazione sono quelli veri: se mentissero, il filtro
    /// sarebbe peggio di nessun filtro.
    #[test]
    fn the_header_counts_what_really_happened() {
        let out = lines(4000, "  Compiling qualcosa v1.0.0");
        let f = filter(&out, &DEFAULT);
        let header = f.header(None).unwrap();
        assert!(header.contains(&thousands(f.original_bytes)), "{header}");
        assert!(header.contains(&thousands(f.body.len())), "{header}");
        assert!(
            header.contains("non è stata conservata"),
            "senza archivio deve dirlo: {header}"
        );
    }

    #[test]
    fn thousands_separates_with_dots() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1.000");
        assert_eq!(thousands(54_591_626), "54.591.626");
    }
}
