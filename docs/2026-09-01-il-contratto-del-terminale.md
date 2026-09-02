# Il contratto del terminale nella finestra

**01/09/2026.** Scritto prima del lavoro, non dopo, perché due cantieri lo
costruiscono in parallelo — il ponte (Rust, dentro `desktop/src-tauri`) e la
finestra (React, dentro `desktop/src`) — e senza un contratto scritto si
incontrerebbero solo in fusione. Il guasto 31 è nato esattamente così: nove
agenti sullo stesso repo, quattro conflitti che `git` non vedeva perché erano
d'intenzione, non di righe.

**Questo file è la fonte per tutti e due.** Chi cambia il contratto lo cambia
qui e lo dice; chi lo scopre diverso dal codice apre un guasto invece di
adeguare in silenzio la propria metà.

## Perché esiste il cantiere

`crates/terminal` è finito e provato — 2.295 righe: pseudo-terminale vero,
scrittura, uscita mentre esce, ridimensionamento, chiusura, elenco degli aperti,
e lo smistamento di ciò che l'utente scrive come dati (`routing`). **Nessun
binario lo spedisce**: l'elenco dei comandi che la finestra espone
(`desktop/src-tauri/src/main.rs`) non ne ha nemmeno uno che riguardi un
terminale.

Senza terminale nella finestra, dentro Sailor non si tiene aperta una sessione
di un motore: è il pezzo che decide se Sailor può sostituire l'ambiente in cui
Theo lavora oggi.

## I nomi, e perché sono in inglese

Sono identificatori — li legge il compilatore e li scrive `invoke(...)` — quindi
inglese, come dice `AGENTS.md`. Ciò che legge una persona resta italiano.

## I comandi che il ponte espone

Sette dal 02/09/2026 (erano sei: `terminal_backlog` è entrato con la
sopravvivenza alla finestra), e nessuno di più senza aggiornare questo file.

| comando | argomenti | risposta |
|---|---|---|
| `terminal_open` | `{ workspaceRoot: string, program?: string, args?: string[], cols: number, rows: number }` | `{ id, workspaceRoot, workspaceName, alive, processId, device, moved }` |
| `terminal_submit` | `{ id: string, line: string }` | `{ kind: "command" } \| { kind: "flow", flow: string, text: string, rule: string }` |
| `terminal_press` | `{ id: string, bytes: string }` (base64) | `null` |
| `terminal_resize` | `{ id: string, cols: number, rows: number }` | `null` |
| `terminal_close` | `{ id: string }` | `null` |
| `terminal_list` | — | `[{ id, workspaceRoot, workspaceName, alive, processId, device, moved }]` |
| `terminal_backlog` | `{ id: string }` | `{ at: number, bytes: string (base64), upto: number, ended: string \| null }` |

`device` è il tty del programma dentro, in forma corta (`ttys004`): è
l'ancora di una scheda, la chiave della cassetta delle lettere e del conteggio.
`moved` sono i byte passati finora nelle due direzioni, lo stesso numero che
`sailor terminal list` stampa. `terminal_open` apre sotto i profili attivi:
l'ambiente del processo dentro porta `CLAUDE_CONFIG_DIR`, `CODEX_HOME` e le
altre variabili che `profiles::active_environment` ricava dal deposito dei
profili al momento dell'apertura.

`terminal_backlog` serve ciò che il terminale ha stampato prima che questo
pannello guardasse, fino a un limite dichiarato (`terminal::host::BACKLOG_LIMIT`),
e `upto` è l'offset da cui gli eventi vivi prendono il testimone: un pannello
scrive il backlog e poi solo gli eventi il cui `at` non è sotto `upto`.

La riga dell'elenco è `terminal::Summary`, che è già `Serialize`: **non si
ricopia in TypeScript un tipo che il Rust già dichiara** — è il guasto 10, che
in questo repo si è già ripresentato cinque volte.

Ogni comando torna `Result<_, String>`: l'errore è il testo che `PtyError`
produce, e va mostrato a chi guarda invece di finire in un `console.error`.

### Perché `submit` e `press` sono due comandi e non uno

Il motore li distingue già, e la distinzione è il senso di tutto il crate:
`submit` guarda una riga intera **prima** di eseguirla e può mandarla a un
flusso invece che alla shell; `press` passa byte grezzi — un Ctrl-C, una
freccia, la risposta a una domanda interattiva — senza farli esaminare da un
elenco di regole che non li riguarda.

La finestra manda a `submit` solo ciò che l'utente conferma con Invio a inizio
riga; tutto il resto è `press`. Un emulatore che mandasse tutto a `submit`
farebbe passare ogni tasto per lo smistamento, e un editor dentro il terminale
diventerebbe inservibile.

## L'evento dell'uscita

