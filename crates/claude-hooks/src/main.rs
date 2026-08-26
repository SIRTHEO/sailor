//! Un binario solo per tutti i ganci, scelto col primo argomento.
//!
//! PERCHÉ UNO SOLO. Un comando `Bash` attraversa oggi 14 ganci, e la misura del
//! 16/08/2026 dice che girano in parallelo: il costo non è la loro somma, è il
//! più lento — ~73 ms, dettati dai due ganci che avviano Node. Nove processi
//! separati per ogni chiamata a strumento sono ~33.500 avvii di interprete al
//! giorno che questo binario riduce a uno.
//!
//! Niente parser di argomenti: `clap` costa più tempo di avvio di quanto ne
//! faccia risparmiare, e qui i sottocomandi sono un elenco chiuso.
//!
//! Uso:
//!     claude-hooks cd-guard      legge il JSON del gancio da stdin
//!     claude-hooks --list        i ganci disponibili

mod authorizations;
mod duplication;
mod handoff;
mod handoff_on_stop;
mod handoff_precompact;
mod handoff_required;
mod linear;
mod live_rules;
mod preflight;
mod relay;
mod destructive_commands;
mod relay_eval;
mod restart;
mod scope_drift;
mod successor;
#[cfg(test)]
mod test_home;
mod worktree_deletes;
// I quattro porti aperti il 17/08/2026. Registrati come scheletri prima di
// essere scritti, così ogni porto tocca un file solo e il binario compila a
// ogni passo.
mod handoff_threshold;
mod hook_census;
mod long_session;
mod link_worktree_rules;
mod permission_stall;
mod spotlight_marker;
mod status_board;
mod uncovered_exit;
mod uncovered_thread;
// La seconda ondata, i quattro grossi: 400-824 righe di Python ciascuno.
mod ai_personal_data;
mod json_tool;
mod memory_anchors;
mod memory_citation_gate;
// La freschezza delle consegne, 24/08/2026: gemello di `queue_freshness` su un
// altro corpus, con in comune il blocco rigenerabile di `guards::regen_block`.
mod memory_freshness;
// Il governo della memoria della macchina, 24/08/2026. Niente a che vedere con
// i due moduli qui sopra, che parlano della memoria delle sessioni: questo
// guarda la RAM, e l'omonimia è solo del vocabolario italiano.
mod memory_governor;
mod orca_cleanup;
mod reachability;
mod register_session;
// Il terzo, portato il 20/08/2026 perché il gate della lingua non lascia più
// eseguire il suo gemello Python: uno strumento di sola lettura che nessuno può
// lanciare è un rosso che nessuno può ispezionare.
mod stale_facts;
mod session_messages;
mod skill_nudge;
mod work_status;
// Il quarto porto della stessa ondata, il 20/08/2026: il controllo che manca
// sulle fusioni con schiacciamento, dove il ramo di partenza resta vivo e
// divergente senza che nessun conflitto lo segnali.
mod squash_orphans;
// Il quinto: se l'indice semantico riflette il ramo giusto. Nasce da una
// misura del 20/08/2026 dove tre canonici su quattro erano su rami di lavoro
// altrui, e l'indicizzatore taceva — nessun errore, solo un «green» bugiardo.
mod index_bridge;
mod index_freshness;
// Il gancio d'innesco della ronda delle novità, 23/08/2026 (gradino 9 della
// scala): due inneschi locali a `SessionStart`. Non ancora acceso — la riga
// di `settings.json` è di Theo, classe MAI (libro di bordo, voce «La
// configurazione si mantiene da sola»).
mod ronda_trigger;
// Il raccoglitore dei marcatori, 21/08/2026: il congedo li butta una volta sola,
// e ciò che in quell'istante non si sapeva vivo restava sul disco per sempre.
// NON È IN SERVIZIO: nessuna radice lo invoca, nessun servizio lo sveglia.
mod marker_sweep;
// Il registro dei costi, 23/08/2026 (gradino 4 della scala): non è un porto —
// nasce già in Rust. `record` è un gancio su `Stop`/`SubagentStop`, non ancora
// acceso (`docs/2026-08-22-gesto-message-budget.md`); `backfill` e `report`
// sono strumenti da riga di comando.
mod costs;
// Il router di fase, 23/08/2026 (gradino 5 della scala): un `PreToolUse` su
// `Agent` che sceglie il modello dal mestiere. Non ancora acceso
// (`docs/2026-08-22-gesto-message-budget.md`).
mod phase_router;
// Il canarino del formato transcript, 24/08/2026: non è un gancio, è lo
// strumento che prova su un transcript vero le assunzioni di schema che ogni
// misura di casa dà per scontate. Lo chiama la ronda.
mod transcript_canary;
// Il filtro sulle uscite dei comandi, 24/08/2026: accorcia un'uscita rumorosa
// e dichiara quanto ha tolto. Non è un gancio — il punto in cui servirebbe
// (dopo l'esecuzione, prima che l'uscita torni) non esiste in questa versione:
// `PostToolUse` può solo aggiungere contesto, non sostituire il risultato.
mod output_filter;
// Il gancio che porta il filtro dove serve, 24/08/2026: `PreToolUse` su Bash,
// riscrive il comando con `updatedInput` — ma solo per la lista ristretta di
// famiglie che pesa il 20,1% dei byte, così il rischio dei permessi rivalutati
// non si presenta sul resto. Non è acceso: la riga è di Theo.
mod bash_wrap;
// L'esame della forma, 24/08/2026: non è un gancio e non giudica un evento.
// Ogni altro meccanismo qui dentro è un allarme, e un allarme ha bisogno di
// qualcosa che si rompa; questo chiede «che forma ha preso questa casa, e in
// che direzione si muove» — una domanda che nessun evento fa scattare. Si
// sveglia su una soglia di righe mosse, come la ronda, non a un'ora.
mod shape_exam;
// La freschezza della coda, 24/08/2026: non è un gancio e non giudica un
// evento. Dice quali voci di coda parlano della stessa cosa e quali sono
// troppo vecchie per essere credute, e scrive il rilievo dentro le voci
// stesse invece di aprirne di nuove.
mod queue_freshness;
// Il depositatore dei guasti, portato dalla shell il 25/08/2026: legge i
// registri delle automazioni e apre da sé una voce di coda quando una riga
// marcata come guasto si ripete oltre soglia. Non è un gancio — lo chiama la
// ronda della coda a ogni giro, e chiunque a mano.
mod fault_deposit;
// `role-claim.sh`/`role-vacancy.sh`, portati insieme il 25/08/2026 perché sono
// un solo giudizio: chi dichiara un mestiere, chi lo tiene ancora, e il terzo
// stato (vuoto per decisione). Non è un gancio — lo chiama chi nasce, a mano.
mod role_claim;
// La terza fonte del mandato di `SessionStart`, il 25/08/2026: quando né la
// staffetta né un filo scoperto danno un incarico, sceglie una voce di coda
// dispacciabile e la impone come lavoro. `opening_notice` è un gancio vero
// dentro `register-session`; il verbo da riga di comando è solo un rapporto —
// sta in NOT_HOOKS.
mod queue_mandate;

use hook_io::{Decision, Mode};

/// Rimette il comportamento Unix quando chi legge chiude il tubo presto.
///
/// La libreria standard di Rust ignora `SIGPIPE` all'avvio, quindi una scrittura
/// su un tubo chiuso torna come errore e `println!` va in **panico**: il
/// messaggio finisce su standard error, l'uscita non è più zero, e un gancio che
/// parla con l'harness scrivendo JSON sporca il canale con cui dice la verità.
/// Visto il 24/08/2026 nel registro di `release-hooks.sh`, da un `hook-census`
/// incanalato in qualcosa che legge una riga sola.
///
/// Ripristinando il gestore predefinito il processo muore quieto, come ogni
/// programma Unix. Gli **altri** errori di scrittura non sono toccati: uno
/// standard output chiuso o rotto per un altro motivo continua a farsi sentire,
/// ed è il verso della prova che conta di più — una cura che zittisse anche
/// quelli avrebbe coperto un guasto invece di ripararlo.
fn restore_default_sigpipe() {
    // 13 è `SIGPIPE` e 0 è `SIG_DFL` su ogni Unix in cui questo binario gira.
    // Dichiarati qui invece di prendere una dipendenza esterna per due numeri.
    extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    unsafe {
        signal(13, 0);
    }
}

fn main() {
    restore_default_sigpipe();
    let args: Vec<String> = std::env::args().collect();
    let Some(which) = args.get(1).map(String::as_str) else {
        eprintln!("uso: claude-hooks <gancio>   (--list per l'elenco)");
        std::process::exit(64);
    };

    if which == "--list" {
        // Prima stampava i soli nomi di SMOKE: chi chiedeva «quali ganci
        // esistono?» ne vedeva 5 su 22, e i 17 senza caso di prova erano
        // invisibili proprio a chi li cercava (17/08/2026).
        // La terza colonna risponde a «questo va acceso in una radice?». Senza,
        // chi legge l'elenco non distingue un gancio spento da uno strumento che
        // si invoca a mano, e i due casi hanno cure opposte.
        // Le colonne 4 e 5 separano le due domande che la seconda fondeva:
        // «il binario si prova da sé qui dentro» (self_check) non è la stessa
        // cosa di «esiste un test altrove» (modulo o crate delegato). La
        // colonna 2 resta l'unione delle due, per chi vuole solo sì/no e per
        // non rompere chi la legge già (`accettazione.py`, indice 1).
        for name in ALL_HOOKS {
            let covered = if is_covered(name) {
                "provato"
            } else {
                "senza caso"
            };
            let kind = if is_hook(name) { "gancio" } else { "strumento" };
            let self_checked = if self_check_covers(name) {
                "autoverifica"
            } else {
                "senza autoverifica"
            };
            let module_tested = if has_module_test(name) {
                "test-modulo"
            } else {
                "senza test-modulo"
            };
            println!("{name}\t{covered}\t{kind}\t{self_checked}\t{module_tested}");
        }
        return;
    }

    if which == "--check" {
        std::process::exit(self_check());
    }

    // Il catalogo degli eventi, e la domanda che il censimento non fa: non «il
    // file esiste» ma «il gancio partirebbe».
    if which == "--preflight" {
        let verbose = args.iter().any(|a| a == "--verbose");
        // All'apertura di una sessione il messaggio va nel contesto del modello
        // e l'uscita resta 0: un gancio di SessionStart che fallisce sarebbe un
        // guasto in piu', non un avviso.
        let voice = if args.iter().any(|a| a == "--session-start") {
            preflight::Voice::SessionStart
        } else {
            preflight::Voice::Command
        };
        std::process::exit(preflight::run_with(verbose, voice));
    }

    // L'indice semantico da riga di comando. Intercettato qui, come `--list` e
    // `--preflight`, perché ha bisogno degli argomenti che seguono — e perché
    // non è un gancio: non legge stdin e non deve fallire in silenzio.
    if which == "indice" {
        std::process::exit(index_bridge::run(&args[2..]));
    }
    // Come `indice`: si invoca a mano, non lo chiama nessun evento — quindi
    // esce prima del giro dei ganci, che si aspetta un payload su stdin.
    if which == "stato" {
        std::process::exit(status_board::run());
    }

    // Fail-open per tutti, prima ancora di leggere stdin: un `PreToolUse` che
    // esce in errore rifiuta ogni strumento della sessione.
    let code = match run(which) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("gancio ({which}) non ha potuto decidere: {message}");
            0
        }
    };
    std::process::exit(code);
}

