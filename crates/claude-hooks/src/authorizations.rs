//! Il registro delle autorizzazioni: distingue una decisione vera di Theo da
//! una riferita da un pari, senza che chi esegue debba chiedere a nessuno.
//!
//! Nasce dalla segnalazione del 21/08/2026
//! (`state/plancia/segnalazioni/2026-08-21-la-catena-delle-autorizzazioni-si-e-rotta.md`):
//! la catena — sessione chiede, capitano valuta, Theo decide, capitano scrive,
//! macchinista esegue — è stata saltata perché il pezzo che la rende
//! eseguibile, il registro, non esisteva. La procedura sta in
//! `docs/procedura-autorizzazioni.md`; qui sta solo la lettura.
//!
//! LA TRAPPOLA CHE QUESTO MODULO ESISTE PER EVITARE: un registro **assente**
//! (il file non si apre) e un registro **vuoto** (il file c'è, zero righe)
//! devono rispondere in modo diverso. Il primo vuol dire «non lo so, fermati»;
//! il secondo vuol dire «nessuna autorizzazione, e il controllo ha funzionato
//! davvero». Confonderli autorizzerebbe tutto il giorno in cui il file sparisce.
//!
//! **La penna**, aggiunta il 21/08/2026: `captain-authorize` scrive una riga.
//! La scrive SOLO il capitano — il nome del comando lo dice apposta. Una
//! riga non si tocca mai dopo: per correggere una decisione se ne aggiunge
//! un'altra che la revoca (`revoked: true`), e fra più righe sulla stessa
//! chiave vince quella scritta per ultima. La procedura resta in
//! `docs/procedura-autorizzazioni.md`.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Una riga del registro: le sei informazioni che la procedura chiede, più la
/// chiave stabile su cui si cerca. I valori restano come scritti dal
/// capitano — italiano compreso — perché `theo_said` è una trascrizione alla
/// lettera, non un riassunto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRecord {
    pub key: String,
    pub when: String,
    pub requested_by: String,
    pub request: String,
    pub theo_said: String,
    pub evaluated_by: String,
    pub executor: String,
    /// `true` se questa riga revoca una precedente autorizzazione sulla
    /// stessa chiave, invece di concederne una. Assente nelle righe vecchie
    /// (`parse_line` lo legge come `false`), quindi non serve riscriverle.
    pub revoked: bool,
}

/// I tre esiti, mai confusi fra loro: `Unreadable` non è un `NotAuthorized`
/// con un motivo in più, è un ramo diverso — chi legge questo tipo non può
/// scambiarli per distrazione, come invece capita con un booleano o un
/// `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Authorized(AuthorizationRecord),
    NotAuthorized,
    Unreadable,
}

/// Interpreta una riga come record. `None` se non è un oggetto JSON o gli
/// manca la chiave — senza la chiave la riga non è cercabile, quindi non è
/// utilizzabile. Gli altri cinque campi mancanti restano vuoti piuttosto che
/// far cadere l'intera riga: un registro scritto a mano con un campo
/// dimenticato non deve nascondere le altre autorizzazioni che contiene.
fn parse_line(line: &str) -> Option<AuthorizationRecord> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let field = |name: &str| {
        v.get(name)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let key = v.get("key").and_then(|x| x.as_str())?.to_string();
    let revoked = v.get("revoked").and_then(|x| x.as_bool()).unwrap_or(false);
    Some(AuthorizationRecord {
        key,
        when: field("when"),
        requested_by: field("requested_by"),
        request: field("request"),
        theo_said: field("theo_said"),
        evaluated_by: field("evaluated_by"),
        executor: field("executor"),
        revoked,
    })
}

/// La domanda pura: dato il contenuto del registro (o l'indicazione che non
/// si è potuto leggere) e la chiave cercata, quale dei tre esiti vale.
///
/// `registry: None` è «il file non si apre o non esiste»: risponde
/// `Unreadable` prima ancora di guardare la chiave, perché non c'è niente da
/// guardare. `Some("")` (il file esiste, zero righe) scende invece nel ciclo,
/// non trova mai una corrispondenza e risponde `NotAuthorized` — è la
/// distinzione che questo modulo esiste per tenere separata.
///
/// Le righe malformate non fermano la ricerca né vengono taciute: finiscono
/// nel secondo elemento della coppia, così chi chiama può avvisarne senza che
/// una riga rotta nasconda una riga buona più avanti nel file.
///
/// Il registro è append-only e cronologico: la riga più recente su una
/// chiave è l'ultima che la cita, non la prima. Per questo qui si scorre
/// **tutto** il file tenendo l'ultimo match, invece di fermarsi al primo —
/// è quello che fa vincere una correzione successiva sulla decisione
/// vecchia, ed è lo stesso meccanismo con cui una revoca (`revoked: true`)
/// spegne un'autorizzazione precedente: se l'ultima riga sulla chiave è una
/// revoca, il verdetto è `NotAuthorized` anche se righe più vecchie
/// autorizzavano.
pub fn check(registry: Option<&str>, key: &str) -> (Verdict, Vec<String>) {
    let Some(content) = registry else {
        return (Verdict::Unreadable, Vec::new());
    };
    let mut warnings = Vec::new();
    let mut latest: Option<AuthorizationRecord> = None;
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            // Confronto esatto, non `starts_with`/`contains`: un'autorizzazione
            // stretta non deve coprirne una larga che le somiglia solo nel testo.
            Some(record) if record.key == key => latest = Some(record),
            Some(_) => {}
            None => warnings.push(format!("riga {}: non è un record valido", i + 1)),
        }
    }
    let verdict = match latest {
        Some(record) if record.revoked => Verdict::NotAuthorized,
        Some(record) => Verdict::Authorized(record),
        None => Verdict::NotAuthorized,
    };
    (verdict, warnings)
}

