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
- **Sempre `--no-fail-fast`, e non è un dettaglio di comodità.** Senza,
  `cargo test` si ferma al **primo binario rosso** e tutto ciò che viene dopo
  **non viene eseguito** — non fallisce: non parte. Misurato il 01/09/2026
  dentro il perimetro, dove il sandbox nega `openpty` e `crates/terminal`
  cade sempre: **36 binari eseguiti su 47**. I dieci che non partono mai sono
  sempre gli stessi, la coda dell'alfabeto — `toolbox`, `trigger`, `ui` e sette
  prove d'integrazione. Chi batte `cargo test` lì dentro sta guardando tre
  quarti dell'albero credendo di guardarlo tutto.

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

- **Identificatori in inglese.** Nomi di funzione, tipi, campi, opzioni,
  **variabili locali**, **moduli**, **costanti**, **nomi di file e cartelle**,
  **classi CSS**, **chiavi JSON**. In Rust un file *è* un modulo: il suo nome è
  un identificatore come gli altri.

  **Questo elenco era più corto, e la differenza è costata 136 rinomine.** Fino
  al 31/08/2026 diceva «nomi di funzione, tipi, campi, opzioni»: chi scriveva un
  `let listino` o un file `smista_il_lavoro.rs` non trovava il proprio caso
  nell'elenco, e la direttiva di sessione — che dice «italiano» senza dire
  «tranne gli identificatori» — vinceva. Una regola incompleta non è una regola
  parziale: è un permesso.

  **La misura è `cargo test -p sailor --test identifiers_are_in_english`**, e
  vale più di questa riga: una regola che nessuno interroga non diventa rossa
  mai. Chi incontra una parola italiana che il controllo non conosce la aggiunge
  al suo elenco.
- **Commenti e messaggi in italiano.** Un commento dice *perché*, non *cosa*: il
  cosa lo dice il codice. Vale anche per i **dati di prova**: `f.name ==
  "assente"` resta così com'è — è un dato, non un identificatore — mentre la
  variabile che lo tiene si chiama `absent`.
- **Gli `id` dei flussi e dei passi restano in italiano**, e i nomi dei file
  `.flow.json` con loro. Il confine è questo: **ciò che il compilatore legge sta
  in inglese, ciò che il deposito conserva è un dato.** Rinominare un passo
  farebbe apparire le corse già registrate come passi sconosciuti. Deciso da
  Theo il 31/08/2026, per esteso in `docs/decisioni.md`.
- **Scritture solo con gli strumenti di modifica file**, mai con `sed`, heredoc o
  uno script interprete: le scritture da interprete saltano i controlli di casa.
- **Percorsi assoluti**, mai `cd X && comando`.
- Un commento che afferma qualcosa di falso si corregge subito. Il codice è la
  fonte; commenti e documenti sono indizi datati.

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