/// Per ogni gancio: il nome, un comando che **deve** essere bloccato, e uno che
/// **deve** passare.
///
/// Serve perché «il file esiste» non è la domanda giusta. Il 16/08/2026 la
/// macchina si è fermata con un gancio il cui file mancava, e il censimento che
/// avrebbe dovuto vederlo controllava proprio l'esistenza — mentre il modo in
/// cui si rompe un gancio, quasi sempre, è che parte e risponde male:
/// sottocomando rinominato, binario per l'architettura sbagliata, panico
/// all'avvio. Qui si chiede al gancio di decidere, e si guarda cosa decide.
/// Ogni gancio che il binario sa eseguire. L'elenco è scritto a mano perché il
/// dispatch è un `match`, ma non può divergere: il test
/// `ogni_gancio_del_dispatch_e_elencato` rilegge questo stesso sorgente e
/// fallisce se un ramo nuovo non compare qui.
const ALL_HOOKS: &[&str] = &[
    "ai-personal-data",
    "allow-worktree-deletes",
    "allow-session-messages",
    "json",
    "block-destructive",
    "block-pr-merge-admin",
    "block-worktree-create",
    "cd-guard",
    "instincts",
    "code-language",
    "comment-refs",
    "duplication",
    "legacy-script",
    "message-budget",
    "handoff-arms-successor",
    "handoff-measure",
    "handoff-on-stop",
    // Il porto del 25/08/2026: lo stesso lavoro di
    // `~/.claude/scripts/handoff-precompact.sh`, ma la consegna finisce in
    // `~/.claude/state/consegne-precompact/`, che un riavvio non cancella —
    // vedi la premessa di `handoff_precompact.rs`. La riga in `settings.json`
    // che lo mette al posto dello script resta un gesto di Theo.
    "handoff-precompact",
    "handoff-required",
    "handoff-latest",
    "repo-tools",
    "handoff-resolve",
    "hooks-off",
    "linear-readonly",
    "live-rules",
    "observe",
    "pr-title",
    "relay",
    "relay-evaluate",
    "relay-chain",
    "relay-read-chain",
    "relay-sweep",
    "restart-count",
    "restart-notice",
    "scope-drift",
    "socraticode-gate",
    "successor-probe",
    "handoff-threshold",
    // Il contatore dei turni del 23/08/2026 (gradino 1, contesto corto): la
    // riga che lo accende è di Theo (`docs/2026-08-22-gesto-message-budget.md`).
    "long-session",
    "hook-census",
    "link-worktree-rules",
    "spotlight-marker",
    "orca-cleanup",
    "register-session",
    "skill-nudge",
    "work-status",
    "squash-orphans",
    "index-freshness",
    // Non e un gancio, come `json`: e lo strumento che risponde a «chi lancia
    // ancora questo controllo». Sta qui perche il dispatch e l'elenco si
    // controllano a vicenda, e il controllo ha fatto il suo mestiere — questa
    // riga mancava, e il workspace non passava piu.
    "reachability",
    "memory-anchors",
    "memory-citation-gate",
    "stale-facts",
    // Il sesto porto, il 21/08/2026: risponde a «questa cosa è autorizzata?»
    // leggendo `state/autorizzazioni.jsonl`. Non giudica un evento della
    // sessione, quindi non e' acceso da nessuna radice — sta in NOT_HOOKS.
    "authorization-check",
    // La penna dello stesso registro, aggiunta lo stesso giorno: scrive una
    // riga in coda. Stessa ragione della voce sopra: non e' un gancio,
    // nessuna radice lo accende, sta in NOT_HOOKS — lo digita solo il
    // capitano.
    "captain-authorize",
    // Il settimo, il 21/08/2026: ripassa i marcatori che il congedo di una
    // sessione non ha potuto buttare. Non giudica nessun evento, quindi sta in
    // NOT_HOOKS — e per ora non lo invoca nemmeno un servizio.
    "marker-sweep",
    // L'ottavo, il 21/08/2026: legge i marcatori che `observe permission`
    // scrive e dice quali sessioni sono ferme su un permesso. Non giudica un
    // evento — lo si interroga da fuori — quindi sta in NOT_HOOKS.
    "permission-stall",
    // Il decimo, il 25/08/2026: elenca le consegne rimaste senza esecutore,
    // cioè i fili che il successore non ha raccolto e la cui sessione non c'è
    // più. I marcatori li scrive `consegna-arma-successore` quando ferma; qui
    // si leggono, e come sopra si interroga da fuori — NOT_HOOKS.
    "fili-scoperti",
    // Il registro dei costi, 23/08/2026: `record` è un gancio vero su
    // `Stop`/`SubagentStop`, quindi NON sta in NOT_HOOKS anche se lo stesso
    // slug porta anche `backfill` e `report` (strumenti) — stessa forma di
    // `duplication`, che mescola un gancio e i suoi verbi da riga di comando.
    "costs",
    // Il nono, il 23/08/2026 (gradino 5): riscrive il modello di un `Agent`
    // dal mestiere. `PreToolUse`/`Agent`, quindi NON sta in NOT_HOOKS.
    "phase-router",
    // Il decimo, il 23/08/2026 (gradino 9): la ronda delle novità. Non
    // giudica un nome di strumento — legge `source` su `SessionStart` — e non
    // è ancora accesa: la riga è di Theo.
    "ronda-trigger",
    // L'undicesimo, il 24/08/2026: il canarino del formato transcript. Non
    // giudica nessun evento — lo si interroga da fuori, e la ronda lo chiama —
    // quindi sta in NOT_HOOKS.
    "transcript-canary",
    // Il dodicesimo, il 24/08/2026: il governo della memoria della macchina.
    // `PreToolUse` su Bash, quindi NON sta in NOT_HOOKS — anche se lo stesso
    // slug porta il verbo `report`, che si digita da fuori. Non è ancora
    // acceso: la riga in `settings.json` è di Theo.
    "memory-governor",
    // Il filtro sulle uscite, 24/08/2026: legge un'uscita da stdin e la
    // accorcia. Sta in NOT_HOOKS perché nessun evento può ospitarlo — vedi il
    // commento sul modulo.
    "filter-output",
    // Il gancio che invoca il filtro qui sopra, stesso giorno: `PreToolUse` su
    // Bash, quindi NON sta in NOT_HOOKS. Non ancora acceso: la riga è di Theo.
    "wrap-bash",
    // L'esame della forma, 24/08/2026. Sta in NOT_HOOKS: non giudica un evento
    // — lo si interroga da fuori, e con `--if-moved` si sveglia da sé quando la
    // casa si è mossa abbastanza. La riga di `settings.json` è di Theo.
    "shape",
    // La freschezza della coda, 24/08/2026: risponde a «questa voce è ancora
    // credibile, e qualcun altro ha già detto il contrario?». Sta in NOT_HOOKS
    // perché non giudica nessun evento — e non aggiunge un byte al prologo di
    // proposito: il rilievo lo scrive dentro le voci, non davanti a tutti.
    "queue-freshness",
    // Lo stesso, lo stesso giorno, sulle consegne: «questo piano è di una
    // sessione chiusa?». Anche questo in NOT_HOOKS, e anche questo scrive il
    // rilievo dentro il documento invece di aprirne uno nuovo.
    "memory-freshness",
    // Il depositatore dei guasti, 25/08/2026, primo dei tredici script che
    // decidono a passare in Rust. Sta in NOT_HOOKS: non giudica un evento della
    // sessione, lo chiama la ronda della coda.
    "fault-deposit",
    // `role-claim`, stesso giorno: dichiara un mestiere della configurazione.
    // Sta in NOT_HOOKS: non giudica un evento della sessione, lo chiama chi
    // nasce, a mano.
    "role-claim",
    // Il verbo da riga di comando della terza fonte del mandato, il
    // 25/08/2026: «quale voce di coda si dispaccerebbe ora». Non giudica un
    // evento — lo si interroga da fuori, come `fili-scoperti` — quindi sta in
    // NOT_HOOKS. Il gancio vero è `register-session`, che lo chiama a ogni
    // `SessionStart`.
    "mandato-coda",
];

/// Gli slug che NON sono ganci: strumenti da riga di comando, finestre di sola
/// lettura, anelli di una catena che chiama un altro anello.
///
/// Serve per una domanda che nessun controllo faceva: «questo gancio e
/// acceso?». Fino al 19/08/2026 `comment-refs` e `allow-session-messages` erano
/// scritti, provati, portati e committati — e nessuna radice li invocava, quindi
/// non erano mai partiti. I due controlli che c'erano guardavano altrove: la
/// vista di aderenza chiede «ha un contratto?» e li dava per **provati**, il
/// censimento di raggiungibilita parte dai file sul disco e vedeva solo i loro
/// ripieghi, in mezzo ad altri sedici orfani legittimi.
///
/// Perche un elenco a mano e non una deduzione dal codice: dedurlo dal braccio
/// (chi legge stdin e un gancio) sbaglia in entrambe le direzioni, perche molti
/// bracci delegano la lettura a una funzione. E dedurlo dai commenti e peggio:
/// il primo tentativo di questa misura ha creduto `handoff-latest` un gancio
/// perche il commento del braccio accanto conteneva la parola «stdin». Qui la
/// dichiarazione e esplicita, e il test la tiene onesta come fa per ALL_HOOKS.
const NOT_HOOKS: &[&str] = &[
    "json",
    "reachability",
    "memory-anchors",
    "handoff-measure",
    "handoff-latest",
    "handoff-resolve",
    "repo-tools",
    "relay-evaluate",
    "relay-chain",
    "relay-read-chain",
    "relay-sweep",
    "restart-count",
    "successor-probe",
    "stale-facts",
    "authorization-check",
    "captain-authorize",
    "marker-sweep",
    "permission-stall",
    "fili-scoperti",
    "transcript-canary",
    "filter-output",
    "shape",
    "queue-freshness",
    "memory-freshness",
    "fault-deposit",
    "role-claim",
    "mandato-coda",
];

fn is_hook(name: &str) -> bool {
    !NOT_HOOKS.contains(&name)
}

