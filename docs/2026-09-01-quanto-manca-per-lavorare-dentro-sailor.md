# Quanto manca per lavorare dentro Sailor

> **Superato il 02/09/2026 da `docs/2026-09-02-the-mvp-that-ends-orca.md`.**
> Quel documento lo dichiara scaduto sul proprio primo punto — il terminale
> nella finestra, chiamato qui «il vero blocco», che il 02/09 esisteva già — ma
> la dichiarazione stava solo là. Per due giorni chi apriva **questo** file
> leggeva una mappa che diceva «qui c'è un muro» dove il muro non c'era più, e
> nessuna riga glielo diceva. Aggiunta il 04/09/2026.
>
> Resta come misura di come si stimava il 01/09. **Per sapere cosa manca, la
> fonte sono le ventuno affermazioni dell'altro documento**, non questa lista.

**01/09/2026.** Nato dalla domanda di Theo: *quanto lavoro manca all'MVP per
poter passare una giornata dentro Sailor?*

Il metro non è «Sailor fa cose»: ne fa già molte. Il metro è **una giornata di
lavoro di Theo che non deve uscire da Sailor**. Questo documento elenca cosa
quella giornata richiede, cosa c'è già misurato, e cosa manca.

I numeri qui sono letti dal codice e da `git` il 01/09/2026. Dove non ho
eseguito una prova, lo dico.

## Cosa Sailor sa già fare, e conta

- **Il motore dei flussi**: 16 azioni registrate, grafo dichiarato, fronte
  parallelo con tetto di quattro passi, `subflow` (un flusso che ne esegue un
  altro), ripresa di una corsa, tetto di spesa per flusso.
- **Il conto della benzina**: token e prezzo per chiamata, listino incorporato
  nel binario, quota residua letta dal motore invece che chiesta, e la riga che
  dichiara *cosa non ha potuto misurare*.
