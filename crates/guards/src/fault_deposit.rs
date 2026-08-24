//! Quale riga di registro diventa una voce di coda, e con quale soglia.
//!
//! LA COSA, PRIMA DEL MECCANISMO. Quando un'automazione inciampa venti volte di
//! fila nello stesso guasto, quel guasto sta in un registro che non legge
//! nessuno: il registro del presidio delle sessioni ha gridato 3.772 volte in
//! cinque giorni davanti a zero lettori. Qui sta il giudizio che chiude il giro
//! — una riga marcata che si ripete oltre soglia apre da sé una voce nella coda
//! di bordo, che il macchinista consuma già.
//!
//! PERCHÉ IN RUST, DAL 25/08/2026. Il giudizio viveva in `deposito-guasti.sh`,
//! 680 righe di shell con una batteria che qualcuno doveva ricordarsi di
//! lanciare — e per mezza giornata l'aveva letta rossa per un difetto suo. Qui
//! le prove girano a ogni compilazione. La divisione è quella di casa: **questo
//! modulo non tocca il disco**, decide su testo e restituisce testo; l'I/O — la
//! coda, il lucchetto, il proprio registro — sta in `claude-hooks`.
//!
//! COSA DISTINGUE UN GUASTO DA UNA RIGA QUALSIASI: non lo decide questo modulo.
//! Le automazioni marcano le proprie righe alla fonte con `[guasto=<nome>]`,
//! subito dopo l'orario. Qui si contano solo quelle: una riga di cronaca, per
//! quanto si ripeta, non apre niente. È la metà del meccanismo che gli impedisce
//! di affogare la coda che doveva servire.

use std::collections::BTreeMap;

/// Il fuso in cui un registro scrive le proprie righe.
///
/// SI DICHIARA, NON SI INDOVINA. Dei tre registri sorvegliati due scrivono l'ora
/// locale e il terzo UTC; con due ore di scarto una finestra prende il giorno
/// sbagliato ai bordi, e dalla riga non si vede quale dei due sia.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Zone {
    Local,
    Utc,
}

/// Un registro sorvegliato: dove sta, di chi è, e i tre numeri che dicono quando
/// una ripetizione è troppa.
///
/// LA SOGLIA SI CONTA A GIRI, NON A ORE. Una soglia unica espressa in ore è rotta
/// per costruzione su chi gira di rado: l'appuntamento del mattino fa un giro al
/// giorno, quindi trenta ripetizioni in ventiquattr'ore non sono rare — sono
/// impossibili. `per_day` sta accanto alla soglia proprio perché chi la ritocca
/// possa rifare il conto senza andarlo a cercare.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Register {
    pub path: String,
    pub source: String,
    pub zone: Zone,
    pub threshold: u32,
    pub per_day: u32,
    pub window_h: u32,
}

impl Register {
    /// Quante volte questo programma gira dentro la propria finestra: il
    /// denominatore contro cui una soglia è raggiungibile o muta.
    ///
    /// Il conto passa da 64 bit e torna saturando: `1440 × 24` sta larghissimo
    /// in `u32`, ma due numeri scritti a mano in una tabella non sono un dato
    /// fidato, e un programma che esiste per non morire in silenzio non può
    /// morire per un trabocco su una riga di configurazione.
    pub fn turns(&self) -> u32 {
        let n = u64::from(self.per_day) * u64::from(self.window_h) / 24;
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

/// Una riga della tabella dei registri, letta o rifiutata.
///
/// UNA RIGA MALFORMATA FERMA QUEL REGISTRO, e non lo lascia passare. Preso in
/// prova nella versione shell: con una riga a tre colonne invece di sei i tre
/// numeri restavano vuoti, ogni confronto numerico dava errore su stderr — e il
/// giro **scriveva lo stesso**, perché un confronto fallito non è un confronto
/// vero. Cioè il caso peggiore: una tabella sbagliata apriva voci senza soglia.
/// Un dato che non si capisce è un guasto, non un permesso di procedere.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RegisterRow {
    Ok(Register),
    /// I tre numeri come sono scritti, non come si sperava fossero: il messaggio
    /// che ne esce finisce citato dentro una voce di coda, e chi la legge deve
    /// vedere la riga di chi l'ha scritta, non un riassunto.
    Malformed {
        source: String,
        path: String,
        threshold: String,
        per_day: String,
        window_h: String,
    },
}

impl RegisterRow {
    /// Dove sta il registro, che si sappia leggere la riga o no: la domanda «il
    /// file c'è?» viene prima di «la riga si capisce?», come nella versione
    /// shell — una riga sbagliata che punta a un registro inesistente non ha
    /// nessuno da avvisare.
    pub fn path(&self) -> &str {
        match self {
            RegisterRow::Ok(r) => &r.path,
            RegisterRow::Malformed { path, .. } => path,
        }
    }
}

/// La tabella dei registri: `percorso|nome|fuso|soglia|giri al giorno|ore`.
pub fn parse_registers(text: &str) -> Vec<RegisterRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(parse_register_row)
        .collect()
}

fn parse_register_row(line: &str) -> RegisterRow {
    let f: Vec<&str> = line.split('|').collect();
    let path = f.first().copied().unwrap_or("").to_string();
    let source = f.get(1).copied().unwrap_or("").to_string();
    // Sei campi esatti, e i tre numeri tutti positivi. Il fuso non è un numero e
    // vale la regola opposta: quello che non si riconosce è ora locale, che è il
    // caso di due registri su tre — un fuso scritto male non è una ragione per
    // smettere di guardare un registro.
    let numbers = if f.len() == 6 {
        match (positive(f[3]), positive(f[4]), positive(f[5])) {
            (Some(t), Some(p), Some(w)) => Some((t, p, w)),
            _ => None,
        }
    } else {
        None
    };
    match numbers {
        Some((threshold, per_day, window_h)) => RegisterRow::Ok(Register {
            path,
            source,
            zone: if f[2] == "utc" { Zone::Utc } else { Zone::Local },
            threshold,
            per_day,
            window_h,
        }),
        None => RegisterRow::Malformed {
            source,
            path,
            threshold: f.get(3).copied().unwrap_or("").to_string(),
            per_day: f.get(4).copied().unwrap_or("").to_string(),
            window_h: f.get(5).copied().unwrap_or("").to_string(),
        },
    }
}