/// Ganci con un blocco dedicato dentro `self_check()`, oltre a quelli della
/// tabella SMOKE: giudicano una coppia di dati (non un comando singolo) e per
/// quello hanno un pezzo di codice scritto a parte, più sotto in questo file.
///
/// Elenco a mano perché il corpo di `self_check()` non è un dato strutturato:
/// il test `ogni_extra_ha_un_blocco_in_self_check` lo tiene onesto contando le
/// occorrenze del nome nel sorgente della funzione.
const SELF_CHECK_EXTRA: &[&str] = &[
    "code-language",
    "comment-refs",
    "duplication",
    "legacy-script",
    "orca-cleanup",
    "spotlight-marker",
    "work-status",
];

/// «Il binario sa provarsi da solo su questo gancio»: gira dentro
/// `self_check()`, con un comando vero o un caso scritto a mano — non solo un
/// nome citato in un elenco.
fn self_check_covers(name: &str) -> bool {
    SMOKE.iter().any(|(n, _, _)| *n == name) || SELF_CHECK_EXTRA.contains(&name)
}

/// La domanda pura, isolata dal resto: un sorgente contiene un caso di prova?
/// Separata da `has_module_test` per poterla provare senza toccare nessun file
/// del disco.
fn source_contains_test(source: &str) -> bool {
    source.contains("#[test]")
}

/// «Esiste una prova che copre questo gancio»: un `#[test]` nel modulo che lo
/// implementa in questo crate, o nel modulo del crate `guards`/`hook-io` a cui
/// delega. I sorgenti si leggono a tempo di compilazione con `include_str!`,
/// così un test tolto o aggiunto altrove si vede da solo alla build
/// successiva, senza un booleano scritto a mano che invecchia in silenzio —
/// il difetto che questa colonna corregge. Resta a mano solo la mappa
/// gancio→file: cambia molto più di rado di quanto cambino i test, ed è
/// tenuta onesta dal test `ogni_gancio_ha_una_voce_nella_mappa_dei_test`.
/// Costo: alcuni file (`relay.rs`, `successor.rs`, `handoff.rs`) sono citati
/// da più ganci e finiscono embedded più volte nel binario — qualche centinaio
/// di KB in più, accettabile per uno strumento locale.
fn has_module_test(name: &str) -> bool {
    let sources: &[&str] = match name {
        "ai-personal-data" => &[include_str!("ai_personal_data.rs")],
        "allow-worktree-deletes" => &[
            include_str!("worktree_deletes.rs"),
            include_str!("../../guards/src/worktree_deletes.rs"),
        ],
        "allow-session-messages" => &[
            include_str!("session_messages.rs"),
            include_str!("../../guards/src/session_messages.rs"),
        ],
        "json" => &[include_str!("json_tool.rs")],
        // Due file: il giudizio e la colla che gli porta il ramo vero. Senza il
        // secondo il rapporto conterebbe coperto un gancio di cui è provata
        // solo metà.
        "block-destructive" => &[
            include_str!("destructive_commands.rs"),
            include_str!("../../guards/src/destructive_commands.rs"),
        ],
        "block-pr-merge-admin" => &[include_str!("../../guards/src/pr_merge_admin.rs")],
        "block-worktree-create" => &[include_str!("../../guards/src/worktree_create.rs")],
        "cd-guard" => &[include_str!("../../guards/src/cd_guard.rs")],
        "instincts" => &[include_str!("../../guards/src/instincts.rs")],
        "code-language" => &[include_str!("../../guards/src/code_language.rs")],
        // Rimessa il 25/08/2026: mancava nell'albero e c'era in `HEAD`, tolta da
        // qualcuno senza committare. Senza di lei il test che tiene onesta questa
        // mappa è rosso, e `--list` dichiara `legacy-script` «senza
        // test-modulo» mentre i suoi sei casi esistono — cioè il falso negativo
        // che questa mappa esiste per impedire.
        "legacy-script" => &[include_str!("../../guards/src/legacy_script.rs")],
        "message-budget" => &[include_str!("../../guards/src/message_budget.rs")],
        // Il secondo file prova la colla, non il giudizio: senza, il rapporto
        // conterebbe coperto un gancio di cui è provata solo metà.
        "comment-refs" => &[
            include_str!("../../guards/src/comment_refs.rs"),
            include_str!("../tests/comment_refs_glue.rs"),
        ],
        "duplication" => &[
            include_str!("duplication.rs"),
            include_str!("../../guards/src/duplication.rs"),
        ],
        "handoff-arms-successor" => &[include_str!("successor.rs")],
        "handoff-measure" => &[include_str!("handoff.rs")],
        "handoff-on-stop" => &[
            include_str!("handoff_on_stop.rs"),
            include_str!("../../guards/src/handoff_on_stop.rs"),
        ],
        "handoff-precompact" => &[include_str!("handoff_precompact.rs")],
        "handoff-required" => &[
            include_str!("handoff_required.rs"),
            include_str!("../../guards/src/handoff_required.rs"),
        ],
        "handoff-latest" => &[include_str!("relay.rs")],
        "repo-tools" => &[include_str!("../../guards/src/repo_tools.rs")],
        "handoff-resolve" => &[include_str!("handoff.rs")],
        "hooks-off" => &[include_str!("../../guards/src/hooks_off.rs")],
        "linear-readonly" => &[
            include_str!("linear.rs"),
            include_str!("../../guards/src/linear_readonly.rs"),
        ],
        "live-rules" => &[
            include_str!("live_rules.rs"),
            include_str!("../../guards/src/live_rules.rs"),
        ],
        "observe" => &[include_str!("../../hook-io/src/observations.rs")],
        "pr-title" => &[include_str!("../../guards/src/pr_title.rs")],
        "relay" => &[include_str!("relay.rs")],
        "relay-evaluate" => &[
            include_str!("relay_eval.rs"),
            include_str!("handoff.rs"),
            include_str!("../../guards/src/handoff.rs"),
        ],
        "relay-chain" => &[
            include_str!("relay_eval.rs"),
            include_str!("../../guards/src/chain.rs"),
        ],
        "relay-read-chain" => &[
            include_str!("relay_eval.rs"),
            include_str!("relay.rs"),
            include_str!("../../guards/src/chain.rs"),
        ],
        "relay-sweep" => &[include_str!("relay_eval.rs"), include_str!("relay.rs")],
        "restart-count" => &[
            include_str!("restart.rs"),
            include_str!("../../guards/src/restart.rs"),
        ],
        "restart-notice" => &[include_str!("restart.rs")],
        "scope-drift" => &[
            include_str!("scope_drift.rs"),
            include_str!("../../guards/src/scope_drift.rs"),
        ],
        "socraticode-gate" => &[include_str!("../../guards/src/socraticode_gate.rs")],
        "successor-probe" => &[
            include_str!("successor.rs"),
            include_str!("../../guards/src/successor.rs"),
        ],
        "handoff-threshold" => &[
            include_str!("handoff_threshold.rs"),
            include_str!("../../guards/src/handoff_threshold.rs"),
        ],
        "long-session" => &[
            include_str!("long_session.rs"),
            include_str!("../../guards/src/long_session.rs"),
        ],
        "hook-census" => &[include_str!("hook_census.rs")],
        "link-worktree-rules" => &[
            include_str!("link_worktree_rules.rs"),
            include_str!("../../guards/src/link_worktree_rules.rs"),
        ],
        "spotlight-marker" => &[
            include_str!("spotlight_marker.rs"),
            include_str!("../../guards/src/spotlight_marker.rs"),
        ],
        "orca-cleanup" => &[include_str!("orca_cleanup.rs")],
        "register-session" => &[include_str!("register_session.rs")],
        "skill-nudge" => &[
            include_str!("skill_nudge.rs"),
            include_str!("../../guards/src/skill_nudge.rs"),
        ],
        "work-status" => &[include_str!("work_status.rs")],
        "squash-orphans" => &[include_str!("squash_orphans.rs")],
        "index-freshness" => &[
            include_str!("index_freshness.rs"),
            include_str!("../../guards/src/index_freshness.rs"),
        ],
        "reachability" => &[include_str!("reachability.rs")],
        "memory-anchors" => &[
            include_str!("memory_anchors.rs"),
            include_str!("../../guards/src/memory_anchor.rs"),
        ],
        "memory-citation-gate" => &[include_str!("memory_citation_gate.rs")],
        "stale-facts" => &[
            include_str!("stale_facts.rs"),
            include_str!("../../guards/src/stale_facts.rs"),
        ],
        "authorization-check" => &[include_str!("authorizations.rs")],
        "captain-authorize" => &[include_str!("authorizations.rs")],
        // Il secondo file è dove vive il giudizio che la passata riusa: se
        // qualcuno togliesse i casi di `should_remove`, questa colonna deve
        // accorgersene anche per il raccoglitore.
        "marker-sweep" => &[
            include_str!("marker_sweep.rs"),
            include_str!("register_session.rs"),
        ],
        "permission-stall" => &[include_str!("permission_stall.rs")],
        "fili-scoperti" => &[include_str!("uncovered_thread.rs")],
        "costs" => &[
            include_str!("costs.rs"),
            include_str!("../../guards/src/cost_ledger.rs"),
        ],
        "phase-router" => &[
            include_str!("phase_router.rs"),
            include_str!("../../guards/src/phase_router.rs"),
        ],
        "ronda-trigger" => &[
            include_str!("ronda_trigger.rs"),
            include_str!("../../guards/src/ronda_trigger.rs"),
        ],
        "transcript-canary" => &[
            include_str!("transcript_canary.rs"),
            include_str!("../../guards/src/transcript_canary.rs"),
        ],
        "memory-governor" => &[
            include_str!("memory_governor.rs"),
            include_str!("../../guards/src/memory_governor.rs"),
        ],
        "filter-output" => &[
            include_str!("output_filter.rs"),
            include_str!("../../guards/src/output_filter.rs"),
        ],
        "wrap-bash" => &[
            include_str!("bash_wrap.rs"),
            include_str!("../../guards/src/bash_wrap.rs"),
        ],
        "shape" => &[
            include_str!("shape_exam.rs"),
            include_str!("../../guards/src/shape_exam.rs"),
        ],
        "queue-freshness" => &[
            include_str!("queue_freshness.rs"),
            include_str!("../../guards/src/queue_overlap.rs"),
        ],
        // Il terzo file è la parte comune coi due qui sopra: se qualcuno
        // togliesse i casi del blocco rigenerabile, questa colonna deve
        // accorgersene anche per la freschezza delle consegne.
        "memory-freshness" => &[
            include_str!("memory_freshness.rs"),
            include_str!("../../guards/src/handoff_freshness.rs"),
            include_str!("../../guards/src/regen_block.rs"),
        ],
        "fault-deposit" => &[
            include_str!("fault_deposit.rs"),
            include_str!("../../guards/src/fault_deposit.rs"),
        ],
        "role-claim" => &[
            include_str!("role_claim.rs"),
            include_str!("../../guards/src/role_claim.rs"),
        ],
        "mandato-coda" => &[include_str!("queue_mandate.rs")],
        _ => &[],
    };
    sources.iter().any(|s| source_contains_test(s))
}