- **I profili**: case separate per motore (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`),
  applicate a ogni motore invocato da un passo — non più solo da `sailor run` —
  e da stamattina `flow check` e `profiles list` dicono se una casa è
  autenticata.
- **La finestra** (Tauri + React): si apre su *cosa sta succedendo adesso*
  (corse vive, chi aspetta te, spesa di oggi), poi tela dei flussi, editor di un
  passo, console di una corsa, storia, inventario della macchina, manuale dei
  comandi letto dal binario.
- **Il registro dei processi**: ogni processo acceso da Sailor è nel deposito,
  con pid e porta; `sailor-live --list` / `--stop` li vedono e li spengono da
  un'invocazione separata (guasto 4, chiuso il 31/08).
- **Il coordinamento fra agenti**: presenza, prenotazione del lavoro
  (`work_claim` / `work_release`), `work_survey` — «chi sta lavorando su cosa» —
  interrogabile *da un flusso*, non solo dalla finestra.

## Cosa manca, in ordine di quanto blocca il trasloco

### 1. Il terminale dentro la finestra — **il vero blocco**

Il motore esiste ed è finito: `crates/terminal`, **2.295 righe**, apre uno
pseudo-terminale vero, scrive, consegna l'uscita mentre esce, ridimensiona,
chiude, e dice quali terminali sono aperti e in quale spazio. Lo smistamento di
ciò che l'utente scrive è già dati, non un `match`.

**Nessun binario lo spedisce e la finestra non lo conosce.** L'elenco dei
comandi esposti da Tauri (`desktop/src-tauri/src/main.rs`) ha 17 voci: flussi,
corse, storia, strumenti, manuale. Nessuna riguarda un terminale.

Manca: i comandi Tauri (apri, scrivi, leggi in streaming, ridimensiona, chiudi),
un emulatore nel frontend, i pannelli e le schede, e il riaggancio a un
terminale già vivo quando la finestra si riapre.

Senza questo, dentro Sailor non si può tenere una sessione di Claude o Codex
aperta — che è il 90% di quello che Theo fa in una giornata.

### 2. Gli spazi di lavoro: repo, rami, worktree

Servono `repo add`, `worktree create/list/rm` e un `worktree ps`. Sailor ha
`sailor workspace init`, che scrive un marcatore `sailor.json` e nient'altro; e
`terminal::Workspace`, che verifica che una cartella esista.

Manca: creare un worktree da un ramo, elencarli, smontarli, e aprirci sopra un
terminale. È il gesto con cui comincia ogni lavorazione di Theo.

### 3. Il motore giusto nel terminale giusto, autenticato

Il pezzo c'è: `sailor run <cli>` sostituisce il processo lanciando la riga di
comando con la casa del profilo attivo, e rifiuta se il collegamento delle
credenziali punta a un altro profilo.

Manca: il ponte dalla finestra («terminale nuovo → quale motore, quale
profilo») e un accesso guidato. **Misurato stamattina: tutti e due i profili
`codex` sono senza credenziali**, e l'attivo è `prove`. Ogni chiamata a `codex`
da un flusso parte non autenticata.

### 4. Vedere il lavoro: diff e file

Serve aprire un file, un diff, tutti i file cambiati. Sailor non ha nessuna
vista sul contenuto di un repository.

Per un MVP basta meno di un editor: **il diff in sola lettura** e «apri
nell'editor che usi già». Ma zero non basta: il giudizio su un lavoro di un
agente si dà guardando cosa ha cambiato.

### 5. La staffetta, dentro Sailor

L'azione c'è — `handed_to_agent`, il passo che non avvia niente e consegna il
lavoro all'agente già vivo nel terminale — e dipende dal punto 1 per avere un
terminale a cui consegnare. Il ciclo mandato → azzeramento → ripresa che Theo
usa oggi non ha ancora una forma in Sailor.

### 6. L'orologio

`sailor flow due` calcola quali flussi sono dovuti. **Nessuno esegue ciò che
calcola.** Finché manca, un flusso a ronda non è ripetibile da dentro Sailor —
e il rimedio ovvio, uno script che rilancia, è già stato scritto e cancellato lo
stesso giorno perché sarebbe un cerotto fuori dal sistema.

La decisione è già scritta e non è «fai un cron»: l'ordine di preferenza è
vincolo → evento → scadenza alla lettura → cron come rete di sicurezza
(`docs/2026-09-01-il-tempo-e-l-ultima-scelta.md`).

### 7. Il rilascio, che è indietro

Il binario in servizio (`~/.local/bin/sailor`) offre ancora `ui`, rimosso dai
sorgenti, e **non ha `remaining`**. Chi lavora con Sailor oggi usa un Sailor
vecchio. Misurato eseguendo `sailor` senza argomenti.

## Cosa resta fuori dall'MVP, e va detto

- **Il browser dentro la finestra.** È l'oracolo del lavoro sull'interfaccia
  (direzione di prodotto 4), non una comodità — ma non blocca il trasloco.
- **La ricerca su tutto ciò che il sistema conserva.**
- **La condivisione dei flussi**, che senza il rifiuto di pubblicare un segreto
  è una fuga di dati con l'aspetto di una funzione.
- **Il modello Bazel dei poteri di un passo**, deciso il 29/08 e non iniziato:
  costa la riscrittura dei flussi esistenti.
- **I 13 guasti aperti su 40** (numero dato da
  `the_fault_table_holds_together`, non ricopiato a mano). Nessuno dei nove
  ancora *interamente* aperti sta sulla strada del trasloco: parlano di
  descrittori, di prove che leggono la macchina, di rinvii non risolti.

## La stima, con il suo margine

Misurata sul ritmo vero di questo repo — il 31/08 sono stati aperti, chiusi e
fusi sette rami di lavoro in una giornata:

| cantiere | dimensione | dipende da |
|---|---|---|
| 1. terminale nella finestra | **2 giornate** — è tutto frontend, il motore è fatto | — |
| 2. spazi di lavoro e worktree | 1 giornata | — |
| 3. motore e profilo nel terminale | mezza giornata | 1 |
| 4. diff in sola lettura | mezza giornata | 2 |
| 5. staffetta | 1 giornata | 1 |
| 6. orologio | mezza giornata | — |
| 7. rilascio in pari | poche ore | — |

**Somma: cinque giornate e mezza di lavoro netto.** Con le fusioni e la verifica
— e le fusioni qui costano: il guasto 31 è nato da nove agenti che si sono
sovrapposti su firme che git non vedeva in conflitto — **una stima onesta è due
settimane** per una giornata di Theo che non deve uscire da Sailor.

Il cantiere 1 vale da solo più della metà del risultato: senza terminale nella
finestra gli altri sei non si possono usare, con il terminale Sailor diventa
subito il posto dove si lancia un agente, anche se tutto il resto resta a metà.

## Il difetto che questo documento può avere

È scritto leggendo il codice, non usandolo. Non ho eseguito la batteria di prove
né aperto la finestra: se qualcosa qui dentro è già più avanti di come lo
racconto, lo dirà chi la apre. Il verdetto su questa analisi non è mio — chi la
usa la controlli sul punto 1, che è quello su cui si decide.
