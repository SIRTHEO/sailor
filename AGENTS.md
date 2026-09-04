# Sailor — istruzioni per chi lavora in questo albero

## Cosa stiamo costruendo

Sailor lancia le righe di comando (Claude Code, Codex, Gemini), applica **un solo
corpo di regole a tutte**, e fa girare ogni lavorazione come **flusso
registrato** invece che come script, gancio o binario a sé.

Il metro di ogni lavoro: *questa cosa toglie a Theo un'approvazione, o gli porta
un dubbio migliore?* Se non fa né l'una né l'altra, non è lavoro.

**Prima di correggere qualunque cosa, leggi `docs/decisioni.md`** — i vincoli
permanenti e le scelte che non si riaprono — **e `docs/da-fare.md`**, che dice a
che punto siamo e cosa sta per sparire. Riparare un pezzo che deve sparire è
lavoro contro il piano, e nessun controllo locale lo mostra: resta tutto verde.

Queste due stanno nel repo di proposito. Fino al 30/08/2026 questa riga mandava
a `~/.claude/docs/sailor-adesso.md`, che la pulizia del 28/08 aveva cancellato:
per due giorni la prima istruzione di ogni sessione è stata un indirizzo vuoto,
e nessuno se n'è accorto perché un puntatore rotto in un documento non è rosso.

## L'ordine dei lavori, deciso da Theo

**Codice in Sailor → rimozione debito → costruzione del flusso.** Mai il
contrario. Non si costruisce dentro ciò che deve sparire.

## Le decisioni già prese, che non si riaprono senza una misura

- **Un flusso è un file di dati; i nodi sono azioni registrate in Rust.** Nessun
  interprete dentro Sailor. Il formato è `{ id, description, graph, inputs }`:
  `graph` è ciò che `flow::Graph` già carica e valida, `inputs` diventa i
  `root_inputs` della richiesta. Il primo esempio: `flows/prima-corsa.flow.json`.
- **Il nome dell'azione sta nel grafo, il codice no** (`graph.rs`, sopra il campo
  `action`). Un passo nuovo è un'azione Rust registrata in `crates/actions`, mai
  uno script.
- **Il freno sta al confine del processo**, non nei ganci nativi di una singola
  riga di comando. Le righe di comando restano vergini: Sailor legge la loro
  configurazione e la migra.

## Come si verifica — l'unico oracolo è `cargo`

**Mai dichiarare fatto senza evidenza misurata nello stesso turno.** E una misura
vale solo se poteva venire diversa: **rompi apposta ciò che provi** e guarda
l'esito cambiare. Se togliendo la riga che dichiari il controllo resta verde, il
controllo non controlla niente.

Trappole già pagate su questa macchina:

- **Mai incanalare `cargo test` in `grep` o `tail`**: il codice d'uscita diventa
  quello dell'ultimo comando, e una batteria rossa passa per verde. Scrivi
  l'uscita su un file e leggila.
- **Una seconda cartella di compilazione sta DENTRO `target/`, mai accanto.**
  Il `.gitignore` chiede `target-<qualcosa>` e ha risolto il problema di git; il
  problema del disco l'ha creato. `cargo clean` svuota **solo** `target/`, quindi
  ogni `target-int`, `target-i18n`, e peggio ogni `sailor-target-*` fratello del
  repo, resta lì per sempre e nessuno lo vede crescere. Il 04/09/2026 erano
  **27 GB su quattro cartelle**, di cui 3,9 GB in un `target-i18n` che nessun
  file dell'albero nominava. Chiamala `target/int`, `target/verifica`, come fa
  già `release_cmd.rs:171` con `target/from-head`: una riga di `.gitignore` la
  copre e `cargo clean` la riprende.