/// Il percorso del registro. La cartella base si inietta per le prove, stessa
/// convenzione di `hook_census`/`reachability`: senza, un test che vuole
/// vedere «registro assente» dovrebbe cancellare il file vero.
fn registry_path() -> PathBuf {
    let home = std::env::var_os("AUTORIZZAZIONI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/theo/.claude"));
    home.join("state").join("autorizzazioni.jsonl")
}

/// La sola lettura da disco: `Ok` con qualunque contenuto (anche vuoto)
/// diventa `Some`, ogni errore (file assente, permessi, altro) diventa
/// `None`. È qui, e non nella funzione pura sopra, che «non esiste» e «non si
/// apre» si fondono nello stesso `Unreadable` — la distinzione che conta è
/// leggibile/non leggibile, non la causa dell'errore.
fn read_registry(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn print_record(key: &str, record: &AuthorizationRecord) {
    println!("AUTORIZZATA: {key}");
    println!("  quando:         {}", record.when);
    println!("  chi ha chiesto: {}", record.requested_by);
    println!("  cosa:           {}", record.request);
    println!("  Theo ha detto:  «{}»", record.theo_said);
    println!("  valutata da:    {}", record.evaluated_by);
    println!("  esegue:         {}", record.executor);
}

/// Sottocomando `authorization-check <chiave>`, per chi esegue e non deve
/// chiedere a nessuno. Uscita 0 = autorizzata, 1 = non autorizzata, 3 = non
/// si sa (registro illeggibile) — tre codici diversi perché confondere
/// l'ultimo col secondo è esattamente il difetto per cui questo strumento
/// esiste: un registro che sparisce non deve leggersi come «via libera».
pub fn run() -> i32 {
    let Some(key) = std::env::args().nth(2) else {
        eprintln!("uso: claude-hooks authorization-check <chiave>");
        return 64;
    };
    let path = registry_path();
    let registry = read_registry(&path);
    let (verdict, warnings) = check(registry.as_deref(), &key);
    for w in &warnings {
        eprintln!("autorizzazioni: {w}, ignorata");
    }
    match verdict {
        Verdict::Authorized(record) => {
            print_record(&key, &record);
            0
        }
        Verdict::NotAuthorized => {
            println!("NON AUTORIZZATA: nessuna riga del registro corrisponde a «{key}».");
            1
        }
        Verdict::Unreadable => {
            eprintln!(
                "NON SO SE È AUTORIZZATA: il registro ({}) non esiste o non si apre. \
Non è \"non autorizzata\": è \"non si può dire\". Fermati, non eseguire, passa dal capitano.",
                path.display()
            );
            3
        }
    }
}

// ---------------------------------------------------------------------
// La penna. Da qui in giù scrive solo il capitano — vedi il commento sul
// sottocomando `run_write` per il perché di ogni scelta.
// ---------------------------------------------------------------------

/// Compone la trascrizione quando Theo ha deciso scegliendo da un modulo
/// invece che a parole libere: non un riassunto, ma la domanda posta più
/// l'etichetta dell'opzione scelta — pura, per poterla provare senza disco.
fn module_choice_to_theo_said(question: &str, option_label: &str) -> String {
    format!("(dal modulo) domanda: {question} — scelta: «{option_label}»")
}

/// Costruisce il record da scrivere. Pura: nessun I/O, nessun orologio —
/// `when` arriva già calcolato da chi chiama, così la funzione che decide
/// *cosa* scrivere resta provabile senza toccare il disco o l'ora di sistema.
#[allow(clippy::too_many_arguments)]
fn build_record(
    key: &str,
    when: &str,
    requested_by: &str,
    request: &str,
    theo_said: &str,
    evaluated_by: &str,
    executor: &str,
    revoked: bool,
) -> AuthorizationRecord {
    AuthorizationRecord {
        key: key.to_string(),
        when: when.to_string(),
        requested_by: requested_by.to_string(),
        request: request.to_string(),
        theo_said: theo_said.to_string(),
        evaluated_by: evaluated_by.to_string(),
        executor: executor.to_string(),
        revoked,
    }
}

/// La riga JSON da scrivere. `revoked` compare solo quando `true`, così le
/// righe di sola concessione restano nella stessa forma già documentata nel
/// README e lette da `parse_line` (che lo dà per `false` quando manca).
/// L'ordine dei campi è quello di lettura: il crate ha `preserve_order`
/// attivo apposta, per chi apre il file a mano.
fn record_to_line(record: &AuthorizationRecord) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("key".into(), record.key.clone().into());
    obj.insert("when".into(), record.when.clone().into());
    obj.insert("requested_by".into(), record.requested_by.clone().into());
    obj.insert("request".into(), record.request.clone().into());
    obj.insert("theo_said".into(), record.theo_said.clone().into());
    obj.insert("evaluated_by".into(), record.evaluated_by.clone().into());
    obj.insert("executor".into(), record.executor.clone().into());
    if record.revoked {
        obj.insert("revoked".into(), true.into());
    }
    serde_json::Value::Object(obj).to_string()
}