/// La soglia che un'automazione si impone da sé, e il tetto che la rende
/// leggibile.
///
/// UN'AUTOMAZIONE CHE SI DÀ UN TETTO RENDE MUTA LA SOGLIA DEL SUO REGISTRO. Il
/// tetto è dell'albero, la soglia è della chiave: la staffetta conta i tentativi
/// per albero, il deposito per (automazione, guasto, soggetto). Sei nomi
/// spartiscono lo stesso tetto, quindi la soglia sta a una finestra di resa.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Cap {
    pub source: String,
    pub name: String,
    pub threshold: u32,
    pub cap: u32,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CapRow {
    Ok(Cap),
    Malformed {
        source: String,
        name: String,
        threshold: String,
        cap: String,
    },
}

/// La tabella dei tetti: `automazione|guasto|soglia|tetto al giorno`.
///
/// I DUE NUMERI SI PROVANO UNO PER UNO, mai concatenati. Con `"$a$b"` una riga a
/// cui manca il tetto diventava una stringa di sole cifre, passava il controllo,
/// e il giro applicava la soglia lo stesso.
///
/// CINQUE CAMPI SONO MALFORMATI QUANTO TRE. La versione shell aveva due lettori
/// — il controllo tagliava col separatore e metteva il resto nell'ultimo campo,
/// chi la usava contava i campi — e dovevano restare d'accordo a mano. Qui il
/// lettore è uno solo, e la domanda «quanti campi» si fa una volta.
pub fn parse_caps(text: &str) -> Vec<CapRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(parse_cap_row)
        .collect()
}

fn parse_cap_row(line: &str) -> CapRow {
    let f: Vec<&str> = line.split('|').collect();
    let source = f.first().copied().unwrap_or("").to_string();
    let name = f.get(1).copied().unwrap_or("").to_string();
    let threshold = f.get(2).copied().unwrap_or("").to_string();
    // Il quarto campo di una riga a cinque non esiste: ciò che segue non è un
    // tetto con una nota accanto, è una riga che non si capisce. Si riporta
    // com'è, perché il messaggio serve a chi l'ha scritta.
    let cap = if f.len() > 3 { f[3..].join("|") } else { String::new() };
    match (f.len() == 4, positive(&threshold), positive(&cap)) {
        (true, Some(t), Some(c)) => CapRow::Ok(Cap {
            source,
            name,
            threshold: t,
            cap: c,
        }),
        _ => CapRow::Malformed {
            source,
            name,
            threshold,
            cap,
        },
    }
}

/// Un intero maggiore di zero, scritto nella forma in cui si scrivono i numeri:
/// niente zero davanti.
///
/// Lo zero non vale — una soglia di zero aprirebbe una voce su nessuna
/// ripetizione, e un registro che gira zero volte al giorno non è un registro.
/// E `03` non vale come 3: è una battitura, e nella versione shell i due lettori
/// della stessa riga la trattavano in due modi diversi. Qui il lettore è uno.
fn positive(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    match s.parse::<u32>() {
        Ok(n) if n > 0 => Some(n),
        _ => None,
    }
}

/// Solo le righe che si sono capite: sono le uniche che possono decidere
/// qualcosa.
fn sound(rows: &[CapRow]) -> impl Iterator<Item = &Cap> {
    rows.iter().filter_map(|r| match r {
        CapRow::Ok(c) => Some(c),
        CapRow::Malformed { .. } => None,
    })
}

/// La soglia che vale per un guasto: la sua, se è dichiarata in una riga che si
/// capisce; altrimenti quella del registro. La forzatura da riga di comando
/// batte tutte e due, perché serve al collaudo e deve poter mentire in fretta.
pub fn threshold_for(
    caps: &[CapRow],
    source: &str,
    name: &str,
    register_threshold: u32,
    forced: Option<u32>,
) -> u32 {
    if let Some(f) = forced {
        return f;
    }
    sound(caps)
        .find(|c| c.source == source && c.name == name)
        .map(|c| c.threshold)
        .unwrap_or(register_threshold)
}

/// Il tetto dichiarato per un guasto, o niente se non ne ha uno.
///
/// IL DENOMINATORE CHE CONTA È IL TETTO, non i giri del programma: «3 su 1440»
/// sembra una soglia larghissima, «3 su 11» dice la cosa vera. Serve al testo
/// della voce, non al giudizio.
pub fn cap_for(caps: &[CapRow], source: &str, name: &str) -> Option<u32> {
    sound(caps)
        .find(|c| c.source == source && c.name == name)
        .map(|c| c.cap)
}

/// La coda del testo che accompagna ogni conteggio scritto in una voce.
pub fn cap_note(cap: Option<u32>) -> String {
    match cap {
        Some(c) => format!(", su un tetto di {c} al giorno che l'automazione si impone da se'"),
        None => String::new(),
    }
}

/// Una spia di questo programma: la riga da mettere nel proprio registro e
/// quella da stampare a chi guarda.
///
/// LE DUE ESCONO INSIEME, E NON È RIDONDANZA. Il registro proprio non basta —
/// chi lancia il deposito butta via il suo stderr quando l'uscita è zero, e un
/// programma che grida davanti a nessuno è esattamente ciò che questo esiste per
/// togliere agli altri. La riga marcata nel registro proprio è quella che poi
/// **rientra da sé** dalla porta principale, perché il registro del deposito sta
/// nella tabella dei registri.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Alarm {
    /// Il nome del guasto, in kebab-case: è la marcatura con cui questa riga
    /// tornerà a contarsi.
    pub fault: &'static str,
    pub note: String,
    /// La riga per stderr, vuota se questa spia parla solo al registro.
    pub warning: String,
}

/// Le spie della tabella dei tetti, lette prima di qualunque registro.
///
/// UNA SOGLIA SOPRA IL PROPRIO TETTO NON È SEVERA, È MUTA: è il difetto che la
/// tabella esiste per chiudere, e non può rientrare dalla porta della tabella
/// stessa.
pub fn cap_table_alarms(rows: &[CapRow]) -> Vec<Alarm> {
    let mut out = Vec::new();
    for row in rows {
        match row {
            CapRow::Malformed {
                source,
                name,
                threshold,
                cap,
            } => out.push(Alarm {
                fault: "tabella-tetti-malformata",
                note: format!("{source}/{name}: threshold='{threshold}' cap='{cap}' — row ignored"),
                warning: format!(
                    "  MALFORMED cap row for {source}/{name}: threshold='{threshold}' cap='{cap}'"
                ),
            }),
            CapRow::Ok(c) if c.threshold > c.cap => out.push(Alarm {
                fault: "soglia-guasto-irraggiungibile",
                note: format!(
                    "{}/{}: threshold {} above its own cap of {} a day — it can never fire",
                    c.source, c.name, c.threshold, c.cap
                ),
                warning: format!(
                    "  UNREACHABLE threshold for {}/{}: {} over a cap of {}",
                    c.source, c.name, c.threshold, c.cap
                ),
            }),
            CapRow::Ok(_) => {}
        }
    }
    out
}