- **Sempre `--no-fail-fast`, e non è un dettaglio di comodità.** Senza,
  `cargo test` si ferma al **primo binario rosso** e tutto ciò che viene dopo
  **non viene eseguito** — non fallisce: non parte. Misurato il 01/09/2026
  dentro il perimetro, dove il sandbox nega `openpty` e `crates/terminal`
  cade sempre. I binari che non partono sono sempre gli stessi, la coda
  dell'alfabeto — `toolbox`, `trigger`, `ui` e sette prove d'integrazione. Chi
  batte `cargo test` lì dentro sta guardando tre quarti dell'albero credendo di
  guardarlo tutto.

  **Le cifre di questo paragrafo erano «36 su 47» ed erano superate.** Rimisurato
  il 04/09/2026 con `--no-fail-fast` e l'uscita su file: **108 binari più 19
  doc-target, 1.252 prove, 1.199 verdi, 53 rosse — e tutte e 53 sono il sandbox**
  (43 per `mkdir /tmp/sr-*` negato, 11 per `openpty` negato). **Zero rosse vere.**
  In CI il 02/09: 109 binari, 1.134 prove, 2 rosse vere. Tutti e 19 i crate hanno
  prove. Un numero scritto qui e non rimisurato è una guardia falsa come le altre:
  chi legge «36 su 47» oggi conclude che manca un quarto dell'albero, e non manca.

  Quel giorno è costato un lavoro dichiarato finito con una regressione dentro:
  la prova che cadeva stava in `toolbox`, e il `grep FAILED` di chi la cercava
  non poteva trovarla perché quella prova non era **mai partita**. È la stessa
  famiglia della riga qui sopra — un esito verde che non ha guardato niente —
  e si riconosce solo contando i binari, non le prove.
- **`cargo fmt -- <file>` non si limita a quel file**: formatta tutto il
  workspace. L'albero non è formattato in blocco e non va formattato in blocco.
- **`cargo test --tests` non aggiorna il binario** che i ganci eseguono.
- **La compilazione può essere negata** quando lo swap è alto: usa `-j 1`, e se
  nega ancora **non dichiarare provato ciò che non hai compilato**.

## Come si scrive

- **Everything is in English.** Identifiers, comments, documentation, commit
  messages, and every message a user of the tool can see. There is no inside
  language and no outside language: this repository is public, and what is
  committed here is world-readable, permanently.

  This is the rule the project was founded with, and it was lost. It lived in a
  `CLAUDE.md` on an orphan branch with an unrelated history — one commit, never
  published, unreachable from anything. The project then spent days
  rediscovering it piece by piece. The branch is kept as the tag
  `archivio-primo-abbozzo`; the rest of what it said is in `docs/decisioni.md`.
  It is the most expensive shape of the defect this project keeps chasing: not
  a rule nobody interrogates, but **a rule nobody could read**.

  Identifiers include function names, types, fields, options, **local
  variables**, **modules**, **constants**, **file and directory names**, **CSS
  classes** and **JSON keys**. In Rust a file *is* a module: its name is an
  identifier like any other. That list used to be shorter, and the difference
  cost 136 renames — an incomplete rule is not a partial rule, it is a
  permission. The measure is `cargo test -p sailor --test
  identifiers_are_in_english`, which reads workflow job keys too.

  **Fixture data is data, not language.** `f.name == "assente"` stays as it is;
  the variable holding it is called `absent`.

- **Few comments, and no chronicle.** A comment says *why*, not *what*: the
  what is said by the code. If what you want to write can be had by renaming a
  variable, extracting a function or writing a test, do that instead. A comment
  earns its place when a *why* is left that the code cannot carry: an external
  constraint, a counter-intuitive choice, a declared limit.

  **Dates, "it used to do X", the story of how it went: not here.** They belong
  in the fault ledger and in the commit message, which keep them with the real
  author and the real date instead of a hand copy. In the code, at most a
  pointer: `// see fault 39`.

  **Cap: six lines per block.** Measured 2026-09-01: 3,036 blocks, median 3
  lines — the ordinary comment is already inside the cap and nothing changes
  for it. What overflows is 636 blocks carrying two thirds of the 14,343
  comment lines, the longest being 66 consecutive lines. The cap hits the tail,
  not the habit. Shortening is not deleting: the first block trimmed was that
  66-line one in `flow/src/subflow.rs`, five decisions that were already in
  `docs/decisioni.md` plus one limit that now sits next to the function causing
  it.

  **And the reason is not taste.** The semantic index does not strip comments:
  SocratiCode's `chunkFileContent` embeds the text as written, and calls
  "preamble" everything preceding a declaration. A 66-line block **becomes the
  chunk the index compares against your question**, instead of the code below
  it. A project that is 25% narrative comment hands you the story when you ask
  it for the code.

  The measure is `cargo test -p sailor --test comments_do_not_crowd_out_the_code`,
  and its numbers can only go down.