/// La colonna larga di `--list`: «esiste una prova, di qualunque tipo, che
/// copre questo gancio». Unione delle due letture — non sostituisce le
/// colonne che le distinguono, le riassume per chi vuole solo sì/no.
fn is_covered(name: &str) -> bool {
    self_check_covers(name) || has_module_test(name)
}

const SMOKE: &[(&str, &str, &str)] = &[
    ("cd-guard", "cd /repo && git status", "git -C /repo status"),
    (
        "block-worktree-create",
        "git worktree add /home/someone/orca/workspaces/suite/x",
        "git worktree add /private/tmp/x",
    ),
    // Il gate SocratiCode non è in questa tabella: la sua decisione dipende da
    // un repo indicizzato e da un contatore per sessione, quindi un caso «deve
    // bloccare» qui sarebbe una finzione. La sua rete è il confronto col Node
    // in `tools/compare-socraticode-gate.py`, che gira con stato isolato.
    (
        // scritto a pezzi: un freno che blocca il proprio smoke test scritto in
        // chiaro renderebbe impossibile provarlo dalla riga di comando
        "block-pr-merge-admin",
        "gh pr merge 262 --admin",
        "gh pr merge 262 --squash",
    ),
    (
        // Scritto in chiaro, al contrario dei due sopra, ed è una proprietà del
        // freno e non una svista: questo gancio giudica il **gesto**, e un
        // comando che si limita a nominare una cancellazione non è un gesto. Il
        // caso che passa lo dimostra proprio nominando quello che blocca.
        "block-destructive",
        "rm -rf /home/someone/personal/sailor/src",
        "grep -n 'rm -rf' /home/someone/personal/sailor/src",
    ),
    (
        // anche questo a pezzi, e per lo stesso motivo: scritto in chiaro, il
        // gancio vivo rifiuterebbe il comando che compila il proprio smoke test
        "linear-readonly",
        concat!("linear ", "issue ", "close HRD-1"),
        "orca linear list --json",
    ),
    // `code-language` non giudica un comando ma una coppia percorso+testo, che
    // in questa tabella non ci sta. Il suo caso «deve bloccare» è dentro
    // `self_check`, insieme agli altri.
    (
        // Il titolo di una richiesta diventa l'oggetto del commit di fusione:
        // fuori formato deve fermarsi qui, in formato deve passare.
        // Niente apostrofi nel titolo di prova: chiuderebbe la stringa, lo
        // splitter di shell rinuncerebbe e il gancio tacerebbe — un caso «deve
        // bloccare» che passa per il motivo sbagliato.
        "pr-title",
        "gh pr create --title 'aggiustato il conteggio dei ganci'",
        "gh pr create --title 'fix(hooks): count the covered hooks'",
    ),
];

/// Esegue ogni gancio su due casi noti, in-process. Uscita 0 se tutti si
/// comportano come devono, 1 al primo che sbaglia — ed è il comando da lanciare
/// **prima** di far puntare la configurazione a questo binario, non dopo.
fn self_check() -> i32 {
    let mut failures = 0;
    for (name, must_block, must_pass) in SMOKE {
        for (command, expected_block) in [(must_block, true), (must_pass, false)] {
            let decision = match *name {
                "cd-guard" => guards::cd_guard::judge(command),
                "block-worktree-create" => guards::worktree_create::judge(command),
                "block-pr-merge-admin" => guards::pr_merge_admin::judge(command),
                // I fatti sono fissati qui: il caso deve dare lo stesso esito su
                // qualunque macchina, e un ramo letto dal disco lo renderebbe
                // dipendente da dove gira il controllo.
                "block-destructive" => {
                    guards::destructive_commands::judge(&guards::destructive_commands::Facts {
                        command,
                        cwd: "/home/someone/orca/general",
                        workspaces: "/home/someone/orca/workspaces",
                        home: "/home/someone",
                        is_link: &|_| false,
                        current_branch: &|_| None,
                        has_pending_changes: &|_, _| false,
                    })
                }
                // Il rifiuto viaggia sull'altro canale (`deny` su stdout,
                // uscita 0), quindi qui si guarda che il giudizio ci sia — non
                // che il gancio esca con codice 2.
                "pr-title" => guards::pr_title::judge(command),
                "linear-readonly" => match guards::linear_readonly::judge_bash(command) {
                    guards::linear_readonly::Verdict::Refused { reason, .. } => {
                        Decision::Block(reason)
                    }
                    _ => Decision::Pass,
                },
                _ => {
                    eprintln!("{name}: nessun caso di prova registrato");
                    failures += 1;
                    continue;
                }
            };
            // Un rifiuto viaggia su due canali: `Block` (uscita 2) e `Deny`
            // (messaggio su stdout, uscita 0). Guardare solo `Block` diceva
            // «non blocca» di ganci che rifiutano eccome — è il motivo per cui
            // `linear-readonly` qui sotto aveva una conversione scritta a mano.
            let blocked = matches!(decision, Decision::Block(_) | Decision::Deny(_));
            if blocked != expected_block {
                let atteso = if expected_block {
                    "blocco"
                } else {
                    "passaggio"
                };
                eprintln!("{name}: atteso {atteso} su {command:?}, ottenuto {decision:?}");
                failures += 1;
            }
        }
    }
    // `code-language` giudica una coppia percorso+testo, non un comando: non
    // entra nella tabella, ma deve passare di qui lo stesso — l'autoverifica
    // serve a dire che ogni gancio registrato decide, non che quasi tutti lo
    // fanno.
    let italian = guards::code_language::judge(
        "/x/a.test.ts",
        "it('rifiuta le date future', () => {})",
        true,
    );
    let english =
        guards::code_language::judge("/x/a.test.ts", "it('rejects future dates', () => {})", true);
    if italian.is_none() || english.is_some() {
        eprintln!("code-language: non distingue una descrizione italiana da una inglese");
        failures += 1;
    }

    // I tre ganci pronti per l'adozione e senza un caso qui. `adopt-hook.py`
    // interroga questa autoverifica prima di far puntare un gancio al binario:
    // il 18/08/2026 copriva 6 nomi su 33, e questi tre non c'erano — l'adozione
    // sarebbe stata cieca proprio dove la rete non c'e'.
    //
    // Tutti e tre giudicano dati, non comandi, e la funzione che decide e' pura:
    // niente Orca, niente `gh`, niente disco, niente orologio. Le funzioni che
    // parlano col mondo restano private apposta — chiamare `riconcilia()` o
    // `write_state()` da qui riscriverebbe lo stato di copie vive a ogni build.

    // `work-status`: quale stato scrivere su una copia di lavoro. La coppia
    // cambia una sola variabile — cosa e' rimasto che vive solo li' — perche' il
    // caso positivo da solo non distingue un giudice da uno stub che risponde
    // sempre «completed».
    let merged = serde_json::json!({
        "git": { "isMainWorktree": false, "branch": "refs/heads/suite-229-tabella" }
    });
    let requests: std::collections::BTreeMap<String, String> =
        [("suite-229-tabella".to_string(), "MERGED".to_string())]
            .into_iter()
            .collect();
    let clean = work_status::state_giusto(&merged, &requests, Some(0)).unwrap_or_default();
    if clean != "completed" {
        eprintln!("work-status: a merged request with nothing left behind must be completed, got {clean:?}");
        failures += 1;
    }
    // Sette commit scritti dopo la fusione: la richiesta e' unita, il lavoro no.
    // E' il caso vero di `a-client/media-link-recovery`, e smontare quella copia
    // avrebbe perso quei commit.
    let leftovers = work_status::state_giusto(&merged, &requests, Some(7)).unwrap_or_default();
    if leftovers != "in-progress" {
        eprintln!("work-status: a merged request holding local-only commits must stay in-progress, got {leftovers:?}");
        failures += 1;
    }

    // `spotlight-marker`: riconosce un comando che ricrea un albero di
    // dipendenze. Il comando vero non e' quasi mai nudo — arriva dietro un `cd`
    // — e stringere il riconoscimento a `starts_with` e' la correzione «ovvia»
    // che lo romperebbe in silenzio: esce 0 e la node_modules resta indicizzata.
    if !guards::spotlight_marker::is_an_install("cd /tmp && pnpm install") {
        eprintln!("spotlight-marker: does not recognise an install behind a leading cd");
        failures += 1;
    }
    if guards::spotlight_marker::is_an_install("git status") {
        eprintln!("spotlight-marker: treats an ordinary command as an install");
        failures += 1;
    }

    // `orca-cleanup`: quale scheda si puo' chiudere. I due terminali sono
    // identici — anonimo, fermo da 90 minuti — e cambia solo se dentro c'e' un
    // agente al lavoro. Chiudere quella e' il danno peggiore che il gancio possa
    // fare, ed e' il motivo per cui il caso negativo vale piu' del positivo.
    let now_ms = 1_000_000_000_000.0_f64;
    let term = serde_json::json!({
        "title": "Terminal 12",
        "handle": "term_selfcheck",
        "tabId": "sc-t1",
        "leafId": "sc-l1",
        "lastOutputAt": now_ms - 90.0 * 60_000.0,
    });
    let nobody: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    let (idle_closed, _) = orca_cleanup::judge(&term, 30.0, false, now_ms, &nobody);
    if !idle_closed {
        eprintln!("orca-cleanup: an anonymous tab idle for 90 minutes is not closed");
        failures += 1;
    }
    let mut working: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    working.insert(
        "sc-t1:sc-l1".to_string(),
        serde_json::Value::String("working".to_string()),
    );
    let (busy_closed, _) = orca_cleanup::judge(&term, 30.0, false, now_ms, &working);
    if busy_closed {
        eprintln!("orca-cleanup: would close a tab with an agent still working");
        failures += 1;
    }

    // Stessa forma per `comment-refs`, che giudica una coppia percorso+testo. Il
    // caso «deve passare» cita un percorso di CODICE: la regola lo lascia stare
    // di proposito, ed è quello che distingue questo freno da uno che nega ogni
    // commento — senza, un porto rotto in quel verso passerebbe l'autoprova.
    let to_a_document =
        guards::comment_refs::judge("/x/src/a.ts", "// ADR 0008 #61: rimosso", false);
    let to_a_source_file = guards::comment_refs::judge(
        "/x/src/a.ts",
        "// rispecchia il contratto di src/api/schema.ts",
        false,
    );
    if !matches!(to_a_document, Decision::Deny(_)) || !matches!(to_a_source_file, Decision::Pass) {
        eprintln!("comment-refs: non distingue un rimando a un documento da un percorso di codice");
        failures += 1;
    }

    // `legacy-script` giudica un percorso, non un comando, e i due bracci che
    // contano sono opposti: lo script che decide dentro la configurazione va
    // negato, quello di avvio a freddo deve passare. Un porto rotto che negasse
    // tutto supererebbe un'autoprova a un braccio solo, e spegnerebbe la casa al
    // primo avvio senza binario.
    let a_deciding_script =
        guards::legacy_script::judge("/home/someone/.claude/scripts/deposito-guasti.sh", true, false);
    let a_cold_start_script = guards::legacy_script::judge(
        "/home/someone/.claude/scripts/rust-hooks-present.sh",
        true,
        false,
    );
    if !matches!(a_deciding_script, Decision::Deny(_))
        || !matches!(a_cold_start_script, Decision::Pass)
    {
        eprintln!("legacy-script: non distingue uno script che decide dall'avvio a freddo");
        failures += 1;
    }

    // Anche il rilevatore di copie ha bisogno di due file per decidere, non di
    // un comando: il suo caso vive in una cartella temporanea.
    if let Err(why) = duplication::self_check() {
        eprintln!("duplication: {why}");
        failures += 1;
    }

    if failures == 0 {
        // Il messaggio diceva «N ganci, tutti rispondono come devono» contando
        // i soli provati: chi lo leggeva capiva «tutto il binario è a posto»,
        // mentre 17 ganci su 22 non avevano nessun caso. Poi si è scoperto che
        // quel 17 fondeva due domande diverse (20/08/2026): quante righe qui
        // sotto le stampano separate, con la propria definizione.
        let self_checked = ALL_HOOKS.iter().filter(|h| self_check_covers(h)).count();
        let module_tested = ALL_HOOKS.iter().filter(|h| has_module_test(h)).count();
        let uncovered: Vec<&str> = ALL_HOOKS
            .iter()
            .copied()
            .filter(|h| !is_covered(h))
            .collect();
        println!(
            "{self_checked} su {} passano un caso dentro self_check (tabella SMOKE + blocchi dedicati)",
            ALL_HOOKS.len()
        );
        println!(
            "{module_tested} su {} hanno un #[test] nel proprio modulo o nel crate a cui delegano",
            ALL_HOOKS.len()
        );
        if uncovered.is_empty() {
            println!("nessun gancio è scoperto da entrambe le letture");
        } else {
            println!("senza nessuna delle due: {}", uncovered.join(", "));
        }
        0
    } else {
        eprintln!("{failures} controlli falliti: NON pubblicare questo binario");
        1
    }
}

