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
- **`cargo fmt -- <file>` non si limita a quel file**: formatta tutto il
  workspace. L'albero non è formattato in blocco e non va formattato in blocco.
- **`cargo test --tests` non aggiorna il binario** che i ganci eseguono.
- **La compilazione può essere negata** quando lo swap è alto: usa `-j 1`, e se
  nega ancora **non dichiarare provato ciò che non hai compilato**.

## Come si scrive

- **Identificatori in inglese** — nomi di funzione, tipi, campi, opzioni.
- **Commenti e messaggi in italiano.** Un commento dice *perché*, non *cosa*: il
  cosa lo dice il codice.
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