- **Notes that cannot meet these rules stay out of the repository**, under
  `~/personal/.sailor-notes/`, which has no git remote and nothing in it is
  ever copied in. That covers absolute paths from a developer machine, client
  or employer names, internal repository names, transcripts and logs copied out
  of private tooling, and any framing of this work as a reaction to or
  comparison with somebody else's product.
- **Flow and step `id`s stay as they are**, and the `.flow.json` filenames with
  them. This is not an exception to the language rule: **what the compiler
  reads is language, what the ledger keeps is data.** Renaming a step would
  make already-recorded runs show up as unknown steps, and renaming a shipped
  flow would silently stop a user's own replacement from winning. Decided by
  Theo on 2026-08-31, in full in `docs/decisioni.md`.

- **Commit messages: Conventional Commits.** `<type>(<scope>): <subject>`,
  lowercase, imperative, no trailing period. The body explains why, not what —
  it is where the chronicle the code must not carry actually belongs. No
  tooling attribution trailers.
- **Scritture solo con gli strumenti di modifica file**, mai con `sed`, heredoc o
  uno script interprete: le scritture da interprete saltano i controlli di casa.
- **Percorsi assoluti**, mai `cd X && comando`.
- Un commento che afferma qualcosa di falso si corregge subito. Il codice è la
  fonte; commenti e documenti sono indizi datati.

## Le copie di lavoro: chi ne apre una, la chiude

Il 02/09 se ne sono trovate **53 abbandonate, 39 GB**, nate in 28 ore a raffiche
di sei-otto all'ora. Tutte pulite, nessun processo dentro, e **50 su 53 già
dentro il tronco byte per byte**: non era lavoro perso, era ingombro.

Nessuno le toglieva perché si credeva che rimuoverle cancellasse il ramo. **Non
è vero, ed è misurato**: `git worktree remove <cartella>` toglie la cartella e
lascia il riferimento dov'è. Il comando di Orca è un'altra cosa. Quindi chiudere
una copia non costa niente e non si perde niente.

- **Quando hai finito, togli la tua copia.** `git worktree remove` sulla
  cartella, e il ramo resta consultabile.
- **Non togliere quella di un altro** senza misurare prima: `git status
  --porcelain` dentro, e nessun processo con la `cwd` lì. Se una delle due parla,
  chiedi.
- **Cancellare il ramo è un'altra decisione**, e non è tua: si prova prima che il
  contenuto sia già nel tronco, e si chiede.

## L'integrazione ha un ramo solo

Undici di quelle 53 esistevano **solo per fondere** — `fusione-quattro`,
`fusione-sei`, `fusione-46`, `fusione-sera`, e così via — e il tronco porta **47
fusioni per una quarantina di rami di lavoro**. Ogni sessione che finiva apriva
la propria copia per integrare e rifaceva da capo gli stessi conflitti. È lì che
se ne sono andati i token: non a scrivere due volte lo stesso codice, ma a
fonderlo dodici volte in dodici posti.

Non aprire un ramo per fondere. Fondi il tuo lavoro dove si integra già, e se non
sai dove sia, **chiedi ai vicini prima di aprirne uno**.

## Come si prova che un ramo è superato

L'antenato non basta e a volte mente: dopo una riscrittura della storia o uno
squash, `merge-base --is-ancestor` dice «no» su lavoro che c'è già tutto. **Si
confronta il contenuto**: fondi il ramo in una copia usa-e-getta del tronco e
guarda se l'albero cambia.

E prima di credere al risultato, **il controllo assurdo**: fai passare dalla
stessa misura un ramo che *deve* risultare portante — uno con dentro un file che
il tronco non ha. Se esce «superato», la misura è cieca e ogni numero di quella
passata si butta.

## Chi crea non giudica

Il verdetto su un lavoro va a un contesto che non l'ha prodotto. Se hai scritto
tu la correzione, non sei tu a dichiararla buona: riferisci cosa hai scritto,
come l'hai provato, e cosa resta incerto.

## Come si riferisce

Frasi corte, un'idea per frase, verbo attivo. **Il risultato prima**, il
dettaglio dopo solo se cambia una decisione. Prima la cosa, poi il meccanismo: un
nome di file è un indirizzo, non una spiegazione.

Chiudi con un verdetto esplicito: cosa è chiuso **con l'evidenza misurata**, e
cosa resta aperto.