/// La spia che il controllo di sopra non può dare: una soglia di guasto sopra
/// quella del suo registro.
///
/// UNA SOGLIA DI GUASTO ESISTE PER ABBASSARE, MAI PER ALZARE. Se ne trovi una
/// sopra quella del registro, qualcuno ha spento una spia scrivendo un numero in
/// una tabella — e il controllo del tetto non se ne accorge, perché lì la soglia
/// si misura contro un numero della stessa mano.
///
/// CON LA FORZATURA IL CONFRONTO NON SI FA: `--threshold` sostituisce la soglia
/// del registro con la bugia del collaudo, e confrontarci le soglie dei guasti
/// produce allarmi inventati dal collaudo stesso.
pub fn raised_threshold_alarms(
    caps: &[CapRow],
    source: &str,
    register_threshold: u32,
    forced: Option<u32>,
) -> Vec<Alarm> {
    if forced.is_some() {
        return Vec::new();
    }
    sound(caps)
        .filter(|c| c.source == source && c.threshold > register_threshold)
        .map(|c| Alarm {
            fault: "soglia-guasto-sopra-il-registro",
            note: format!(
                "{}/{}: fault threshold {} above the register's own {} — a fault threshold is there to lower it",
                c.source, c.name, c.threshold, register_threshold
            ),
            warning: format!(
                "  RAISED threshold for {}/{}: {} over the register's {}",
                c.source, c.name, c.threshold, register_threshold
            ),
        })
        .collect()
}

/// La spia sul registro stesso: una soglia che i suoi giri non raggiungono.
///
/// È il controllo che avrebbe intercettato il difetto vero della tabella: una
/// soglia di trenta su un programma che gira una volta al giorno non è severa, è
/// irraggiungibile. Un numero che non può scattare è peggio di uno sbagliato:
/// non dà mai torto a se stesso.
pub fn unreachable_register_alarm(r: &Register) -> Option<Alarm> {
    let turns = r.turns();
    if turns >= r.threshold {
        return None;
    }
    Some(Alarm {
        fault: "soglia-registro-irraggiungibile",
        note: format!(
            "{}: threshold {}, but in {}h it only runs {} times ({} a day) — it can never fire",
            r.source, r.threshold, r.window_h, turns, r.per_day
        ),
        warning: String::new(),
    })
}

/// Un guasto ripetuto, come esce dal registro: quante volte, su cosa, e fra
/// quando e quando.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Group {
    pub count: u32,
    pub name: String,
    pub subject: String,
    pub first: String,
    pub last: String,
    /// L'ultima riga esatta, non riassunta: è ciò che si legge nella voce.
    pub text: String,
}

/// Raggruppa le righe **marcate** di un registro, da un istante in poi.
///
/// CHI CONTA PARTE DALLE RIGHE CHE COMINCIANO CON UNA DATA. Nel registro del
/// presidio ci sono 539 righe che non sono voci ma l'uscita grezza di un comando
/// riversata dentro (16-17/08/2026): senza questo filtro falserebbero ogni
/// conteggio fatto a peso.
///
/// Il confronto fra istanti è alfabetico e non numerico: in forma ISO
/// `AAAA-MM-GGTHH:MM:SS` le due cose coincidono, e si evita di convertire in
/// epoca una riga alla volta.
///
/// L'ORDINE DI USCITA È QUELLO DELLA CHIAVE, non quello di arrivo. Non cambia
/// cosa nasce — due gruppi differiscono per nome o per soggetto, quindi anche
/// per nome di file — ma rende **riproducibili le righe stampate**: due giri a
/// secco sullo stesso registro si possono confrontare invece di rimescolarsi,
/// e in awk l'ordine di `for (k in n)` non è definito.
pub fn extract(log: &str, cutoff: &str) -> Vec<Group> {
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for line in log.lines() {
        let Some(stamp) = stamp_of(line) else { continue };
        if stamp.as_str() < cutoff {
            continue;
        }
        let body = line[19..].trim_start();
        let body = body.strip_prefix("UTC").unwrap_or(body).trim_start();
        let Some(name) = fault_name(body) else { continue };
        let subject = subject_of(body);
        let entry = groups
            .entry((name.clone(), subject.clone()))
            .or_insert_with(|| Group {
                count: 0,
                name,
                subject,
                first: stamp.clone(),
                last: String::new(),
                text: String::new(),
            });
        entry.count += 1;
        if stamp < entry.first {
            entry.first = stamp.clone();
        }
        if entry.last.is_empty() || stamp > entry.last {
            entry.last = stamp;
            entry.text = line.to_string();
        }
    }
    groups.into_values().collect()
}

/// I primi diciannove caratteri, se sono una data e un'ora: `2026-08-24 21:03:00`
/// e `2026-08-24T21:03:00` valgono uguale, e si normalizzano sulla seconda forma
/// perché è quella che si confronta.
fn stamp_of(line: &str) -> Option<String> {
    let b = line.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    let shape = (0..4).all(digit)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9)
        && (b[10] == b' ' || b[10] == b'T')
        && digit(11)
        && digit(12)
        && b[13] == b':'
        && digit(14)
        && digit(15)
        && b[16] == b':'
        && digit(17)
        && digit(18);
    if !shape {
        return None;
    }
    let mut stamp = line[..19].to_string();
    stamp.replace_range(10..11, "T");
    Some(stamp)
}

/// Il nome dentro `[guasto=…]`, se la riga è marcata. Solo kebab-case minuscolo:
/// è la forma che le automazioni scrivono, e allargarla farebbe entrare in coda
/// righe di cronaca che contengono una parentesi quadra.
fn fault_name(body: &str) -> Option<String> {
    let rest = body.strip_prefix("[guasto=")?;
    let end = rest.find(']')?;
    let name = &rest[..end];
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return None;
    }
    Some(name.to_string())
}