fn run(which: &str) -> Result<i32, String> {
    match which {
        "cd-guard" => {
            let mode = Mode::from_env("CD_GUARD");
            if mode == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0); // invocato fuori contesto
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = mode.soften(guards::cd_guard::judge(input.bash_command()));
            Ok(emit_with_legacy_prefix("cd-guard", &decision))
        }
        "instincts" => {
            // Porto di `inject-instincts.sh`: stesso interruttore, stessa
            // cartella, ma la scelta di cosa iniettare è in `guards::instincts`.
            // Nessuno stdin da leggere: SessionStart non ne manda uno che serva.
            let home = std::env::var("HOME").unwrap_or_default();
            let base = std::path::PathBuf::from(&home).join(".claude/homunculus");
            if base.join("disabled").exists() {
                return Ok(0);
            }
            let Ok(entries) = std::fs::read_dir(base.join("instincts/personal")) else {
                return Ok(0);
            };
            // `None` è un file trovato ma non leggibile: passa lo stesso a
            // `select`, che lo conta come «senza misura» invece di perderlo.
            let mut instincts: Vec<(String, Option<String>)> = entries
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let text = std::fs::read_to_string(e.path()).ok();
                    (name, text)
                })
                .collect();
            if instincts.is_empty() {
                return Ok(0);
            }
            instincts.sort_by(|a, b| a.0.cmp(&b.0));
            let today: String = hook_io::local_time::now_local_iso8601()
                .chars()
                .take(10)
                .collect();
            let rendered = guards::instincts::select(&instincts, &today);
            println!("{}", json_tool::context("SessionStart", &rendered.text));
            Ok(0)
        }
        "message-budget" => {
            let mode = Mode::from_env("MESSAGE_BUDGET");
            if mode == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0); // invocato fuori contesto
            };
            if !input.is_tool("SendMessage") {
                return Ok(0);
            }
            let message = input
                .tool_input
                .as_ref()
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // `soften` ammorbidisce solo un blocco; qui il verdetto è un
            // diniego di permesso, quindi la valvola `avvisa` si applica a mano.
            let decision = match (mode, guards::message_budget::judge(message)) {
                (Mode::WarnOnly, Decision::Deny(m)) => Decision::Warn(m),
                (_, d) => d,
            };
            Ok(emit_with_legacy_prefix("message-budget", &decision))
        }
        "block-worktree-create" => {
            let mode = Mode::from_env("BLOCK_WORKTREE_CREATE");
            if mode == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0); // invocato fuori contesto
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = mode.soften(guards::worktree_create::judge(input.bash_command()));
            Ok(emit_with_legacy_prefix("block-worktree-create", &decision))
        }
        "block-pr-merge-admin" => {
            // Nessuna valvola: è l'unico freno della configurazione che
            // difende un'azione irreversibile fatta su un repo condiviso, e una
            // variabile d'ambiente non deve poterlo togliere. Chi ha davvero
            // bisogno del bypass lo esegue da sé, fuori dalla sessione.
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let decision = guards::pr_merge_admin::judge(input.bash_command());
            Ok(emit_with_legacy_prefix("block-pr-merge-admin", &decision))
        }
        // `observe` registra e sveglia l'osservatore. È il gancio più caldo di
        // tutti, perché gira due volte per ogni chiamata.
        //
        // E DA OGGI DECIDE UNA COSA SOLA, per una ragione che va scritta o
        // sembrerà un abuso: **è l'unico gancio chiamato su ogni strumento**, e
        // il controllo che serviva riguarda `AskUserQuestion`, che nessun altro
        // gancio vede. L'alternativa era una riga nuova in `settings.json` —
        // file che nessuna sessione può scrivere, per costruzione. Metterlo qui
        // è ciò che rende il controllo **eseguibile senza la mano di Theo**, che
        // è esattamente il collo di bottiglia che il controllo esiste per
        // togliere.
        //
        // Il giudizio è cablato per uscire subito su qualunque altro strumento:
        // un errore qui fermerebbe la nave intera, non una domanda.
        //
        // LA FASE `permission` DICHIARA UNA SESSIONE FERMA, `pre`/`post` LA
        // SGOMBERANO: nessuna delle due decide niente (nessun exit code, nessun
        // messaggio), quindi «decide una cosa sola» resta vero — qui si scrive
        // solo un marcatore che `permission-stall` legge da fuori. La fase
        // `permission` non è ancora cablata in `settings.json`: nessuna sessione
        // può scriverlo, quindi il gesto aspetta Theo.
        "observe" => {
            let phase = std::env::args().nth(2).unwrap_or_else(|| "post".into());
            let mut raw = String::new();
            use std::io::Read as _;
            let _ = std::io::stdin().read_to_string(&mut raw);
            hook_io::observations::record(&phase, &raw);
            hook_io::observations::wake_observer();
            match phase.as_str() {
                "permission" | "PermissionRequest" => permission_stall::declare(&raw),
                "pre" | "PreToolUse" | "post" | "PostToolUse" => permission_stall::clear(&raw),
                _ => {}
            }
            if phase == "pre" {
                let verdict = ask_routing_verdict(&raw);
                if verdict != Decision::Pass {
                    return Ok(hook_io::emit("ask-routing", &verdict));
                }
            }
            Ok(0)
        }
        "pr-title" => {
            if Mode::from_env("TITOLO_RICHIESTA") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            Ok(hook_io::emit(
                "pr-title",
                &guards::pr_title::judge(input.bash_command()),
            ))
        }
        "hooks-off" => {
            if Mode::from_env("GANCI_SPENTI") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            if !input.is_tool("Bash") {
                return Ok(0);
            }
            let default_dir = input
                .cwd
                .clone()
                .or_else(|| std::env::var("CLAUDE_PROJECT_DIR").ok())
                .unwrap_or_default();
            let decision = guards::hooks_off::judge(input.bash_command(), &default_dir);
            Ok(hook_io::emit("hooks-off", &decision))
        }
        "socraticode-gate" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            let ws = guards::socraticode_gate::Workspace::from_env();
            let verdict = guards::socraticode_gate::judge(&ws, &input);
            guards::socraticode_gate::record(
                &verdict,
                input.tool_name.as_deref().unwrap_or(""),
                input.session_id.as_deref().unwrap_or("nosession"),
            );
            // Il messaggio esce senza il prefisso comune: l'originale scriveva
            // il testo nudo, e quel testo contiene già il nome del gate nella
            // prima riga. Aggiungerlo cambierebbe ciò che il modello legge.
            if let hook_io::Decision::Block(m) = &verdict.decision {
                eprintln!("{m}");
                return Ok(2);
            }
            // La regola arriva da qui, non dal prologo. Il frontmatter `paths:`
            // non sa distinguere un repo indicizzato da uno qualunque — vede
            // solo il percorso relativo alla cartella di lavoro — e consegnarla
            // dove l'indice non c'è costa senza produrre niente.
            if guards::socraticode_gate::rule_is_due(&ws, &input) {
                if let Some(testo) = socraticode_rule_text(&ws) {
                    let session = input.session_id.as_deref().unwrap_or("nosession");
                    if guards::socraticode_gate::claim_rule(&ws, session) {
                        println!(
                            "{}",
                            serde_json::json!({
                                "hookSpecificOutput": {
                                    "hookEventName": "PreToolUse",
                                    "additionalContext": testo,
                                }
                            })
                        );
                        // La riga di registro è l'unico modo di accorgersi che
                        // questa via è morta: il prologo non elenca ciò che ha
                        // caricato, quindi una consegna che non arriva non si
                        // vede da dentro la sessione.
                        hook_io::journal::record(
                            "socraticode-gate",
                            "consegna",
                            "regola-consegnata",
                            &[
                                ("sessione", session.into()),
                                ("byte", (testo.len() as i64).into()),
                            ],
                        );
                    }
                }
            }
            Ok(0)
        }
        // Il codice ricopiato. Due fasi con mestieri diversi: `pre` elenca la
        // famiglia di un file che sta per nascere, `post` misura i blocchi
        // identici. È l'unico gancio portato finora il cui tempo non è avvio
        // dell'interprete ma lavoro vero — albero, letture, sottosequenza comune.
        "duplication" => {
            // `--congela` non e' un gancio: e' il verbo che crea la linea di
            // base, e va eseguito **prima** di leggere stdin — altrimenti resta
            // in attesa di un JSON che nessuno gli manda.
            let args: Vec<String> = std::env::args().skip(2).collect();
            let bare = args.iter().find(|a| !a.starts_with("--"));
            if args.iter().any(|a| a == "--congela") {
                return Ok(duplication::freeze(bare.map(String::as_str)));
            }
            if args.iter().any(|a| a == "--debito") {
                return Ok(duplication::debt(bare.map(String::as_str)));
            }
            if args.iter().any(|a| a == "--scan") {
                return Ok(duplication::scan(bare.map(String::as_str)));
            }
            if Mode::from_env("DUPLICAZIONE") == Mode::Off {
                return Ok(0);
            }
            let phase = std::env::args().nth(2).unwrap_or_else(|| "post".into());
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(duplication::run(&input, &phase))
        }
        // L'italiano dove la convenzione chiede l'inglese. Due fasi: registrato
        // su `pre`, dove il rifiuto viaggia su stdout e il file non viene
        // scritto. Su `post` avviserebbe a cose fatte — «un avviso dopo lascia
        // il file scritto in italiano», dice l'originale, ed è il motivo per
        // cui la fase registrata è la prima.
        "code-language" => {
            if Mode::from_env("LINGUA_CODICE") == Mode::Off {
                return Ok(0);
            }
            let phase = std::env::args().nth(2).unwrap_or_else(|| "pre".into());
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            let empty = serde_json::json!({});
            let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Stessa regola, altra porta. Senza questo ramo il gate vedeva solo
            // `Write`/`Edit`, e chi scriveva con `cat > file <<EOF` passava
            // intatto — misurato dal vivo il 18/08/2026. Si giudica solo il
            // primo file sorvegliato che il comando scrive: un messaggio che ne
            // elenca cinque non lo legge nessuno.
            if path.is_empty() {
                if !input.is_tool("Bash") {
                    return Ok(0);
                }
                let command = input.bash_command();
                for (target, body) in guards::code_language::writes_from_bash(&command) {
                    let exists = std::path::Path::new(&target).exists();
                    let Some(message) = guards::code_language::judge(&target, &body, exists) else {
                        continue;
                    };
                    if phase == "pre" {
                        return Ok(hook_io::emit(
                            "code-language",
                            &hook_io::Decision::Deny(message),
                        ));
                    }
                    eprintln!("{message}");
                    return Ok(2);
                }
                // E le scritture che non sappiamo leggere. Un file sorvegliato
                // che nasce dentro `python3 - <<PY … write_text …` passava
                // intatto: misurato il 19/08/2026, è la via da cui era passato
                // tutto il codice di quella notte. Qui non si indovina il
                // contenuto — dentro un interprete può nascere da una variabile
                // — si dice che da lì non si vede, e si manda a `Write`/`Edit`.
                let opachi = guards::code_language::opaque_writes(&command);
                if !opachi.is_empty() {
                    let message = guards::code_language::report_opaque(&opachi);
                    if phase == "pre" {
                        return Ok(hook_io::emit(
                            "code-language",
                            &hook_io::Decision::Deny(message),
                        ));
                    }
                    eprintln!("{message}");
                    return Ok(2);
                }
                return Ok(0);
            }
            let text = guards::code_language::written_text(tool_input);
            let exists = std::path::Path::new(path).exists();
            let Some(message) = guards::code_language::judge(path, &text, exists) else {
                return Ok(0);
            };
            let decision = if phase == "pre" {
                hook_io::Decision::Deny(message)
            } else {
                hook_io::Decision::Block(message)
            };
            // Senza prefisso: il messaggio è già un rapporto intero, e la prima
            // riga dice da sola di che si tratta.
            match decision {
                hook_io::Decision::Deny(m) => {
                    Ok(hook_io::emit("code-language", &hook_io::Decision::Deny(m)))
                }
                hook_io::Decision::Block(m) => {
                    eprintln!("{m}");
                    Ok(2)
                }
                _ => Ok(0),
            }
        }
        // I rimandi a documenti locali nei commenti. Stesso involucro di
        // `code-language`: entrambi guardano il testo appena scritto e negano
        // prima della scrittura, perché un avviso dopo lascia la riga sul file.
        // L'esenzione la legge il chiamante — `judge` è pura per poter essere
        // confrontata col Python sulla sola decisione.
        "comment-refs" => {
            if Mode::from_env("COMMENT_REFS") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            // La fase si prende dall'evento, non dall'argomento. L'argomento è
            // una parola che qualcuno deve ricopiare in `settings.json`, e
            // prima o poi non la ricopia: il 18/08/2026 `forget_session` non
            // era mai girata perché `--fine` era scritto sul solo ripiego
            // Python mentre a girare era il binario. Resta come ripiego, per
            // poter provare il gancio dalla riga di comando.
            let phase = match input.hook_event_name.as_deref() {
                Some("PreToolUse") => "pre".to_string(),
                Some("PostToolUse") => "post".to_string(),
                _ => std::env::args().nth(2).unwrap_or_else(|| "pre".into()),
            };
            let empty = serde_json::json!({});
            let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Ok(0);
            }
            let text = guards::code_language::written_text(tool_input);
            let exempt = std::fs::read_to_string(path)
                .map(|c| guards::comment_refs::declares_marker(&c))
                .unwrap_or(false);
            let hook_io::Decision::Deny(message) = guards::comment_refs::judge(path, &text, exempt)
            else {
                return Ok(0);
            };
            // In fase `post` non si può più negare: si blocca con uscita 2, che
            // è il canale che l'assistente legge. Il codice d'uscita di `pre`
            // resta 0 — la negazione viaggia dentro lo stdout, ed è il motivo
            // per cui la prima misura di questo freno lo credette muto.
            if phase == "pre" {
                Ok(hook_io::emit(
                    "comment-refs",
                    &hook_io::Decision::Deny(message),
                ))
            } else {
                eprintln!("{message}");
                Ok(2)
            }
        }
        // Il linguaggio deprecato dentro la configurazione. Nega e basta: non ha
        // una fase `post` perché a cose scritte non c'è più niente da decidere,
        // e il messaggio serve **prima** che il lavoro sia fatto.
        //
        // L'ESISTENZA DEL FILE SI GUARDA QUI E NON NEL GIUDIZIO, perché il
        // giudizio dev'essere puro: è quello che permette di provarlo su
        // percorsi che non esistono su nessuna macchina.
        "legacy-script" => {
            if Mode::from_env("LEGACY_SCRIPT") == Mode::Off {
                return Ok(0);
            }
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            let empty = serde_json::json!({});
            let tool_input = input.tool_input.as_ref().unwrap_or(&empty);
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.is_empty() {
                return Ok(0);
            }
            let exists = std::path::Path::new(path).exists();
            // UNA VOLTA PER FILE E PER SESSIONE, e non è un dettaglio di forma:
            // a messaggio pieno per ogni gesto questo freno costerebbe 424
            // dinieghi nel giorno peggiore misurato, lo stesso ordine di
            // grandezza che ha già fatto respingere la bozza di un altro freno.
            // Il marcatore è la forma che usa il gate della ricerca — un file
            // che nasce con `create_new` — riusata invece di inventarne una.
            let session = input.session_id.as_deref().unwrap_or("ignota");
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                path.hash(&mut h);
                h.finish()
            };
            let marker = std::env::temp_dir().join(format!("claude-legacy-{session}-{key:x}"));
            let already_said = marker.exists();
            let hook_io::Decision::Deny(message) =
                guards::legacy_script::judge(path, exists, already_said)
            else {
                return Ok(0);
            };
            // Il posto si prende DOPO il giudizio: se il verdetto fosse `Pass`,
            // segnare il file brucerebbe la spiegazione per un gesto che non è
            // mai stato negato.
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker);
            Ok(hook_io::emit(
                "legacy-script",
                &hook_io::Decision::Deny(message),
            ))
        }
        // Le regole appena scritte. Nessuna valvola d'ambiente: l'originale non
        // ne aveva una, e aggiungerla qui vorrebbe dire che il porting cambia
        // ciò che si può spegnere — una decisione, non una traduzione.
        "live-rules" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(live_rules::run(&input))
        }
        // Il divieto su Linear: 813 righe di Python, il gancio più grande del
        // parco. Il giudizio sta in `guards::linear_readonly` ed è puro; la
        // parte con stato — permesso di Theo e registro — in `linear.rs`.
        // Una consegna appena scritta arma la sessione dopo — una sola.
        "handoff-arms-successor" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(successor::run(&input))
        }
        "linear-readonly" => {
            // Nessuna valvola d'ambiente: il mandato dell'11/08/2026 non
            // prevede che una variabile lo tolga. Le tre valvole che esistono
            // stanno dentro il giudizio, e la più forte non la digita l'agente.
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(linear::run(&input))
        }
        // Non un gancio: l'interrogazione che permette allo strumento di
        // equivalenza di chiedere al Rust la stessa cosa che chiede al Python.
        // Senza, il porting della misura di consegna si proverebbe solo sui casi
        // scritti a mano — e sono proprio quelli a non trovare i difetti.
        "handoff-measure" => {
            let mut args = std::env::args().skip(2);
            let Some(transcript) = args.next() else {
                return Err("handoff-measure vuole il percorso di un transcript".into());
            };
            Ok(handoff::measure(
                &transcript,
                &args.next().unwrap_or_default(),
            ))
        }
        // Stessa ragione: l'elenco dei pannelli arriva da stdin perché le due
        // implementazioni devono giudicare lo stesso elenco, non due letture a
        // un secondo di distanza.
        // L'interrogazione del gancio che arma il successore, stessa ragione.
        // La staffetta, un passo. `--secco` non rigenera nessuna sessione ma
        // NON e' a vuoto: i record dei terminali morti li cancella lo stesso.
        "relay" => Ok(relay::step(std::env::args().any(|a| a == "--secco"))),
        // Il promemoria alla sessione che riparte da un riassunto. Legge il
        // JSON di SessionStart da stdin e, solo dopo una compattazione, parla.
        "restart-notice" => Ok(restart::run()),
        // Il promemoria sui dati personali spediti a un modello. La regola
        // gemella non basta da sola: il suo `paths:` si valuta sull'albero
        // della sessione, e l'84,5% dei tocchi cade fuori.
        "ai-personal-data" => {
            let Some(input) = hook_io::read_input() else {
                return Ok(0);
            };
            Ok(ai_personal_data::run(&input))
        }
        // Il presidio della consegna, lato PostToolUse.
        "handoff-required" => Ok(handoff_required::run()),
        "handoff-on-stop" => Ok(handoff_on_stop::run()),
        // Il porto del gancio `PreCompact`, 25/08/2026: legge il JSON da
        // stdin e scrive la consegna in un posto che un riavvio non cancella.
        // Non ancora acceso: la riga in `settings.json` è di Theo.
        "handoff-precompact" => Ok(handoff_precompact::run()),
        "allow-worktree-deletes" => Ok(worktree_deletes::run()),
        // Il gemello dal lato che nega. Nessuna valvola d'ambiente: quella che
        // c'è sta sulla riga di comando, dove resta scritta nel registro invece
        // di disarmare in silenzio tutto ciò che viene dopo un export.
        "block-destructive" => Ok(destructive_commands::run()),
        // La sessione che cambia mestiere in corsa. Sta su PostToolUse `*`,
        // quindi è insieme a `observe` il gancio che parte più spesso: il suo
        // lavoro è quasi niente, e quasi tutto il costo era l'avvio di Python.
        // La valvola resta quella dell'originale, `SCOPE_DRIFT=off`.
        "scope-drift" => Ok(scope_drift::run()),
        // Non è un gancio: è l'aggancio dello strumento di equivalenza, che pone
        // la stessa domanda ai due conteggi sullo stesso transcript vero.
        "restart-count" => {
            let path = std::env::args().nth(2).unwrap_or_default();
            Ok(restart::count_probe(&path))
        }
        "successor-probe" => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
            Ok(successor::probe(&arg(0), &arg(1), &arg(2)))
        }
        // Non è un gancio: è `latest_handoff` esposta in sola lettura, perché una
        // correzione su quale consegna eredita il successore si verifica sul
        // binario che gira, non sul sorgente che si legge.
        "handoff-latest" => {
            let cwd = std::env::args().nth(2).unwrap_or_default();
            println!("{}", relay::latest_handoff(&cwd));
            Ok(0)
        }
        // Non è un gancio: è il consiglio di `guards::repo_tools` interrogabile
        // dall'esterno, perché la misura che lo giustifica si prende sui comandi
        // veri dei transcript e non sui casi scritti a mano. Il comando arriva da
        // stdin, il repo come argomento.
        "repo-tools" => {
            let dir = std::env::args().nth(2).unwrap_or_default();
            let mut command = String::new();
            use std::io::Read;
            let _ = std::io::stdin().read_to_string(&mut command);
            let pkg = std::fs::read_to_string(std::path::Path::new(&dir).join("package.json"))
                .unwrap_or_default();
            let said = guards::repo_tools::advice(command.trim(), &pkg);
            if !said.is_empty() {
                println!("{said}");
            }
            Ok(0)
        }
        "handoff-resolve" => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
            Ok(handoff::resolve(&arg(0), &arg(1), &arg(2)))
        }
        // Nemmeno questo è un gancio: è la decisione della staffetta esposta a
        // `tools/compare-relay-evaluate.py`, che pone la stessa domanda a
        // `relay.evaluate()` con una HOME finta e pretende la stessa risposta.
        "relay-evaluate" => Ok(relay_eval::run()),
        // Il gemello per il freno della catena, interrogato da
        // `tools/compare-relay-chain.py`.
        "relay-chain" => Ok(relay_eval::run_chain()),
        // Il terzo, per la lettura da disco: lo stesso confronto, ma con la
        // guardia sull'albero ricreato in mezzo.
        "relay-read-chain" => Ok(relay_eval::run_read_chain()),
        // Il quarto: quali file di stato restano senza albero.
        "relay-sweep" => Ok(relay_eval::run_sweep()),
        // I quattro porti del 17/08: rispondono già, ma la configurazione li
        // nomina solo quando il confronto col Python è verde e i mutanti sono
        // uccisi. L'ordine è quello di `adopt-hook.py`: prima si dimostra, poi
        // si registra.
        "handoff-threshold" => Ok(handoff_threshold::run()),
        "long-session" => Ok(long_session::run()),
        "hook-census" => Ok(hook_census::run()),
        "link-worktree-rules" => Ok(link_worktree_rules::run()),
        "spotlight-marker" => Ok(spotlight_marker::run()),
        "orca-cleanup" => Ok(orca_cleanup::run()),
        "register-session" => Ok(register_session::run()),
        "skill-nudge" => Ok(skill_nudge::run()),
        "work-status" => Ok(work_status::run()),
        "squash-orphans" => Ok(squash_orphans::run()),
        "index-freshness" => Ok(index_freshness::run()),
        "allow-session-messages" => Ok(session_messages::run()),
        // Nemmeno questo è un gancio: legge i registri delle automazioni e apre
        // da sé una voce di coda quando una riga marcata si ripete oltre soglia.
        // Lo chiama la ronda della coda a ogni giro.
        "fault-deposit" => Ok(fault_deposit::run()),
        // Nemmeno questo è un gancio: dichiara un mestiere della configurazione
        // per la sessione che lo chiama, a mano.
        "role-claim" => Ok(role_claim::run()),
        // Non e un gancio: e lo strumento che risponde a «chi lancia
        // ancora questo controllo». Sta nel dispatch perche il binario e
        // gia il posto dove vive la logica, e un secondo eseguibile per
        // una misura sarebbe una superficie in piu da tenere viva.
        "reachability" => Ok(reachability::run()),
        // Nemmeno questo è un gancio: risponde a «questa memoria parla ancora
        // del codice di adesso?», e nessuna radice lo invoca. Chi lo lancia è un
        // servizio settimanale o una sessione che sta per agire su una memoria.
        "memory-anchors" => Ok(memory_anchors::run()),
        // Nega una scrittura su una memoria se introduce una citazione a un
        // file che non si trova su nessuna radice. Stesso involucro di
        // `comment-refs`: guarda il testo che sta per essere scritto e nega
        // prima, perché un avviso dopo lascia la citazione morta sul file.
        "memory-citation-gate" => Ok(memory_citation_gate::run()),
        // Non è un gancio: è lo strumento che risponde a «quali affermazioni
        // dicono di aver misurato qualcosa, e quando». Sta qui perché il suo
        // gemello Python non è più eseguibile da dentro una sessione.
        "stale-facts" => Ok(stale_facts::run()),
        // Non è un gancio: risponde a «questa cosa è autorizzata?» leggendo il
        // registro che il capitano scrive, senza che chi esegue debba
        // chiedere a nessuno. La chiave arriva come argomento.
        "authorization-check" => Ok(authorizations::run()),
        // La penna dello stesso registro: SCRIVE SOLO IL CAPITANO. Non è un
        // gancio, nessuna radice lo invoca — lo digita a mano chi ha appena
        // ricevuto una decisione di Theo da trascrivere. La procedura sta in
        // `docs/procedura-autorizzazioni.md`.
        "captain-authorize" => Ok(authorizations::run_write()),
        // Non è un gancio: ripassa i marcatori che il congedo di una sessione
        // non ha potuto buttare, col GIUDIZIO DEL CONGEDO e non con l'età nuda.
        // Senza `--delete` racconta e basta. NON È IN SERVIZIO: la sveglia
        // periodica la mette il capitano, dopo un verdetto indipendente.
        "marker-sweep" => Ok(marker_sweep::run()),
        // Non è un gancio: legge i marcatori che `observe permission` scrive
        // e stampa quali sessioni sono ferme su un permesso — il comando da
        // riga di comando che risponde a «è bloccata?» da fuori.
        "permission-stall" => Ok(permission_stall::run_report()),
        "fili-scoperti" => Ok(uncovered_thread::run_report()),
        // Non è un gancio: quale voce di coda si dispaccerebbe ora, senza
        // scrivere niente — lo stesso contratto di sola lettura di
        // `fili-scoperti`, sulla terza fonte del mandato.
        "mandato-coda" => Ok(queue_mandate::run_report()),
        // Il registro dei costi. `record` legge stdin come ogni gancio; gli
        // altri due verbi leggono i loro argomenti da riga di comando e non
        // aspettano niente su stdin — chiamarli senza un verbo noto non deve
        // bloccarsi in attesa di un ingresso che non arriva, quindi il ramo
        // di ripiego risponde ed esce invece di provare a leggere stdin.
        "costs" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            match args.first().map(String::as_str) {
                Some("backfill") => Ok(costs::backfill(&args[1..])),
                Some("report") => Ok(costs::report(&args[1..])),
                Some("record") | None => Ok(costs::record()),
                Some(other) => {
                    eprintln!("costs: unknown verb {other:?} (record | backfill | report)");
                    Ok(0)
                }
            }
        }
        // Il router di fase: `PreToolUse`/`Agent`, mai un diniego — o riscrive
        // `model` nell'input o non stampa niente.
        "phase-router" => Ok(phase_router::run()),
        "ronda-trigger" => Ok(ronda_trigger::run()),
        // Non è un gancio: legge un transcript vero e dice se lo schema che
        // tutte le misure danno per scontato regge ancora. Esce 1 quando muore
        // e 2 quando non ha potuto misurare, così chi lo chiama dalla ronda
        // distingue «rotto» da «non provato».
        "transcript-canary" => Ok(transcript_canary::run()),
        // Il governo della memoria: `PreToolUse` su Bash senza verbo, e col
        // verbo `report` la fotografia per chi guarda da fuori. Il rapporto
        // vuole `ps`, che dentro il sandbox è negato — per questo è un verbo a
        // parte e non una riga in più del gancio.
        "memory-governor" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            match args.first().map(String::as_str) {
                Some("report") => Ok(memory_governor::report()),
                None => Ok(memory_governor::run()),
                Some(other) => {
                    eprintln!("memory-governor: verbo sconosciuto {other:?} (report)");
                    Ok(0)
                }
            }
        }
        // `json` non è un gancio: è il pezzo che toglie `python3 -c` dai tre
        // ganci scritti in shell, che lo invocano per leggere un campo o
        // costruire una risposta. Sta nell'elenco perché il dispatch e l'elenco
        // si controllano a vicenda, non perché `settings.json` lo nomini.
        "json" => Ok(json_tool::run()),
        // Non è un gancio: accorcia un'uscita che gli si passa da stdin. Il
        // momento in cui dovrebbe girare da solo non esiste fra i ganci di
        // questa versione — il commento sul modulo dice cosa l'ha stabilito.
        "filter-output" => Ok(output_filter::run()),
        // `PreToolUse` su Bash: o riscrive il comando per farne passare
        // l'uscita dal filtro, o non stampa niente. Non nega mai.
        "wrap-bash" => Ok(bash_wrap::run()),
        // Non è un gancio: osserva la forma della casa e dice una cosa sola.
        // Non legge stdin — le sue opzioni stanno sulla riga di comando — e non
        // corregge niente: nessun pannello, nessun agente, nessuna scrittura
        // fuori da `state/` e dalla coda.
        "shape" => Ok(shape_exam::run()),
        // Non è un gancio: risponde a «questa voce di coda è ancora credibile?»
        // e a «qualcun altro ha già parlato della stessa cosa?». Senza `--mark`
        // racconta e basta; con `--mark` scrive il rilievo dentro le voci
        // interessate, e non ne apre nessuna di nuova.
        "queue-freshness" => Ok(queue_freshness::run()),
        // Il gemello sulle memorie: risponde a «questo piano è ancora un
        // ordine, o era il piano di quel giorno?». Senza `--mark` racconta e
        // basta; con `--mark` scrive il rilievo sopra la sezione operativa
        // delle consegne di sessioni chiuse, e non tocca una riga del testo che
        // qualcuno ha scritto a mano.
        "memory-freshness" => Ok(memory_freshness::run()),
        other => Err(format!("gancio sconosciuto: {other}")),
    }
}