/// Aggiunge una riga senza poterne lasciare una a metà. Non si scrive in
/// append diretto: si compone l'intero nuovo contenuto (il vecchio più la
/// riga nuova) su un file temporaneo nella stessa cartella, lo si
/// sincronizza su disco, e solo allora lo si rinomina sopra il registro — una
/// `rename` fra file dello stesso volume è atomica, quindi il registro finale
/// è o tutto il vecchio contenuto o tutto il nuovo, mai una via di mezzo.
/// Costa riscrivere l'intero file a ogni riga: accettabile per un registro
/// locale di poche righe, in cambio della garanzia che conta. Se un passo
/// fallisce (cartella non creabile, disco pieno, permessi) il registro
/// originale resta intatto e l'errore risale a chi ha chiamato.
/// Il lucchetto accanto al registro, tenuto per tutta la durata di una
/// scrittura. Serve perché `append_record` non aggiunge: legge tutto,
/// ricompone e rinomina sopra — quindi due scritture che si sovrappongono si
/// cancellano a vicenda, **senza errore e con uscita zero**.
///
/// `create_new` è l'unica esclusione che funziona anche fra processi diversi
/// senza portarsi dietro una libreria: fallisce se il file c'è già, e la
/// creazione è atomica per il sistema. `Drop` lo toglie anche se chi scrive
/// va in panico a metà.
///
/// IL LUCCHETTO ABBANDONATO è il difetto che ogni lucchetto si porta dietro:
/// un processo ucciso fra la presa e il rilascio bloccherebbe tutti per sempre,
/// quindi dopo l'attesa massima si forza. Ma **si forza solo il lucchetto che si
/// sta guardando da dieci secondi**, e questo è il pezzo che regge tutto.
///
/// PERCHÉ, misurato da un verdetto su processi veri: due scrittori che aspettano
/// lo stesso lucchetto abbandonato arrivano a scadenza a pochi millisecondi di
/// distanza. Il primo forza e ne crea uno suo, legittimo; il secondo — scaduto
/// anche lui — trovava «occupato» e **forzava il lucchetto vivo del primo**. Da
/// lì i due tornavano senza esclusione, e chi rinominava per ultimo cancellava la
/// riga dell'altro: **due `AUTORIZZATA` stampati, due uscite zero, una riga sola
/// su disco.** Due volte su dodici tentativi reali.
///
/// La cura è guardare *quale* lucchetto: se il contenuto è cambiato mentre si
/// aspettava, qualcuno l'ha preso adesso e l'attesa riparte da capo. Si forza
/// solo ciò che è rimasto immobile per tutta la finestra.
struct RegistryLock {
    path: PathBuf,
    /// Il proprio nome dentro il lucchetto: unico per processo **e** per
    /// chiamata, perché due thread condividono il numero di processo.
    token: String,
}

/// Distingue due gesti dello stesso processo — il nome del file temporaneo e il
/// nome dentro il lucchetto. Sta qui e non dentro una funzione perché lo usano
/// in due, e due contatori separati darebbero gli stessi numeri.
static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Quanto si aspetta prima di considerare abbandonato un lucchetto immobile.
/// Dev'essere più lungo della sezione critica, che qui è una lettura, una
/// scrittura e una rinomina su un file di poche righe.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

impl RegistryLock {
    fn acquire(registry: &Path) -> std::io::Result<Self> {
        Self::acquire_waiting(registry, LOCK_WAIT)
    }