/// Il soggetto separa due stalli diversi sullo stesso guasto. Si prende dalla
/// riga perché è già lì: il presidio nomina la sessione, gli altri il pannello.
/// Quando non c'è nessuno dei due, il guasto è dell'automazione nel suo insieme.
/// IL SOGGETTO ENTRA NELLA CHIAVE, quindi le due regole vanno prese alla
/// lettera: un carattere in più o in meno apre una voce nuova invece di
/// aggiornare quella che c'è già. Sono `sess=[0-9a-f]+` e `term_[0-9a-f-]+`, le
/// stesse due della versione shell — **solo minuscole**: un identificativo
/// maiuscolo non è un soggetto riconosciuto, ed è meglio che resti tale finché
/// qualcuno non decide il contrario, invece di cambiare in silenzio la chiave di
/// una voce già aperta.
fn subject_of(body: &str) -> String {
    if let Some(s) = scan(body, "sess=", |c| is_lower_hex(c)) {
        return s;
    }
    if let Some(s) = scan(body, "term_", |c| is_lower_hex(c) || c == b'-') {
        return s;
    }
    "-".to_string()
}

fn is_lower_hex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

/// La prima posizione in cui l'**intera** forma riesce, non la prima in cui
/// comincia: un `sess=ZZZ` più avanti nella riga non deve nascondere il
/// `sess=abc12345` che viene dopo. È la differenza fra `find` e il `match` di
/// awk, e da sola cambia la chiave di una voce.
fn scan(body: &str, prefix: &str, valid: fn(u8) -> bool) -> Option<String> {
    let bytes = body.as_bytes();
    let mut from = 0;
    while let Some(rel) = body[from..].find(prefix) {
        let at = from + rel;
        let tail = &bytes[at + prefix.len()..];
        let len = tail.iter().take_while(|c| valid(**c)).count();
        if len > 0 {
            return Some(body[at..at + prefix.len() + len].to_string());
        }
        from = at + prefix.len();
    }
    None
}

/// La chiave di una voce: `<automazione>/<guasto>/<soggetto>`.
///
/// UNA VOCE PER GUASTO, NON UNA PER RIPETIZIONE. Senza questa guardia un guasto
/// che si ripete ogni minuto scriverebbe mille file e affogherebbe la coda che
/// doveva servire. Due sessioni diverse bloccate sullo stesso guasto sono due
/// guasti, perché si sbloccano una per una.
pub fn key_of(source: &str, name: &str, subject: &str) -> String {
    format!("{source}/{name}/{subject}")
}

/// La riga che porta la chiave dentro la voce: è così che una voce si ritrova al
/// giro dopo. Si cerca la chiave, non il nome del file — il nome può cambiare,
/// la chiave no.
pub fn key_line(key: &str) -> String {
    format!("chiave: {key}")
}

/// Il nome del file, che dice di cosa parla senza aprirlo.
pub fn file_stem(today: &str, source: &str, name: &str, subject: &str) -> String {
    let base = format!("{today}-guasto-{source}-{name}");
    if subject == "-" {
        return base;
    }
    let slug: String = subject
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{base}-{}", slug.trim_end_matches('-'))
}

