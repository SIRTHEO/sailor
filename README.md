# Sailor

Sailor fa lavorare insieme gli agenti da riga di comando che hai già installati —
Claude Code, Codex, Gemini CLI e chiunque altro — dentro **flussi** che sai
leggere, misurare e fermare.

Non è un altro agente. È l'impalcatura attorno a quelli che usi: decide quale
motore chiamare, con quale identità, quanto può spendere, e **scrive nel
dettaglio che cosa è successo** — così una corsa si può rivedere, confrontare e
ripetere invece di raccontare.

> **Stato: in costruzione, e usato ogni giorno da chi lo scrive.** Le interfacce
> cambiano. Quello che c'è funziona ed è provato; quello che manca è scritto in
> `docs/guasti-incontrati.md`, con dentro anche i difetti ancora aperti.

## Cosa fa, in concreto

- **Esegue flussi**: un grafo di passi, con quelli indipendenti che partono
  davvero insieme. Un passo può chiamare un motore, eseguire una verifica,
  leggere e scrivere in un deposito, o **consegnare il lavoro a una persona**.
- **Non conosce nessun motore per nome.** Ogni strumento si presenta con un
  *descrittore* che dichiara come gli si parla: come si fa una domanda secca,
  con quali parole rifiuta una riga malformata, come dichiara di essere
  esaurito, come gli si chiede se è autenticato. Chi non dichiara non fa
  scattare niente — **tacere è diverso da indovinare.**
- **Prova le righe di comando prima di spendere**: `sailor flow check` monta la
  riga vera di ogni motore e la esegue *senza la domanda*, così una riga
  malformata si scopre lì e non alla prima corsa a pagamento.
- **Misura quanto costa**: token per classe, costo per chiamata, tetto di spesa
  che si stringe man mano che il residuo cala. E quando un pezzo del conto non è
  misurato **lo dice al posto del numero**, invece di dare una cifra secca che
  chi legge prenderebbe per il totale.
- **Sa con quale identità è partito ogni motore**: profili di credenziali
  separati per riga di comando, e ogni chiamata registra quale casa ha usato e
  **come è stata scelta** — mai un campo vuoto.
- **Non lascia processi orfani**: quello che accende lo registra, e lo sa
  spegnere anche da un'altra invocazione.

## Come si costruisce e si prova

```sh
cargo build --workspace
cargo test --workspace      # ~880 prove su 73 bersagli
```

Serve Rust 1.89 o più recente. Nessun servizio, nessun database da avviare:
il deposito è un file SQLite creato al primo uso.

La finestra (Tauri + React) sta in `desktop/` ed è **fuori dal workspace**:

```sh
cd desktop && npm install && npm run live
```

`npm run live` avvia `sailor-live`, non `cargo tauri dev`. La differenza non è di
gusto: `cargo tauri dev` chiude la finestra a **ogni** file toccato, *prima* di
compilare, quindi un errore di compilazione la fa sparire e non torna. Con
`sailor-live` si costruisce prima e si tocca ciò che è acceso **solo se** la
costruzione è riuscita: la finestra sopravvive, cambia titolo e mostra l'errore.
Il perché per esteso è il guasto 11 in `docs/guasti-incontrati.md`.

## I comandi

| comando | a cosa serve |
|---|---|
| `sailor flow list \| check \| run \| cost \| cap` | i flussi: quali ci sono, se reggono, eseguirli, quanto sono costati, che tetto hanno |
| `sailor step open \| close` | i passi che un agente vivo prende in carico |
| `sailor run <cli>` | lancia una riga di comando con la dotazione del suo profilo |
| `sailor profiles list \| create \| switch` | le identità di ogni motore, e se sono autenticate |
| `sailor remaining` | quanta quota resta, letta dal fornitore invece che dedotta |
| `sailor inventory` | cosa sa fare questa macchina, e cosa è comparso o sparito |
| `sailor session` | i terminali tracciati e cosa c'è acceso adesso |
| `sailor workspace` | la radice del progetto e cosa dichiara |

`sailor --help` li elenca tutti, e ogni comando spiega le proprie forme.

## Come è fatto

Quindici crate Rust in un workspace, con i confini messi dove passano le
responsabilità e non dove è comodo:

| | |
|---|---|
| `flow` | il motore: grafo, fronti paralleli, tetto di spesa, ripresa |
| `actions` | cosa può fare un passo, e come si invoca un motore esterno |
| `toolbox` | i descrittori: come si parla a uno strumento che non conosciamo |
| `ledger` | il deposito: eventi, e le proiezioni che se ne ricavano |
| `models` | catalogo, listino dei prezzi, quota residua |
| `profiles` | le case di credenziali, una per riga di comando |
| `registry` · `trigger` · `release` | il registro delle azioni, gli inneschi, i rilasci |
| `inventory` · `sessions` · `supervisor` · `terminal` | la macchina: cosa c'è, cosa gira, cosa è acceso |
| `ui` · `sailor` | la vista condivisa e la riga di comando |

## Le regole che questo progetto si dà

Sono poche e valgono più delle preferenze di stile. Stanno per esteso in
[`AGENTS.md`](AGENTS.md) e [`docs/decisioni.md`](docs/decisioni.md).

- **Una prova conta solo se può venire diversa.** Si scrive prima della
  riparazione, si guarda nascere rossa, e si verifica rimettendo il difetto
  **originale**. Una prova mai vista fallire non è una prova.
- **Quello che non è una misura non diventa uno zero.** Un costo sconosciuto è
  sconosciuto, non zero; un elenco vuoto perché non si è potuto guardare non è
  «non c'è niente». L'errore, se capita, deve andare nella direzione che
  preoccupa — mai in quella che tranquillizza.
- **Chi crea non giudica.** Il verdetto su un lavoro non lo dà chi l'ha scritto.
- **Una regola in un commento non è una difesa: è la forma di una difesa.**
  Vale solo dove c'è un elenco che la applica o un controllo che la interroga.
- **I difetti si scrivono tutti**, in `docs/guasti-incontrati.md`, con come si
  sono visti e **cosa li impedirebbe** — perché il seguito di un guasto è un
  controllo, non un compito assegnato a qualcuno.
- **Identificatori in inglese, commenti e messaggi in italiano.** Ovunque,
  prove comprese.

## Licenza

[GNU AGPL v3](LICENSE).

Puoi usarlo, modificarlo e anche venderlo. Se lo modifichi — o se lo offri come
servizio in rete — **devi pubblicare le tue modifiche** con la stessa licenza.
Nessuno può prendere questo lavoro e chiuderlo.