    /// L'attesa è un parametro **solo** perché la corsa fra due che forzano si
    /// possa provare in un decimo di secondo invece che in venti: una prova che
    /// costa venti secondi non la lancia più nessuno, e allora non prova niente.
    fn acquire_waiting(registry: &Path, wait: std::time::Duration) -> std::io::Result<Self> {
        let dir = registry.parent().unwrap_or_else(|| Path::new("."));
        let name = registry
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "autorizzazioni".to_string());
        let path = dir.join(format!(".{name}.lock"));
        let token = format!(
            "{}.{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut deadline = std::time::Instant::now() + wait;
        let mut seen = std::fs::read_to_string(&path).ok();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(token.as_bytes())?;
                    file.sync_all()?;
                    return Ok(Self { path, token });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let now = std::fs::read_to_string(&path).ok();
                    if now != seen {
                        // Non è più il lucchetto che stavo aspettando: qualcuno
                        // l'ha preso adesso, e il suo tempo comincia ora.
                        //
                        // QUESTO RAMO NON È COPERTO DA UNA PROVA, e va detto
                        // invece di lasciarlo credere coperto: la prova
                        // `two_forcers_do_not_both_get_in` resta verde anche
                        // togliendolo, perché il caso che riproduce lo chiude
                        // già la riga di sotto. Serve per lo scenario diverso —
                        // il lucchetto cambia padrone *mentre* aspetto — e chi
                        // ci mette mano lo verifichi prima di fidarsi.
                        seen = now;
                        deadline = std::time::Instant::now() + wait;
                    } else if std::time::Instant::now() >= deadline {
                        // SI FORZA, E SI RIPARTE CON L'ATTESA PIENA. È la riga
                        // che chiude la corsa misurata: senza, chi ha appena
                        // forzato ha il tempo già scaduto e forza di nuovo un
                        // istante dopo — stavolta il lucchetto **vivo** di chi
                        // gli è arrivato davanti.
                        let _ = std::fs::remove_file(&path);
                        deadline = std::time::Instant::now() + wait;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for RegistryLock {
    /// SI TOGLIE IL PROPRIO LUCCHETTO, non il file che sta lì.
    ///
    /// Prima si rimuoveva il percorso e basta: se nel frattempo il lucchetto era
    /// stato forzato e ricreato da un altro, questo `Drop` **cancellava il
    /// lucchetto vivo di quell'altro** e rompeva l'esclusione senza che nessuno
    /// se ne accorgesse. Riprodotto in modo deterministico da un verdetto con
    /// uno scrittore lento. Il nome dentro il file c'era già e non lo rileggeva
    /// nessuno: era una diligenza apparente.
    fn drop(&mut self) {
        match std::fs::read_to_string(&self.path) {
            Ok(content) if content == self.token => {
                let _ = std::fs::remove_file(&self.path);
            }
            // Non è più mio: chi ce l'ha adesso lo toglierà da sé.
            _ => {}
        }
    }
}

fn append_record(path: &Path, record: &AuthorizationRecord) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    // Preso prima della lettura, tenuto fino dopo la rinomina: è la sequenza
    // intera a dover essere indivisibile, non i singoli passi.
    let _lock = RegistryLock::acquire(path)?;
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&record_to_line(record));
    new_content.push('\n');

    // Il contatore rende unico il NOME DEL TEMPORANEO, e solo quello. Da solo
    // basta: `fetch_add` non restituisce mai due volte lo stesso valore, e
    // numero di processo e istante restano per leggibilità. L'istante da solo
    // non bastava: due thread che ci arrivano nello stesso nanosecondo
    // ottengono lo stesso nome, e chi rinomina per primo porta via il
    // temporaneo dell'altro.
    //
    // Due scritture sullo stesso registro le tiene separate il lucchetto qui
    // sopra, non questo nome: prima che ci fosse, chi rinominava per ultimo
    // cancellava la riga dell'altro senza errore — 50 righe perse su 100,
    // misurate tre volte con lo stesso esito.
    let tmp_path = dir.join(format!(
        ".autorizzazioni.{}.{}.{}.tmp",
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut tmp = std::fs::File::create(&tmp_path)?;
        tmp.write_all(new_content.as_bytes())?;
        tmp.sync_all()
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    std::fs::rename(&tmp_path, path)
}

/// L'ora corrente in ISO 8601 col fuso locale (`+02:00`, non `+0200`), presa
/// dal comando di sistema invece che da una libreria di fusi orari portata
/// solo per questo. `--when` in `captain-authorize` la scavalca.
fn now_iso8601() -> String {
    let out = std::process::Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%z")
        .output();
    let raw = out
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let raw = raw.trim();
    if raw.len() > 5 {
        let (dt, offset) = raw.split_at(raw.len() - 5);
        if offset.len() == 5 {
            return format!("{dt}{}:{}", &offset[0..3], &offset[3..5]);
        }
    }
    raw.to_string()
}

/// Gli argomenti nominali di `captain-authorize`, letti da un vettore
/// (non da `std::env::args()` direttamente) per poter provare il parsing
/// senza un processo vero. Niente `clap`, stessa scelta del resto del
/// binario: qui i flag sono un elenco chiuso.
#[derive(Debug, Default, PartialEq, Eq)]
struct WriteArgs {
    key: Option<String>,
    requested_by: Option<String>,
    request: Option<String>,
    executor: Option<String>,
    theo_said: Option<String>,
    question: Option<String>,
    option_label: Option<String>,
    evaluated_by: Option<String>,
    when: Option<String>,
    revoke: bool,
}

fn parse_write_args(argv: &[String]) -> WriteArgs {
    let mut out = WriteArgs::default();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "--revoke" {
            out.revoke = true;
            i += 1;
            continue;
        }
        let Some(value) = argv.get(i + 1) else {
            break;
        };
        match argv[i].as_str() {
            "--key" => out.key = Some(value.clone()),
            "--requested-by" => out.requested_by = Some(value.clone()),
            "--request" => out.request = Some(value.clone()),
            "--executor" => out.executor = Some(value.clone()),
            "--theo-said" => out.theo_said = Some(value.clone()),
            "--question" => out.question = Some(value.clone()),
            "--option" => out.option_label = Some(value.clone()),
            "--evaluated-by" => out.evaluated_by = Some(value.clone()),
            "--when" => out.when = Some(value.clone()),
            _ => {}
        }
        i += 2;
    }
    out
}

/// Le parole di Theo, trascritte alla lettera con `--theo-said`, oppure
/// composte da `--question`+`--option` quando ha scelto da un modulo — mai
/// un misto dei due, e mai nessuno dei due: il comando non deve lasciar
/// passare un'autorizzazione senza le sue parole in una forma o nell'altra.
fn theo_said_from_args(args: &WriteArgs) -> Result<String, String> {
    match (&args.theo_said, &args.question, &args.option_label) {
        (Some(t), None, None) => Ok(t.clone()),
        (None, Some(q), Some(o)) => Ok(module_choice_to_theo_said(q, o)),
        (None, None, None) => Err(
            "servono le parole di Theo: --theo-said (alla lettera) oppure --question e --option insieme (scelta da un modulo)".to_string(),
        ),
        _ => Err(
            "--theo-said non si combina con --question/--option: sono due modi di trascrivere la stessa decisione, non un misto".to_string(),
        ),
    }
}

fn print_write_usage() {
    eprintln!("uso: claude-hooks captain-authorize --key <chiave> --requested-by <chi ha chiesto> \\");
    eprintln!("       --request <cosa esattamente> --executor <chi esegue> \\");
    eprintln!("       (--theo-said <le sue parole esatte, senza parafrasare> | --question <domanda> --option <etichetta scelta>) \\");
    eprintln!("       [--evaluated-by <chi ha valutato, default: capitano>] [--when <quando, default: adesso>] [--revoke]");
    eprintln!();
    eprintln!("QUESTO COMANDO SCRIVE NEL REGISTRO: LO USA SOLO IL CAPITANO.");
    eprintln!("Chi esegue non scrive mai qui: legge con 'claude-hooks authorization-check <chiave>'.");
}

/// Sottocomando `captain-authorize`, la penna del registro.
///
/// SOLO IL CAPITANO SCRIVE QUI — il nome del comando, questo aiuto e ogni
/// riga stampata lo ripetono apposta, perché un macchinista che se lo trova
/// sotto le dita deve riconoscere in un secondo che non è il suo comando.
///
/// Dopo aver scritto, rilegge il registro con `check` — lo stesso lettore di
/// `authorization-check` — e verifica che la chiave risulti nello stato
/// appena scritto. È la difesa contro la forma di guasto per cui questo
/// modulo esiste anche dalla parte della scrittura: un programma che esce 0
/// senza che la riga sia davvero finita nel registro.
pub fn run_write() -> i32 {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    let args = parse_write_args(&argv);

    let mut missing = Vec::new();
    if args.key.as_deref().unwrap_or("").is_empty() {
        missing.push("--key");
    }
    if args.requested_by.as_deref().unwrap_or("").is_empty() {
        missing.push("--requested-by");
    }
    if args.request.as_deref().unwrap_or("").is_empty() {
        missing.push("--request");
    }
    if args.executor.as_deref().unwrap_or("").is_empty() {
        missing.push("--executor");
    }
    if !missing.is_empty() {
        print_write_usage();
        eprintln!("captain-authorize: mancano {}", missing.join(", "));
        return 64;
    }

    let theo_said = match theo_said_from_args(&args) {
        Ok(t) => t,
        Err(message) => {
            print_write_usage();
            eprintln!("captain-authorize: {message}");
            return 64;
        }
    };

    let when = args.when.clone().unwrap_or_else(now_iso8601);
    let record = build_record(
        args.key.as_deref().unwrap_or_default(),
        &when,
        args.requested_by.as_deref().unwrap_or_default(),
        args.request.as_deref().unwrap_or_default(),
        &theo_said,
        args.evaluated_by.as_deref().unwrap_or("capitano"),
        args.executor.as_deref().unwrap_or_default(),
        args.revoke,
    );

    let path = registry_path();
    if let Err(e) = append_record(&path, &record) {
        eprintln!(
            "captain-authorize: NON SCRITTA — {e} ({}). Il registro resta come prima di questo tentativo: riprova.",
            path.display()
        );
        return 1;
    }

    // La prova che la riga è davvero finita nel registro, non solo che il
    // comando è arrivato in fondo: si rilegge da disco e si guarda cosa
    // risponderebbe ora `authorization-check` sulla stessa chiave.
    let reread = read_registry(&path);
    let (verdict, _) = check(reread.as_deref(), &record.key);
    let landed = match (&verdict, record.revoked) {
        (Verdict::Authorized(r), false) => *r == record,
        (Verdict::NotAuthorized, true) => true,
        _ => false,
    };
    if !landed {
        eprintln!(
            "captain-authorize: NON SCRITTA — rileggendo il registro la riga non torna come atteso. Non fidarti, controlla a mano {}.",
            path.display()
        );
        return 1;
    }

    if record.revoked {
        println!("REVOCATA (dal capitano): {}", record.key);
    } else {
        println!("AUTORIZZATA (dal capitano): {}", record.key);
    }
    println!("  quando:         {}", record.when);
    println!("  chi ha chiesto: {}", record.requested_by);
    println!("  cosa:           {}", record.request);
    println!("  Theo ha detto:  «{}»", record.theo_said);
    println!("  valutata da:    {}", record.evaluated_by);
    println!("  esegue:         {}", record.executor);
    println!("  registro:       {}", path.display());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(key: &str) -> String {
        format!(
            r#"{{"key":"{key}","when":"2026-08-21T12:00:00+02:00","requested_by":"prova","request":"cosa richiesta","theo_said":"parole di Theo","evaluated_by":"capitano","executor":"macchinista"}}"#
        )
    }

    // La riga che c'è: il caso base.
    // Mutazione che lo fa rosso: invertire il ramo `Some(record) if record.key
    // == key` in `check`, così una riga che combacia esce come `NotAuthorized`.
    #[test]
    fn authorized_when_the_key_is_present() {
        let registry = line("gancio-nuovo:foo");
        let (verdict, warnings) = check(Some(&registry), "gancio-nuovo:foo");
        assert!(warnings.is_empty());
        match verdict {
            Verdict::Authorized(r) => {
                assert_eq!(r.theo_said, "parole di Theo");
                assert_eq!(r.executor, "macchinista");
            }
            other => panic!("atteso Authorized, trovato {other:?}"),
        }
    }

    // La riga che non c'è: una chiave diversa nel registro.
    // Mutazione che lo fa rosso: far restituire `Verdict::Authorized` per la
    // prima riga incontrata, indipendentemente dalla chiave cercata.
    #[test]
    fn not_authorized_when_the_key_is_absent() {
        let registry = line("gancio-nuovo:foo");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:bar");
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // Il registro illeggibile: `None`, cioè il file non si apre o non esiste.
    // Mutazione che lo fa rosso: sostituire il primo `let Some(content) =
    // registry else { return Unreadable }` con `registry.unwrap_or_default()`,
    // che tratta l'assenza come un registro vuoto — È IL MUTANTE DEL COMPITO:
    // un registro assente passerebbe per «nessuna autorizzazione» invece di
    // fermare tutto. Provato per davvero, esito sotto nel rapporto.
    #[test]
    fn unreadable_when_the_registry_could_not_be_read() {
        let (verdict, warnings) = check(None, "qualunque-chiave");
        assert_eq!(verdict, Verdict::Unreadable);
        assert!(warnings.is_empty());
    }

    // Il registro vuoto: il file esiste, zero righe. Deve rispondere
    // `NotAuthorized`, non `Unreadable` — è il gemello della prova sopra, e
    // insieme sono la distinzione che questo modulo esiste per tenere.
    // Mutazione che lo fa rosso: far rispondere `Unreadable` anche quando
    // `content.is_empty()`.
    #[test]
    fn empty_registry_is_not_authorized_not_unreadable() {
        let (verdict, warnings) = check(Some(""), "qualunque-chiave");
        assert_eq!(verdict, Verdict::NotAuthorized);
        assert!(warnings.is_empty());
    }

    // Una riga malformata insieme a una buona: non deve far crollare la
    // ricerca né nascondere la riga buona che segue.
    // Mutazione che lo fa rosso: interrompere il ciclo (`break`/`return
    // NotAuthorized`) alla prima riga che `parse_line` non riconosce, invece
    // di segnalarla e proseguire.
    #[test]
    fn malformed_line_is_reported_but_does_not_hide_a_good_line_after_it() {
        let registry = format!("{{questo non è JSON valido\n{}", line("gancio-nuovo:foo"));
        let (verdict, warnings) = check(Some(&registry), "gancio-nuovo:foo");
        assert_eq!(warnings.len(), 1, "la riga rotta deve produrre un avviso, uno solo");
        match verdict {
            Verdict::Authorized(r) => assert_eq!(r.key, "gancio-nuovo:foo"),
            other => panic!("la riga buona dopo quella rotta deve comunque farsi trovare, trovato {other:?}"),
        }
    }

    // Nessun combaciamento parziale: una chiave che contiene l'altra non deve
    // corrispondere, altrimenti un'autorizzazione larga ne coprirebbe una
    // stretta (o viceversa).
    // Mutazione che lo fa rosso: cambiare `record.key == key` in
    // `record.key.starts_with(key)` o `.contains(key)`.
    #[test]
    fn a_similar_key_is_not_a_match() {
        let registry = line("gancio-nuovo:foo-esteso");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:foo");
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // La lettura pura di una riga: campi mancanti restano vuoti, non fanno
    // cadere l'intera riga — solo la chiave è obbligatoria. `revoked`
    // assente si legge `false`, così le righe vecchie restano leggibili.
    #[test]
    fn parse_line_defaults_missing_optional_fields_to_empty() {
        let r = parse_line(r#"{"key":"solo-la-chiave"}"#).expect("la chiave basta a fare un record");
        assert_eq!(r.key, "solo-la-chiave");
        assert_eq!(r.theo_said, "");
        assert!(!r.revoked);
    }

    #[test]
    fn parse_line_rejects_a_record_without_a_key() {
        assert!(parse_line(r#"{"when":"oggi"}"#).is_none());
    }

    fn revoke_line(key: &str) -> String {
        format!(
            r#"{{"key":"{key}","when":"2026-08-21T13:00:00+02:00","requested_by":"prova","request":"revoca","theo_said":"revocata","evaluated_by":"capitano","executor":"macchinista","revoked":true}}"#
        )
    }

    // La proprietà che vale più di tutte: una revoca successiva spegne
    // un'autorizzazione precedente sulla stessa chiave.
    // Mutazione che lo fa rosso: in `check`, restituire `Verdict::Authorized`
    // ogni volta che `latest` è `Some(_)`, ignorando `record.revoked`.
    #[test]
    fn revoke_makes_a_previously_authorized_key_not_authorized() {
        let registry = format!("{}\n{}", line("gancio-nuovo:foo"), revoke_line("gancio-nuovo:foo"));
        let (verdict, warnings) = check(Some(&registry), "gancio-nuovo:foo");
        assert!(warnings.is_empty());
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // Una revoca su una chiave mai autorizzata: resta NotAuthorized, non
    // diventa un errore né un terzo stato — non c'era niente da revocare, e
    // il risultato per chi esegue è lo stesso di sempre.
    #[test]
    fn revoke_of_a_never_authorized_key_is_still_not_authorized() {
        let registry = revoke_line("gancio-nuovo:mai-concessa");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:mai-concessa");
        assert_eq!(verdict, Verdict::NotAuthorized);
    }

    // Due autorizzazioni sulla stessa chiave: vince la più recente, cioè
    // l'ultima nel file — il registro è append-only e cronologico.
    // Mutazione che lo fa rosso: tornare al vecchio `check` che si fermava
    // al primo match (`return` dentro il ciclo) invece di tenere l'ultimo.
    #[test]
    fn latest_authorization_for_the_same_key_wins() {
        let old = line("gancio-nuovo:foo");
        let newer = old.replace("parole di Theo", "decisione aggiornata");
        let registry = format!("{old}\n{newer}");
        let (verdict, _) = check(Some(&registry), "gancio-nuovo:foo");
        match verdict {
            Verdict::Authorized(r) => assert_eq!(r.theo_said, "decisione aggiornata"),
            other => panic!("atteso Authorized con la decisione più recente, trovato {other:?}"),
        }
    }

    // La composizione del caso «modulo»: la domanda più l'etichetta scelta,
    // non un riassunto.
    #[test]
    fn module_choice_composes_question_and_label() {
        let said = module_choice_to_theo_said(
            "aggiungere il permesso X?",
            "sì, per questa sessione soltanto",
        );
        assert!(said.contains("aggiungere il permesso X?"));
        assert!(said.contains("sì, per questa sessione soltanto"));
    }

    fn sample_record(key: &str, revoked: bool) -> AuthorizationRecord {
        build_record(
            key,
            "2026-08-21T15:00:00+02:00",
            "prova",
            "cosa richiesta",
            "parole di Theo",
            "capitano",
            "macchinista",
            revoked,
        )
    }

    #[test]
    fn build_record_carries_the_revoked_flag() {
        assert!(sample_record("k", true).revoked);
        assert!(!sample_record("k", false).revoked);
    }

    #[test]
    fn parse_write_args_reads_named_flags_and_the_bare_revoke_switch() {
        let argv: Vec<String> = [
            "--key", "k", "--requested-by", "chi", "--request", "cosa", "--executor", "chi-esegue",
            "--theo-said", "detto", "--revoke",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let args = parse_write_args(&argv);
        assert_eq!(args.key.as_deref(), Some("k"));
        assert_eq!(args.executor.as_deref(), Some("chi-esegue"));
        assert_eq!(args.theo_said.as_deref(), Some("detto"));
        assert!(args.revoke);
    }

    #[test]
    fn theo_said_from_args_prefers_the_literal_quote() {
        let mut args = WriteArgs::default();
        args.theo_said = Some("detto alla lettera".to_string());
        assert_eq!(theo_said_from_args(&args).unwrap(), "detto alla lettera");
    }

    #[test]
    fn theo_said_from_args_composes_from_the_module_choice() {
        let mut args = WriteArgs::default();
        args.question = Some("domanda?".to_string());
        args.option_label = Some("opzione A".to_string());
        let said = theo_said_from_args(&args).unwrap();
        assert!(said.contains("domanda?") && said.contains("opzione A"));
    }

    #[test]
    fn theo_said_from_args_rejects_neither_form() {
        assert!(theo_said_from_args(&WriteArgs::default()).is_err());
    }

    #[test]
    fn theo_said_from_args_rejects_a_mix_of_both_forms() {
        let mut args = WriteArgs::default();
        args.theo_said = Some("detto".to_string());
        args.question = Some("domanda?".to_string());
        assert!(theo_said_from_args(&args).is_err());
    }

    fn temp_registry_path(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "claude-hooks-authz-test-{label}-{}-{n}.jsonl",
            std::process::id()
        ))
    }

    // La prova che conta di più: una riga scritta con `append_record` e
    // riletta con lo stesso lettore che usa `authorization-check`
    // (`read_registry` + `check`) deve tornare identica a quella scritta.
    // Mutazione che lo fa rosso: in `record_to_line`, dimenticare un campo
    // (es. non scrivere `theo_said`) — il record riletto non torna uguale.
    #[test]
    fn record_round_trips_through_write_then_read() {
        let path = temp_registry_path("round-trip");
        let record = sample_record("prova:andata-e-ritorno", false);
        append_record(&path, &record).expect("la scrittura deve riuscire su una cartella pulita");
        let content = read_registry(&path).expect("il registro appena scritto deve rileggersi");
        let (verdict, warnings) = check(Some(&content), &record.key);
        assert!(warnings.is_empty());
        assert_eq!(verdict, Verdict::Authorized(record));
        let _ = std::fs::remove_file(&path);
    }

    // Due scritture in parallelo non devono contendersi lo stesso file
    // temporaneo. Il nome lo teneva distinto solo l'istante: due thread che
    // ci arrivano nello stesso nanosecondo ne ottengono uno solo, e chi
    // rinomina per primo porta via il file dell'altro — o il registro finisce
    // con la riga sbagliata, o la `rename` non trova più niente.
    // Mutazione che lo fa rosso: togliere il contatore dal nome del
    // temporaneo in `append_record`.
    #[test]
    fn two_parallel_appends_do_not_share_the_temporary_file() {
        // La barriera a ogni giro è ciò che rende la prova capace di
        // fallire: senza, i due thread scivolano e la collisione si vede
        // tre volte su cinque, che è il modo peggiore di provare qualcosa.
        let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|slot| {
                let gate = std::sync::Arc::clone(&gate);
                std::thread::spawn(move || {
                    for round in 0..200 {
                        gate.wait();
                        let path = temp_registry_path(&format!("parallel-{slot}-{round}"));
                        let record =
                            sample_record(&format!("prova:parallela-{slot}-{round}"), false);
                        append_record(&path, &record).expect("la scrittura deve riuscire");
                        let content = read_registry(&path).expect("il registro deve rileggersi");
                        let (verdict, _) = check(Some(&content), &record.key);
                        assert_eq!(verdict, Verdict::Authorized(record));
                        let _ = std::fs::remove_file(&path);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("nessuno dei due thread deve cadere");
        }
    }

    // Chi lascia il lucchetto toglie IL PROPRIO, non il file che sta lì.
    //
    // Il caso, riprodotto da un verdetto su uno scrittore lento: A prende il
    // lucchetto e ci mette più dell'attesa massima; B aspetta, alla scadenza lo
    // forza e ne crea uno suo; poi A finisce e il suo `Drop` cancella **il
    // lucchetto vivo di B**. Da lì i due scrivono senza esclusione e una riga
    // sparisce. Il nome dentro il file c'era già e non lo rileggeva nessuno.
    // Mutazione che lo fa rosso: in `Drop`, rimuovere senza confrontare il nome.
    #[test]
    fn dropping_a_lock_that_is_no_longer_ours_leaves_it_alone() {
        let registry = temp_registry_path("lucchetto-altrui");
        let mine = RegistryLock::acquire(&registry).expect("il primo lo prende");
        // Qualcun altro lo forza e ci mette il proprio nome, come farebbe un
        // secondo scrittore alla scadenza.
        std::fs::write(&mine.path, "un-altro-processo").expect("la forzatura scrive");
        let path = mine.path.clone();
        drop(mine);
        assert_eq!(
            std::fs::read_to_string(&path).ok().as_deref(),
            Some("un-altro-processo"),
            "il lucchetto di un altro non si tocca uscendo"
        );
        let _ = std::fs::remove_file(&path);
    }

    // DUE CHE FORZANO INSIEME NON DEVONO ENTRARE INSIEME.
    //
    // Lo scenario che un verdetto ha riprodotto su processi veri, due volte su
    // dodici: un lucchetto abbandonato, due scrittori che lo aspettano e
    // arrivano a scadenza a pochi millisecondi di distanza. Il primo forza e ne
    // crea uno suo; il secondo, scaduto anche lui, forzava **quello vivo** — e
    // da lì i due scrivevano senza esclusione, con due `AUTORIZZATA` stampati e
    // una riga sola su disco.
    //
    // La cura è guardare QUALE lucchetto si sta forzando: se il contenuto è
    // cambiato mentre si aspettava, l'attesa riparte.
    // Mutazione che lo fa rosso: in `acquire_waiting`, togliere il ramo
    // `if now != seen` che azzera la scadenza.
    #[test]
    fn two_forcers_do_not_both_get_in() {
        let registry = temp_registry_path("due-forzatori");
        let lock_path = registry
            .parent()
            .unwrap()
            .join(format!(".{}.lock", registry.file_name().unwrap().to_string_lossy()));
        let wait = std::time::Duration::from_millis(80);
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worst = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for round in 0..15 {
            // Un lucchetto abbandonato da un processo che non c'è più.
            std::fs::write(&lock_path, format!("orfano-{round}")).expect("il finto orfano");
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let (gate, inside, worst) = (
                        std::sync::Arc::clone(&gate),
                        std::sync::Arc::clone(&inside),
                        std::sync::Arc::clone(&worst),
                    );
                    let registry = registry.clone();
                    std::thread::spawn(move || {
                        gate.wait();
                        let lock = RegistryLock::acquire_waiting(&registry, wait)
                            .expect("il lucchetto si prende");
                        let now = inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        worst.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        drop(lock);
                    })
                })
                .collect();
            for handle in handles {
                handle.join().expect("nessuno dei due deve cadere");
            }
        }
        let _ = std::fs::remove_file(&lock_path);
        assert_eq!(
            worst.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "due scrittori sono stati dentro il lucchetto nello stesso momento"
        );
    }

    // E il caso normale resta: il proprio lucchetto si toglie, o il prossimo
    // aspetta dieci secondi per niente.
    #[test]
    fn dropping_our_own_lock_removes_it() {
        let registry = temp_registry_path("lucchetto-proprio");
        let mine = RegistryLock::acquire(&registry).expect("lo prende");
        let path = mine.path.clone();
        drop(mine);
        assert!(!path.exists(), "il proprio lucchetto si toglie uscendo");
    }

    // Due scritture sullo STESSO registro devono esserci tutte e due. È il
    // caso reale, non uno di scuola: `registry_path()` restituisce sempre lo
    // stesso file, quindi due `captain-authorize` in parallelo — o due thread —
    // ci arrivano insieme. Senza serializzazione questa funzione legge tutto,
    // ricompone e rinomina sopra: chi rinomina per ultimo cancella la riga
    // dell'altro **senza errore e con uscita zero**, che è la forma peggiore.
    // Mutazione che lo fa rosso: togliere il lock da `append_record`.
    #[test]
    fn two_writers_on_the_same_registry_both_survive() {
        let path = temp_registry_path("stesso-registro");
        let _ = std::fs::remove_file(&path);
        let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|slot| {
                let gate = std::sync::Arc::clone(&gate);
                let path = path.clone();
                std::thread::spawn(move || {
                    for round in 0..50 {
                        let record =
                            sample_record(&format!("prova:condivisa-{slot}-{round}"), false);
                        gate.wait();
                        append_record(&path, &record).expect("la scrittura deve riuscire");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("nessuno dei due thread deve cadere");
        }
        let content = read_registry(&path).expect("il registro deve rileggersi");
        let mut mancanti = Vec::new();
        for slot in 0..2 {
            for round in 0..50 {
                let key = format!("prova:condivisa-{slot}-{round}");
                if !matches!(check(Some(&content), &key).0, Verdict::Authorized(_)) {
                    mancanti.push(key);
                }
            }
        }
        let _ = std::fs::remove_file(&path);
        assert!(
            mancanti.is_empty(),
            "{} righe su 100 sono sparite senza errore, la prima è {}",
            mancanti.len(),
            mancanti[0]
        );
    }

    // Una seconda riga che revoca la prima, scritta e riletta da disco: la
    // stessa proprietà del test sopra, con una vera revoca in mezzo.
    #[test]
    fn a_second_append_that_revokes_the_first_wins_on_disk() {
        let path = temp_registry_path("revoke-on-disk");
        let grant = sample_record("prova:revoca-su-disco", false);
        append_record(&path, &grant).expect("la prima scrittura deve riuscire");
        let mut revoke = grant.clone();
        revoke.revoked = true;
        revoke.theo_said = "revocata".to_string();
        append_record(&path, &revoke).expect("la seconda scrittura deve riuscire");
        let content = read_registry(&path).expect("il registro deve rileggersi");
        let (verdict, _) = check(Some(&content), &grant.key);
        assert_eq!(verdict, Verdict::NotAuthorized);
        let _ = std::fs::remove_file(&path);
    }

    // La scrittura che fallisce: il genitore del percorso è un file vero,
    // non una cartella, quindi `create_dir_all` non può funzionare. Il
    // codice d'uscita di `run_write` non deve mai essere 0 in questo caso —
    // qui si prova solo che la funzione di basso livello riporta l'errore.
    // Mutazione che lo fa rosso: far inghiottire l'errore con `.ok()` invece
    // di propagarlo con `?`.
    #[test]
    fn append_fails_loudly_when_the_directory_cannot_be_created() {
        let blocker = std::env::temp_dir().join(format!(
            "claude-hooks-authz-test-blocker-{}",
            std::process::id()
        ));
        std::fs::write(&blocker, b"non e' una cartella").expect("serve un file vero da bloccare");
        let target = blocker.join("autorizzazioni.jsonl");
        let record = sample_record("prova:scrittura-fallita", false);
        let result = append_record(&target, &record);
        assert!(
            result.is_err(),
            "la scrittura doveva fallire: il genitore del percorso non è una cartella"
        );
        let _ = std::fs::remove_file(&blocker);
    }
}