/// I messaggi conservano il prefisso degli script Python (`BLOCCATO (cd-guard):`)
/// perché sono già citati nelle regole e nei documenti: cambiarli spezzerebbe i
/// rimandi senza migliorare niente.
fn emit_with_legacy_prefix(hook: &str, decision: &Decision) -> i32 {
    hook_io::emit(hook, decision)
}

/// Il testo della regola SocratiCode, senza frontmatter, o niente.
///
/// Il file resta la fonte unica: qui non c'è nessuna copia del testo, così una
/// modifica alla regola entra in servizio senza ricompilare il gancio. Se il
/// file manca o è vuoto il gancio tace — non difende niente, consegna.
fn socraticode_rule_text(ws: &guards::socraticode_gate::Workspace) -> Option<String> {
    let raw = std::fs::read_to_string(ws.home.join(guards::socraticode_gate::RULE_PATH)).ok()?;
    let body = guards::socraticode_gate::rule_body(&raw).to_string();
    (!body.trim().is_empty()).then_some(body)
}

/// La cartella dove ogni figura viva dichiara il proprio mestiere.
fn roles_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/someone".into());
    std::path::PathBuf::from(home).join(".claude").join("state").join("ruoli")
}

/// Il mestiere della sessione, se ne ha dichiarato uno.
fn role_of(session: &str) -> Option<String> {
    if session.is_empty() {
        return None;
    }
    let short: String = session.chars().take(8).collect();
    let raw = std::fs::read_to_string(roles_dir().join(short)).ok()?;
    let first = raw.lines().next()?.trim().to_string();
    (!first.is_empty()).then_some(first)
}