/// Quante volte questa voce era già tornata.
pub fn returns_of(entry: &str) -> u32 {
    entry
        .lines()
        .find(|l| l.starts_with("ritorni:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Riscrive alcune righe dell'intestazione, e solo quelle.
///
/// SI RISCRIVE SOLO DENTRO L'INTESTAZIONE, fra i due `---`. Senza questo confine
/// il primo giro di prova della versione shell ha riscritto una riga del corpo
/// che per caso cominciava con la stessa parola, e il testo del guasto è
/// diventato illeggibile: un aggiornamento che corrompe la voce che doveva
/// tenere aggiornata.
fn rewrite_header(entry: &str, replace: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(entry.len() + 64);
    let mut in_header = false;
    for (i, line) in entry.lines().enumerate() {
        let boundary = line == "---";
        if i == 0 && boundary {
            in_header = true;
        } else if in_header && boundary {
            in_header = false;
        } else if in_header {
            if let Some(new) = replace(line) {
                out.push_str(&new);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Il campo di questa riga, se è una riga di intestazione `campo: valore`.
fn field_is(line: &str, name: &str) -> bool {
    line.starts_with(name) && line[name.len()..].starts_with(':')
}

/// Una voce viva che si aggiorna: il conteggio, la soglia e l'ultima volta.
///
/// IL NUMERO IN TESTA È L'UNICO CHE VALE. Il corpo porta i numeri del giorno in
/// cui la voce è nata, e nel frattempo la soglia può essere cambiata:
/// l'intestazione è l'unico posto dove il numero vivo può stare.
pub fn update_header(entry: &str, count: u32, last: &str, threshold: u32) -> String {
    rewrite_header(entry, &|line| {
        if field_is(line, "ripetizioni") {
            Some(format!("ripetizioni: {count}"))
        } else if field_is(line, "soglia") {
            Some(format!("soglia: {threshold}"))
        } else if field_is(line, "ultima") {
            Some(format!("ultima: {last}"))
        } else {
            None
        }
    })
}

/// Una voce chiusa che si riapre.
///
/// SI RIAPRE QUELLA, NON SE NE FA UNA GEMELLA. Prima qui nasceva un file col
/// suffisso numerico e il vecchio restava chiuso per sempre: a ogni ciclo
/// chiudi-riapri la coda cresceva di un file che nessuno avrebbe ricollegato
/// all'altro. Ma la coda è anche il registro di cosa si rompe più spesso, e quel
/// registro si legge solo se un guasto che torna tre volte racconta tre ritorni
/// in **una** voce.
///
/// LO STATO NUOVO COMINCIA CON `aperta`, e non è una formula: chi sceglie quale
/// voce svegliare legge la **prima parola** di `stato:`. Uno `stato: RIAPERTA
/// alle 09:50` è uno stato ignoto, e il 21/08/2026 ha reso invisibile una voce
/// riaperta — riaperta e muta insieme, il peggio dei due mondi.
#[allow(clippy::too_many_arguments)]
pub fn reopen_header(
    entry: &str,
    count: u32,
    first: &str,
    last: &str,
    threshold: u32,
    returns: u32,
    back_at: &str,
) -> String {
    rewrite_header(entry, &|line| {
        if field_is(line, "stato") {
            Some(format!(
                "stato: aperta — riaperta il {back_at}, il guasto e tornato"
            ))
        } else if field_is(line, "ripetizioni") {
            Some(format!("ripetizioni: {count}"))
        } else if field_is(line, "soglia") {
            Some(format!("soglia: {threshold}"))
        } else if field_is(line, "prima") {
            Some(format!("prima: {first}"))
        } else if field_is(line, "ultima") {
            Some(format!("ultima: {last}"))
        } else if field_is(line, "ritorni") {
            Some(format!("ritorni: {returns}"))
        } else {
            None
        }
    })
}

/// La narrazione che si accoda a una voce riaperta.
#[allow(clippy::too_many_arguments)]
pub fn return_note(
    count: u32,
    threshold: u32,
    cap_note: &str,
    first: &str,
    last: &str,
    text: &str,
    back_at: &str,
    returns: u32,
) -> String {
    format!(
        "
---

**Tornato il {back_at}** — ritorno numero {returns}.

Dopo l'ultima chiusura il registro ha ripetuto la stessa riga **{count} volte**
contro una soglia di **{threshold}**{cap_note}, fra il {first} e il {last}. La soglia va
riletta qui: il corpo della voce porta quella del giorno in cui e' nata, e nel
frattempo puo' essere cambiata. L'ultima:

    {text}
"
    )
}

/// Tutto ciò che una voce nuova racconta di sé.
pub struct EntryFacts<'a> {
    pub key: &'a str,
    pub source: &'a str,
    pub name: &'a str,
    pub subject: &'a str,
    pub count: u32,
    pub threshold: u32,
    pub cap_note: &'a str,
    pub first: &'a str,
    pub last: &'a str,
    pub text: &'a str,
    pub log_path: &'a str,
    pub per_day: u32,
    pub window_h: u32,
    pub turns: u32,
    pub when: &'a str,
}

/// La voce che nasce. Il testo è lungo apposta: chi la trova in coda non ha
/// visto niente di ciò che è successo, e la prima domanda che si fa — «chi l'ha
/// scritta?» — ha una risposta che sorprende.
pub fn new_entry(f: &EntryFacts) -> String {
    let EntryFacts {
        key,
        source,
        name,
        subject,
        count,
        threshold,
        cap_note,
        first,
        last,
        text,
        log_path,
        per_day,
        window_h,
        turns,
        when,
    } = *f;
    format!(
        "---
sessione: deposito-guasti (automazione, nessuna sessione)
albero: -
quando: {when}
stato: aperta
per: un investigator — nessuno ha guardato questa voce: l'ha aperta un conteggio, quindi la causa non e' stata cercata da nessuno. Chi la prende diagnostica prima di riparare, e se il rimedio e' meccanico gira la voce a un builder
chiave: {key}
automazione: {source}
guasto: {name}
soggetto: {subject}
ripetizioni: {count}
soglia: {threshold}
prima: {first}
ultima: {last}
ritorni: 0
---

**Chi ha scritto questa voce**: nessuno. `deposito-guasti` legge i registri
delle automazioni e apre da se' una voce quando una riga **marcata come guasto
alla fonte** si ripete oltre soglia. Non l'ha vista una persona: e' arrivata qui
da sola.

**Cosa stava facendo**: `{source}` girava per conto suo.

**Cosa e' successo**: ha scritto **{count} volte** la stessa riga di guasto
`{name}` su `{subject}`, fra il {first} e il {last}. L'ultima, esatta e non
riassunta:

    {text}

**Quante volte**: {count}, contro una soglia di {threshold}{cap_note}. La soglia e' di
questo guasto se ne ha una sua, altrimenti di `{source}`, che fa {per_day} giri
al giorno e in {window_h}h ne ha a disposizione {turns} — non e' una soglia
della casa. **Un guasto ha una soglia propria quando l'automazione si impone un
tetto**: se dopo tre tentativi si arrende per sei ore, la soglia del programma
non potrebbe piu' scattare, e il numero che conta e' il tetto, non i giri. Il
numero in testa (`ripetizioni:`) e' l'unico che vale: si aggiorna a ogni giro
finche' questa voce resta aperta.

**Da quando conta questo numero**: dalla prima riga **marcata**, non dalla prima
volta che il guasto e' successo. Sono due cose diverse ogni volta che la
marcatura e' arrivata dopo il comportamento: il 21/08/2026 una voce diceva 54
mentre il comportamento andava avanti da 588 giri, undici volte tanto, perche'
le prime dieci ore di righe nessuno le aveva ancora marcate. Chi vuole sapere
**da quanto dura** non lo legge qui: lo cerca nel registro, dove le righe
precedenti al marcatore ci sono ancora e si contano col comando qui sotto senza
il filtro sul marcatore.

**Perche' e' un guasto e non un rinvio**: l'ha deciso chi ha scritto la riga, non
chi la conta. Le automazioni marcano solo i casi in cui **la condizione non puo'
cadere da sola** — una riga battuta e mai inviata, un pannello che non si riesce
a leggere, un invio rifiutato. Quello che passa col tempo resta cronaca e non
arriva qui. Un rinvio che si ripete su una condizione che non scade non e' un
rinvio: e' un blocco.

**Come l'ho aggirato**: niente da aggirare, nessuno era bloccato mentre questo
succedeva — ed e' esattamente il problema. L'automazione continua a girare e a
non concludere, e senza questa voce non se ne accorgerebbe nessuno.

**Dove guardare**: il registro di `{source}`, `{log_path}`. Per vederle tutte:

    grep -F '[guasto={name}]' {log_path} | grep -F '{subject}' | tail -20

**Quando e' chiusa**: si mette `stato: chiusa` come per ogni altra voce. Non
rinasce sulle righe vecchie — chi la riapre conta solo cio' che il registro
scrive **dopo** la chiusura.
"
    )
}

/// La tabella dei registri in servizio. I due segnaposto li espande chi conosce
/// la casa: qui non si legge l'ambiente, perché un giudizio che dipende
/// dall'ambiente non si prova.
///
/// L'ULTIMA RIGA È IL PROPRIO REGISTRO, e non è un vezzo: le spie di questo
/// programma finivano in un file che nessuno legge, e chi lo lancia butta via il
/// suo stderr quando l'uscita è zero. Nessun ciclo — qui contano solo le righe
/// marcate, e le sue righe di cronaca non lo sono.
pub const DEFAULT_REGISTERS: &str = "\
{HOME}/.claude/state/staffetta.log|staffetta|local|30|1440|24
{HOME}/.local/state/guardiano-macchina.log|guardiano-macchina|local|30|48|48
{HOME}/.claude/state/nine-am-wakeup.log|nine-am-wakeup|utc|2|1|720
{OWN_LOG}|deposito-guasti|local|3|144|24";

/// I tetti in servizio.
///
/// LE DUE RIGHE DELLA RESA STANNO A UNO, e non è una soglia debole: quel nome
/// compare **solo** quando la staffetta ha smesso di provare. Non è una
/// ripetizione da contare, è l'annuncio che qualcosa è fermo. I tetti vengono
/// dalle costanti della staffetta e vanno rifatti a mano se quelle cambiano.
pub const DEFAULT_CAPS: &str = "\
staffetta|pannello-non-letto|3|11
staffetta|pannello-non-identificato|3|11
staffetta|riga-battuta-mai-inviata|3|11
staffetta|turno-non-letto|3|11
staffetta|clear-non-inviato|3|11
staffetta|rigenerazione-non-confermata|3|11
staffetta|rinvio-senza-scadenza|1|3
staffetta|staffetta-cieca|1|3
staffetta|avvio-abbandonato|2|24";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_overlap::{is_closed, state_word};

    fn caps(text: &str) -> Vec<CapRow> {
        parse_caps(text)
    }

    /// Il caso per cui la tabella dei tetti esiste: tre righe bastano dove il
    /// registro ne chiederebbe trenta.
    #[test]
    fn a_fault_with_its_own_cap_fires_below_the_register_threshold() {
        let t = caps("staffetta|pannello-non-letto|3|11");
        assert_eq!(
            threshold_for(&t, "staffetta", "pannello-non-letto", 30, None),
            3
        );
    }

    /// Il caso che rende valido il primo: chi non ha una soglia propria tiene
    /// quella del registro.
    #[test]
    fn a_fault_with_no_cap_keeps_the_register_threshold() {
        let t = caps("staffetta|pannello-non-letto|3|11");
        assert_eq!(
            threshold_for(&t, "staffetta", "avvio-non-marcato", 30, None),
            30
        );
    }

    /// La forzatura del collaudo batte tutte e due le tabelle.
    #[test]
    fn the_forced_threshold_beats_both_tables() {
        let t = caps("staffetta|pannello-non-letto|3|11");
        assert_eq!(
            threshold_for(&t, "staffetta", "pannello-non-letto", 30, Some(20)),
            20
        );
    }

    /// Una riga a cui manca il tetto non vale, e la sua soglia **non si
    /// applica**: era la forma che passava il controllo con i due campi
    /// concatenati.
    #[test]
    fn a_row_without_its_cap_is_reported_and_not_applied() {
        let t = caps("staffetta|pannello-non-letto|40");
        assert!(matches!(t[0], CapRow::Malformed { .. }));
        assert_eq!(
            threshold_for(&t, "staffetta", "pannello-non-letto", 30, None),
            30,
            "una riga che non si capisce non decide una soglia"
        );
        let alarms = cap_table_alarms(&t);
        assert_eq!(alarms.len(), 1);
        assert!(alarms[0].warning.contains("MALFORMED cap row"));
    }

    /// Una soglia vuota è malformata quanto un tetto mancante.
    #[test]
    fn an_empty_threshold_is_malformed_too() {
        let t = caps("staffetta|pannello-non-letto||12");
        assert!(cap_table_alarms(&t)[0].warning.contains("MALFORMED"));
    }

    /// Cinque campi non sono quattro più una nota: la riga non si capisce, e i
    /// due giudizi che ne dipendevano restano d'accordo perché il lettore è uno.
    #[test]
    fn a_five_field_row_is_reported_and_not_applied() {
        let t = caps("staffetta|pannello-non-letto|40|100|nota");
        assert!(cap_table_alarms(&t)[0].warning.contains("MALFORMED"));
        assert_eq!(
            threshold_for(&t, "staffetta", "pannello-non-letto", 30, None),
            30
        );
    }

    /// UNA RIGA MALFORMATA NON PRODUCE NEMMENO IL SECONDO ALLARME. La versione
    /// shell leggeva la tabella una seconda volta guardando solo la soglia:
    /// `staffetta|x|40` senza tetto usciva **anche** come RAISED, cioè un
    /// giudizio su una riga appena dichiarata incomprensibile. Nessun braccio
    /// della batteria shell lo copriva: questa prova fissa la scelta.
    #[test]
    fn a_malformed_row_is_not_judged_twice() {
        let t = caps("staffetta|pannello-non-letto|40");
        assert!(
            raised_threshold_alarms(&t, "staffetta", 30, None).is_empty(),
            "una riga malformata è già stata giudicata una volta"
        );
    }

    /// Una soglia sopra il proprio tetto è muta, e lo dice.
    #[test]
    fn a_threshold_above_its_own_cap_is_reported() {
        let over = caps("staffetta|pannello-non-letto|20|12");
        assert!(cap_table_alarms(&over)[0].warning.contains("UNREACHABLE"));
        // il caso che rende valido il primo
        let sane = caps("staffetta|pannello-non-letto|3|11");
        assert!(cap_table_alarms(&sane).is_empty());
    }

    /// L'altra mano: il tetto è coerente, quindi il controllo di sopra tace. Se
    /// ne accorge solo chi confronta con la soglia del registro.
    #[test]
    fn a_fault_threshold_above_the_register_one_is_reported() {
        let t = caps("staffetta|pannello-non-letto|40|100");
        assert!(cap_table_alarms(&t).is_empty(), "il tetto qui è coerente");
        let raised = raised_threshold_alarms(&t, "staffetta", 30, None);
        assert_eq!(raised.len(), 1);
        assert!(raised[0].warning.contains("RAISED threshold"));
    }

    /// La bugia del collaudo non inventa allarmi contro se stessa.
    #[test]
    fn the_forced_threshold_does_not_invent_alarms() {
        let t = caps("staffetta|pannello-non-letto|3|11");
        assert!(raised_threshold_alarms(&t, "staffetta", 1, Some(1)).is_empty());
    }

    /// Una soglia che i giri del registro non raggiungono non è severa: è muta.
    #[test]
    fn a_register_threshold_above_its_own_turns_is_reported() {
        let daily = Register {
            path: "/x".into(),
            source: "nine-am-wakeup".into(),
            zone: Zone::Utc,
            threshold: 30,
            per_day: 1,
            window_h: 24,
        };
        let alarm = unreachable_register_alarm(&daily).expect("un giro al giorno non fa trenta");
        assert!(alarm.note.contains("it can never fire"));
        // e il caso che lo rende valido
        let sane = Register {
            threshold: 2,
            window_h: 720,
            ..daily
        };
        assert!(unreachable_register_alarm(&sane).is_none());
    }

    /// Una riga di tabella malformata ferma quel registro invece di passargli
    /// una soglia vuota.
    #[test]
    fn a_short_register_row_stops_that_register() {
        let rows = parse_registers("/x|staffetta|local\n/y|altro|local|30|1440|24");
        assert!(matches!(rows[0], RegisterRow::Malformed { .. }));
        assert!(matches!(rows[1], RegisterRow::Ok(_)));
    }

    /// Uno zero non è un numero valido: una soglia di zero aprirebbe una voce su
    /// nessuna ripetizione. E `03` non è tre: è una battitura.
    #[test]
    fn zero_is_not_a_threshold_and_neither_is_a_padded_number() {
        for row in [
            "/x|staffetta|local|0|1440|24",
            "/x|staffetta|local|03|1440|24",
            "/x|staffetta|local|30|1440|00",
        ] {
            assert!(
                matches!(parse_registers(row)[0], RegisterRow::Malformed { .. }),
                "{row}"
            );
        }
    }

    /// La riga malformata porta i tre numeri **come sono scritti**: quel testo
    /// finisce citato dentro una voce di coda, e chi la legge deve vedere la riga
    /// di chi l'ha sbagliata.
    #[test]
    fn a_malformed_register_row_carries_the_fields_as_written() {
        let rows = parse_registers("/x|staffetta|local||1440|24");
        let RegisterRow::Malformed {
            source,
            path,
            threshold,
            per_day,
            window_h,
        } = &rows[0]
        else {
            panic!("una riga senza soglia non si capisce");
        };
        assert_eq!((source.as_str(), path.as_str()), ("staffetta", "/x"));
        assert_eq!((threshold.as_str(), per_day.as_str(), window_h.as_str()), ("", "1440", "24"));
        assert_eq!(rows[0].path(), "/x", "il percorso si legge comunque");
    }

    /// Due numeri assurdi in una tabella non fanno morire il programma: il conto
    /// satura invece di traboccare.
    #[test]
    fn absurd_turns_saturate_instead_of_overflowing() {
        let r = Register {
            path: "/x".into(),
            source: "s".into(),
            zone: Zone::Local,
            threshold: 1,
            per_day: u32::MAX,
            window_h: u32::MAX,
        };
        assert_eq!(r.turns(), u32::MAX);
    }

    // ── L'estrattore ────────────────────────────────────────────────────────

    const LOG: &str = "\
2026-08-24 21:00:00 [guasto=pannello-non-letto] sess=abc12345 stuck
2026-08-24 21:01:00 [guasto=pannello-non-letto] sess=abc12345 stuck again
2026-08-24 21:02:00 [guasto=pannello-non-letto] sess=ff00 another session
2026-08-24 21:03:00 la ronda ha girato, tutto a posto
questa riga non comincia con una data e non si conta
2026-08-24 21:04:00 [guasto=turno-non-letto] term_9f-3b handle";

    #[test]
    fn only_marked_lines_are_counted_and_the_subject_splits_them() {
        let g = extract(LOG, "2026-01-01T00:00:00");
        assert_eq!(g.len(), 3, "due sessioni e un pannello, la cronaca fuori");
        let stuck = g
            .iter()
            .find(|x| x.subject == "sess=abc12345")
            .expect("il gruppo della prima sessione");
        assert_eq!(stuck.count, 2);
        assert_eq!(stuck.first, "2026-08-24T21:00:00");
        assert_eq!(stuck.last, "2026-08-24T21:01:00");
        assert!(stuck.text.ends_with("stuck again"), "l'ultima riga esatta");
        assert!(g.iter().any(|x| x.subject == "term_9f-3b"));
    }

    /// Il taglio è la metà che rende utile il conteggio: una voce appena chiusa
    /// non deve rinascere sulle righe vecchie.
    #[test]
    fn the_cutoff_leaves_the_older_lines_out() {
        let g = extract(LOG, "2026-08-24T21:02:00");
        assert_eq!(g.len(), 2);
        assert_eq!(
            g.iter().find(|x| x.subject == "sess=ff00").unwrap().count,
            1
        );
        assert!(g.iter().all(|x| x.subject != "sess=abc12345"));
    }

    /// Un registro che scrive in UTC mette la sigla in mezzo, e il corpo comincia
    /// dopo quella.
    #[test]
    fn a_utc_register_line_is_read_past_its_marker() {
        let g = extract(
            "2026-08-24T21:00:00 UTC [guasto=sveglia-non-suonata] nothing",
            "2026-01-01T00:00:00",
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "sveglia-non-suonata");
        assert_eq!(
            g[0].subject, "-",
            "senza sessione il guasto è dell'automazione"
        );
    }

    /// IL SOGGETTO È LA CHIAVE, e le due regole vanno prese alla lettera. I tre
    /// casi che distinguono una lettura dall'altra, tutti e tre misurati sulla
    /// versione shell prima di scriverli qui: un identificativo maiuscolo non è
    /// un soggetto; un `sess=` che non regge non nasconde quello buono più
    /// avanti; un pannello tiene i trattini.
    #[test]
    fn the_subject_is_read_the_way_the_key_was_always_built() {
        let subject = |line: &str| {
            extract(line, "2026-01-01T00:00:00")
                .first()
                .map(|g| g.subject.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            subject("2026-08-24 21:00:00 [guasto=x] sess=ABC12345 stuck"),
            "-",
            "maiuscolo non è un soggetto riconosciuto"
        );
        assert_eq!(
            subject("2026-08-24 21:00:00 [guasto=x] sess=zzz poi sess=abc12345"),
            "sess=abc12345",
            "un sess= che non regge non nasconde quello buono"
        );
        assert_eq!(
            subject("2026-08-24 21:00:00 [guasto=x] term_9f-3b handle"),
            "term_9f-3b"
        );
    }

    /// La marcatura è kebab-case: allargarla farebbe entrare righe di cronaca
    /// con una parentesi quadra dentro.
    #[test]
    fn a_bracket_that_is_not_a_marker_does_not_count() {
        assert!(extract("2026-08-24 21:00:00 [INFO] tutto bene", "2026-01-01T00:00:00").is_empty());
        assert!(extract(
            "2026-08-24 21:00:00 [guasto=Panne Grosse] x",
            "2026-01-01T00:00:00"
        )
        .is_empty());
    }

    // ── I testi ─────────────────────────────────────────────────────────────

    /// Il confine dei due `---`: fuori dall'intestazione non si tocca niente,
    /// nemmeno una riga del corpo che comincia con la stessa parola.
    #[test]
    fn the_update_stays_inside_the_header() {
        let entry = "---\nstato: aperta\nripetizioni: 1\nsoglia: 99\nultima: 2026-01-01 00:00\n---\n\nripetizioni: questa riga e del corpo\n";
        let out = update_header(entry, 4, "2026-08-24T21:00:00", 3);
        assert!(out.contains("\nripetizioni: 4\n"));
        assert!(out.contains("\nsoglia: 3\n"));
        assert!(out.contains("\nultima: 2026-08-24T21:00:00\n"));
        assert!(
            out.contains("ripetizioni: questa riga e del corpo"),
            "il corpo resta intatto"
        );
    }

    /// Una voce riaperta porta la soglia viva in testa **e** nel racconto del
    /// ritorno: chi legge un conteggio e una soglia che non tornano non sa a
    /// quale dei due credere. E lo stato nuovo deve restare leggibile al
    /// selettore della coda, che guarda la prima parola.
    #[test]
    fn a_reopened_entry_carries_the_live_threshold_and_stays_readable() {
        let entry = "---\nstato: chiusa\nripetizioni: 1\nsoglia: 99\nprima: 2026-01-01 00:00\nultima: 2026-01-01 00:00\nritorni: 0\n---\n\nCorpo.\n";
        assert!(is_closed(&state_word(entry).unwrap()));
        assert_eq!(returns_of(entry), 0);
        let out = reopen_header(
            entry,
            4,
            "2026-08-24T21:00:00",
            "2026-08-24T21:03:00",
            3,
            1,
            "2026-08-24 21:05",
        );
        assert!(out.contains("stato: aperta — riaperta il 2026-08-24 21:05"));
        assert!(out.contains("\nsoglia: 3\n"));
        assert!(out.contains("\nritorni: 1\n"));
        assert_eq!(
            state_word(&out).as_deref(),
            Some("aperta"),
            "chi sceglie chi svegliare legge la prima parola"
        );
        assert_eq!(returns_of(&out), 1);
    }

    /// Il tetto arriva al lettore invece di restare nella tabella: «3 su 1440»
    /// sembra larghissima, «3 su un tetto di 11» dice la cosa vera.
    #[test]
    fn the_entry_names_the_cap_it_is_measured_against() {
        let note = cap_note(Some(11));
        let facts = EntryFacts {
            key: "staffetta/pannello-non-letto/sess=abc",
            source: "staffetta",
            name: "pannello-non-letto",
            subject: "sess=abc",
            count: 3,
            threshold: 3,
            cap_note: &note,
            first: "2026-08-24T21:00:00",
            last: "2026-08-24T21:02:00",
            text: "2026-08-24 21:02:00 [guasto=pannello-non-letto] sess=abc stuck",
            log_path: "/tmp/staffetta.log",
            per_day: 1440,
            window_h: 24,
            turns: 1440,
            when: "2026-08-24 21:05:00",
        };
        let entry = new_entry(&facts);
        assert!(entry.contains("tetto di 11 al giorno"));
        assert!(entry.contains("\nsoglia: 3\n"));
        assert!(entry.contains("\nripetizioni: 3\n"));
        assert!(entry.contains(&key_line(facts.key)));
        assert_eq!(state_word(&entry).as_deref(), Some("aperta"));
        // e senza tetto la coda non compare affatto
        assert!(cap_note(None).is_empty());
    }

    #[test]
    fn the_return_note_states_the_threshold_in_full() {
        let note = return_note(
            4,
            3,
            &cap_note(Some(11)),
            "2026-08-24T21:00:00",
            "2026-08-24T21:03:00",
            "riga",
            "2026-08-24 21:05",
            2,
        );
        assert!(note.contains("soglia di **3**"));
        assert!(note.contains("ritorno numero 2"));
    }

    /// Il nome del file dice di cosa parla senza aprirlo, e non porta dentro un
    /// carattere che il filesystem legge come percorso.
    #[test]
    fn the_file_name_carries_the_subject_without_its_punctuation() {
        assert_eq!(
            file_stem("2026-08-24", "staffetta", "pannello-non-letto", "sess=ab12"),
            "2026-08-24-guasto-staffetta-pannello-non-letto-sess-ab12"
        );
        assert_eq!(
            file_stem("2026-08-24", "staffetta", "staffetta-cieca", "-"),
            "2026-08-24-guasto-staffetta-staffetta-cieca"
        );
        assert!(!file_stem("2026-08-24", "s", "n", "a/b").contains('/'));
    }

    /// La tabella di serie sorveglia il registro di questo stesso programma: è
    /// la riga che gli impedisce di essere l'unico a gridare davanti a nessuno.
    #[test]
    fn the_default_table_watches_this_program_own_register() {
        let rows = parse_registers(
            &DEFAULT_REGISTERS
                .replace("{HOME}", "/home/x")
                .replace("{OWN_LOG}", "/home/x/.claude/state/deposito-guasti.log"),
        );
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| matches!(r, RegisterRow::Ok(_))));
        let own = rows
            .iter()
            .filter_map(|r| match r {
                RegisterRow::Ok(v) => Some(v),
                _ => None,
            })
            .find(|r| r.source == "deposito-guasti")
            .expect("il proprio registro sta nella tabella di serie");
        assert_eq!(own.threshold, 3);
    }

    /// I tetti di serie devono essere tutti raggiungibili: una tabella che
    /// spegne una spia da sé è il difetto che la tabella esiste per chiudere.
    #[test]
    fn every_shipped_cap_can_actually_fire() {
        let rows = parse_caps(DEFAULT_CAPS);
        assert_eq!(rows.len(), 9);
        assert!(
            cap_table_alarms(&rows).is_empty(),
            "{:?}",
            cap_table_alarms(&rows)
        );
        // e nessuna di esse alza la soglia del registro della staffetta
        assert!(raised_threshold_alarms(&rows, "staffetta", 30, None).is_empty());
    }
}