Nome: **`terminal_output`**. Payload: `{ id: string, bytes: string, at: number }`,
dove `bytes` è base64 e `at` è l'offset del primo byte dall'apertura del
terminale — ciò che permette a un pannello di unire il backlog e l'uscita viva
senza un buco né una ripetizione.

**Base64 e non una stringa** perché ciò che esce da uno pseudo-terminale è una
sequenza di byte che può spezzarsi a metà di un carattere multibyte: consegnarla
come stringa la corromperebbe, e l'accento sparito si vedrebbe solo su una
parola italiana in mezzo a un'uscita lunga.

Un evento **`terminal_closed`** con `{ id, status }` dice che il processo dentro
è finito: senza, la finestra mostrerebbe come vivo un terminale morto — che è la
forma in cui il guasto 12 si ripresenta ogni volta.

## Chi possiede quali file

Perché due cantieri paralleli non si tocchino:

- **il ponte**: `desktop/src-tauri/src/terminal.rs` (nuovo), `crates/terminal/**`,
  `crates/supervisor/**`, e **una sola riga** in
  `desktop/src-tauri/src/main.rs` — le sei voci dentro `generate_handler!`;
- **la finestra**: `desktop/src/Terminals.tsx`, `desktop/src/TerminalPane.tsx`,
  `desktop/src/terminal.ts` e le loro prove (nuovi), `desktop/src/styles.css`, e
  **poche righe** in `desktop/src/App.tsx` per la voce di navigazione.

Chi ha bisogno di toccare un file dell'altro si ferma e lo dice a chi coordina.

## Cosa mancava nel crate — corretto dopo la misura

**Questo paragrafo diceva il falso, e va letto come storia.** Chiedeva al ponte
«il modo di leggere l'uscita», perché `Terminal` non espone `Pty::reader()`. Ma
la cosa per cui serviva **c'era già**: `Terminals::open` prendeva un
`Arc<dyn Output>` e apriva già il filo che drena, e la prova che il primo pezzo
arriva prima che il secondo esista era nel repo da prima di questo cantiere.

Quello che mancava davvero è **la fine**: nessuno diceva che il processo dentro
era finito, e senza quello l'evento `terminal_closed` che questo documento
pretende non sarebbe potuto esistere. È ciò che il ponte ha costruito —
`Ending` coi suoi tre casi, `Output::ended`, `Pty::finished()` che non blocca
mai.

La correzione sta qui e non in un commento del codice perché il documento lo
chiede a chiare lettere: *chi lo scopre diverso dal codice apre un guasto invece
di adeguare in silenzio la propria metà*. Vale anche per chi il documento
l'ha scritto.

## Le due proprietà che il cantiere deve avere, e come si vedono rosse

1. **Un terminale sopravvive alla finestra.** Chi chiude la finestra non uccide
   la sessione dentro: al riavvio, `terminal_list` la ritrova e la finestra si
   riaggancia.

   **Vera dal 02/09/2026, e per la strada che il paragrafo sotto indicava come
   necessaria: un processo residente che tiene i capi dei pty.** È `sailor
   terminal host` (`crates/terminal/src/host.rs`): il ponte non apre nessuno
   pseudo-terminale, è un cliente di quel processo su un socket accanto alle
   cassette delle lettere, e lo avvia se nessuno risponde. La prova è
   `crates/sailor/tests/a_terminal_outlives_the_window.rs`, col binario vero:
   il cliente che ha aperto la shell sparisce, un cliente nuovo la ritrova viva
   col suo backlog, e il controllo assurdo — spento l'ospite, la shell muore —
   dice che il pty è dell'ospite e di nessun altro. Ciò che segue è la storia
   di com'era misurato prima.

   **Non si fa passando da `supervisor::child::Process::start`, e questo
   documento diceva il contrario.** Misurato il 01/09, e confermato da un
   giudice che non aveva scritto il ponte: quella strada fa `Command::spawn()`
   con `Stdio::null()` — nessuno pseudo-terminale, nessun `setsid`, nessun
   `TIOCSCTTY` — e in tutto `crates/supervisor` non c'è una riga che apra un
   pty. Resta vero che è **l'unica strada che registra**, e che aggirarla è
   vietato (è il guasto 4). Quindi la proprietà non sta in questo cantiere: è
   un cantiere a sé, e comincia dall'unica strada lecita — **insegnare a
   `Process::start` ad avviare dentro uno pseudo-terminale**, con una voce in
   più nella sua `Spec`.

   Ci vuole anche un processo residente che tenga i capi dei pty: oggi vivono
   nel processo della finestra, e chiusa quella il `follower` va in EOF e la
   shell esce. Nessuna registrazione nel deposito cambia questo.
2. **Ciò che esce arriva mentre esce, non alla fine.** La prova che lo dice
   rossa: un comando che stampa, aspetta, stampa ancora — e l'asserzione che il
   primo pezzo è arrivato **prima** che il secondo esista.