/// Il verdetto sull'apertura di un modulo, letto il payload del gancio.
///
/// FAIL-OPEN OVUNQUE, e non è pigrizia: questo gira davanti a **ogni** strumento
/// di **ogni** sessione. Un payload che non si legge, una cartella che non
/// risponde, un campo assente — tutto lascia passare. Il costo di un falso
/// diniego qui è la nave ferma; quello di un falso permesso è una domanda di
/// troppo a Theo, che è il difetto che si sta curando, non un disastro.
fn ask_routing_verdict(raw: &str) -> Decision {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Decision::Pass;
    };
    let tool = data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
    if tool != "AskUserQuestion" {
        return Decision::Pass; // il caso normale esce qui, prima di toccare il disco
    }
    let session = data.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
    let role = role_of(session);
    guards::ask_routing::judge(tool, role.as_deref())
}

#[cfg(test)]
mod catalogo {
    use super::*;

    /// I nomi dei rami del `match` di `run()`, letti da questo stesso sorgente.
    ///
    /// Un elenco scritto a mano accanto a un `match` diverge al primo gancio
    /// nuovo, e diverge in silenzio: chi aggiunge un ramo non ha motivo di
    /// sospettare che esista un secondo posto da aggiornare. Qui il secondo
    /// posto se ne accorge da solo.
    fn hooks_in_dispatch() -> Vec<String> {
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn run(which: &str)")
            .expect("la firma di run() è cambiata: aggiorna questo test")
            .1;
        let mut found = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            // i rami hanno la forma `"nome" => …`, eventualmente su più nomi
            let Some(rest) = t.strip_prefix('"') else {
                continue;
            };
            let Some((name, after)) = rest.split_once('"') else {
                continue;
            };
            if after.trim_start().starts_with("=>")
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                found.push(name.to_string());
            }
        }
        found.sort();
        found.dedup();
        found
    }

    #[test]
    fn ogni_gancio_del_dispatch_e_elencato() {
        let dispatch = hooks_in_dispatch();
        assert!(
            !dispatch.is_empty(),
            "nessun ramo trovato: il lettore del sorgente non funziona più"
        );
        let missing: Vec<&String> = dispatch
            .iter()
            .filter(|h| !ALL_HOOKS.contains(&h.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "ganci nel dispatch ma non in ALL_HOOKS: {missing:?}"
        );
    }

    #[test]
    fn nessun_gancio_elencato_e_sconosciuto_al_dispatch() {
        let dispatch = hooks_in_dispatch();
        let ghosts: Vec<&&str> = ALL_HOOKS
            .iter()
            .filter(|h| !dispatch.contains(&h.to_string()))
            .collect();
        assert!(
            ghosts.is_empty(),
            "ganci elencati che il dispatch non conosce: {ghosts:?}"
        );
    }

    #[test]
    fn no_declared_tool_is_unknown_to_the_dispatch() {
        let ghosts: Vec<&&str> = NOT_HOOKS
            .iter()
            .filter(|s| !ALL_HOOKS.contains(s))
            .collect();
        assert!(
            ghosts.is_empty(),
            "dichiarati non-ganci, ma il dispatch non li conosce: {ghosts:?}"
        );
    }

    /// Il vincolo che rende utile la dichiarazione: uno slug esce dai ganci
    /// perché è uno strumento, non perché così il conto torna. Questi tre
    /// GIUDICANO — e restano ganci anche mentre nessuna radice li accende.
    #[test]
    fn whatever_judges_stays_a_hook_even_while_switched_off() {
        for name in [
            "block-worktree-create",
            "comment-refs",
            "allow-session-messages",
        ] {
            assert!(
                is_hook(name),
                "{name} giudica: toglierlo dai ganci spegnerebbe la domanda invece di rispondere"
            );
        }
    }

    #[test]
    fn hooks_declared_self_checked_have_a_real_case() {
        for name in SELF_CHECK_EXTRA {
            assert!(
                ALL_HOOKS.contains(name),
                "{name} è dichiarato provato ma non è un gancio"
            );
            assert!(
                !SMOKE.iter().any(|(n, _, _)| n == name),
                "{name} è contato due volte: sta in SMOKE e in SELF_CHECK_EXTRA"
            );
        }
    }

    #[test]
    fn self_check_runs_from_here() {
        // `self_check()` girava solo dentro il binario: togliere il caso di un
        // gancio lasciava i test verdi, e se ne accorgeva soltanto chi lanciava
        // `--check` a mano. Ora la stessa domanda la fa anche `cargo test`.
        assert_eq!(self_check(), 0, "l'autoverifica del binario non passa");
    }

    #[test]
    fn self_check_is_not_total_and_says_so() {
        // Solo 11 ganci girano dentro `self_check()`: gli altri hanno prove nel
        // modulo, non un caso qui. Se un giorno self_check li esercitasse tutti
        // questo test cadrà, ed è il momento giusto per toglierlo.
        let n = ALL_HOOKS.iter().filter(|h| self_check_covers(h)).count();
        assert!(
            n < ALL_HOOKS.len(),
            "self_check ora esercita ogni gancio: togli questo test"
        );
    }

    #[test]
    fn every_self_check_extra_has_a_block_in_self_check() {
        // Il nome deve comparire nel corpo di `self_check()`: tiene onesto
        // SELF_CHECK_EXTRA contro il caso «dichiarato ma il blocco è stato
        // tolto». Mutazione che l'ha ucciso: togliere "work-status" da questo
        // sorgente lasciandolo in `SELF_CHECK_EXTRA` fa rossire questo test.
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn self_check() -> i32")
            .expect("la firma di self_check() è cambiata: aggiorna questo test")
            .1
            .split_once("fn run(which: &str)")
            .expect("la fine di self_check() è cambiata: aggiorna questo test")
            .0;
        for name in SELF_CHECK_EXTRA {
            assert!(
                body.contains(name),
                "{name} è in SELF_CHECK_EXTRA ma il suo blocco non è più in self_check()"
            );
        }
    }

    #[test]
    fn source_reader_recognises_a_test() {
        // La logica pura di `has_module_test`, isolata dal disco: prova diretta
        // sul riconoscimento del marcatore, non sulla mappa gancio→file.
        assert!(source_contains_test("fn f() {}\n#[test]\nfn g() {}"));
        assert!(!source_contains_test("fn f() {}\nfn g() {}"));
    }

    #[test]
    fn every_hook_has_an_entry_in_the_test_map() {
        // Stessa tecnica di `hooks_in_dispatch`: legge il proprio sorgente
        // invece di ripetere l'elenco, così un gancio nuovo senza voce qui
        // dentro si vede da sé invece di cadere silenziosamente nel `_ => &[]`
        // finale e risultare sempre «senza test-modulo».
        let source = include_str!("main.rs");
        let body = source
            .split_once("fn has_module_test(name: &str) -> bool {")
            .expect("la firma di has_module_test() è cambiata: aggiorna questo test")
            .1
            .split_once("\n    };\n")
            .expect("il match di has_module_test() è cambiato: aggiorna questo test")
            .0;
        let mut found = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            let Some(rest) = t.strip_prefix('"') else {
                continue;
            };
            let Some((name, after)) = rest.split_once('"') else {
                continue;
            };
            if after.trim_start().starts_with("=>") {
                found.push(name.to_string());
            }
        }
        found.sort();
        let mut expected: Vec<String> = ALL_HOOKS.iter().map(|h| h.to_string()).collect();
        expected.sort();
        assert_eq!(
            found, expected,
            "la mappa gancio→sorgente in has_module_test non elenca esattamente ALL_HOOKS"
        );
    }

    #[test]
    fn known_modules_have_a_real_test() {
        // Ancoraggio ai nove porti che avevano prove non collegate qui, più
        // quelli scritti da allora: se un domani perdessero i test il rapporto
        // dovrebbe dirlo, non tacerlo.
        for name in [
            "register-session",
            "skill-nudge",
            "handoff-threshold",
            "scope-drift",
            "restart-notice",
            "observe",
            "link-worktree-rules",
            "handoff-on-stop",
            "handoff-required",
            "hook-census",
            "handoff-arms-successor",
            "allow-worktree-deletes",
            "allow-session-messages",
        ] {
            assert!(
                has_module_test(name),
                "{name} risultava senza test nel modulo: ha perso il suo #[test]?"
            );
        }
    }
}
