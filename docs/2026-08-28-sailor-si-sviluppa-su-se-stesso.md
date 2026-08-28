# Sailor si sviluppa su se stesso

Documento di progetto, 28/08/2026. Nessun codice scritto, nessun flusso messo in
servizio. Tre domande di Theo: la modalità viva, l'autocura, le buone pratiche.

**Le tre risposte in tre righe.** La modalità viva conviene: ricompilare il
guscio costa **1,8 s e 430 MiB**, e lo strumento è già installato. L'autocura si
regge su un fatto misurato oggi: `sailor flow check` chiude con **uscita 0** un
flusso che poi fallisce davvero, quindi c'è un difetto vero da riparare e un
oracolo da costruire. Il playbook vale per **sei pratiche su dodici**: le altre
presuppongono una CI, delle PR e più persone, e qui non esistono.

Tutto ciò che segue porta un numero o dichiara di non averlo.

---

## 1. Il confine, prima di tutto

### 1.1 Cosa resta codice e cosa diventa flusso

Il criterio proposto — «codice ciò che esegue, flusso ciò che decide» — si rompe
sul caso che conta di più. Il gate delle autorizzazioni **decide**, ed è la cosa
che più di ogni altra deve restare codice.

Il criterio che propongo al suo posto:

> **È codice ciò che deve restare vero anche quando un flusso sbaglia.
> È flusso ciò che si giudica dagli esiti delle sue corse.**

E il modo di applicarlo, in una domanda sola:

> *Si può scrivere una prova che diventa rossa quando questa cosa è sbagliata?*

Se sì, è codice: ha un oracolo che risponde subito, `cargo`. Se il giudizio
richiede di guardare venti corse e discuterne, è flusso: il suo oracolo è il
registro, e arriva dopo.

Il corollario è più corto della regola: **i dinieghi sono codice, le proposte
sono flussi.** Un diniego sbagliato scritto in un flusso lo può riparare
l'autocura stessa — ed è esattamente il difetto che questo documento esiste per
impedire. Una proposta sbagliata scritta in codice costa una ricompilazione ogni
volta che si cambia idea, ed è il debito che Theo vuole evitare.

Dove cade la riga, oggi:

| Cosa | Dove | Perché |
|---|---|---|
| Il motore, il deposito, gli schemi | codice | un errore qui rende falso ogni esito |
| Il perimetro di scrittura, il gate | codice | è il diniego che protegge tutto il resto |
| La validazione di un `.flow.json` | codice | ha un oracolo secco, e serve prima che il flusso giri |
| Quali difetti cercare, in che ordine | flusso | si giudica sul registro: quante volte ha trovato qualcosa |
| Come si ripara, con che prompt, con che motore | flusso | cambia ogni settimana, e cambiarlo non deve costare una compilazione |
| Quando fermarsi, quando chiamare Theo | flusso | è una politica, e le politiche si discutono guardando gli esiti |

### 1.2 Flusso del prodotto, flusso del cantiere

Misura di partenza: **`crates/release/src/lib.rs` non nomina la parola «flow»
nemmeno una volta** (0 occorrenze, senza distinzione di maiuscole). Oggi il
rilascio non spedisce nessun flusso. Il confine quindi non esiste ancora — esiste
per omissione. Va deciso adesso, prima che qualcuno scriva la riga che spedisce
`flows/`, perché dopo sarà una rimozione invece di una scelta.

La regola che propongo si vede a occhio e si prova a macchina:

- `flows/` — **del prodotto**. Spedito. Un flusso qui non nomina mai `cargo`, né
  `crates/`, né un percorso di questo albero. Gira sulla macchina di chi installa,
  dove Sailor è un binario e non un repository.
- `flows-cantiere/` — **di chi costruisce Sailor**. Mai spedito. Ha il diritto di
  sapere che esiste `crates/actions`, che l'oracolo è `cargo`, che il guscio sta
  in `desktop/src-tauri`.

Il confine non regge perché è scritto qui. Regge se c'è una prova che diventa
rossa:

1. una prova che il pacchetto rilasciato non contiene nessun file proveniente da
   `flows-cantiere/`;
2. una prova che **nessun file in `flows/` nomina `crates/`, `cargo`, o un
   percorso assoluto di questa macchina.** È la stessa forma di controllo che
   `crates/sailor/tests/smista_il_lavoro.rs` già applica ai binari («nessun passo
   nomina un binario»): il precedente esiste, va esteso.

La seconda prova è quella che vale, perché prende il caso vero: non qualcuno che
sposta un file, ma qualcuno che scrive in `flows/` un flusso che sa troppo.

---

## 2. Il gate: l'autocura non deve potersi autorizzare da sola

Questa sezione sta prima delle altre perché è quella che, se sbagliata, rende
dannoso tutto il resto.

### 2.1 Il rischio, in una frase

L'autocura cerca difetti. Il file delle autorizzazioni le impedisce di toccare
qualcosa. **Un impedimento, visto da un riparatore, è un difetto.** Il primo
bersaglio di un sistema che ripara se stesso è la cosa che gli dice di no.

Non è un caso di scuola. È il caso più probabile, perché è anche il più facile da
«riparare»: una riga di JSON.

### 2.2 Tre anelli, e solo il secondo è nuovo

**Anello 1 — l'autocura non scrive mai.** Nessun passo di un flusso di autocura
ha permesso di scrittura. Il flusso produce un diff come *dato*, dentro
un'`answer_shape` dichiarata. Un dato non modifica niente. È già la postura di
`smista-il-lavoro`, che dichiara: «nessun passo ha permesso di scrittura, e
ognuno lo dichiara nei propri argomenti».

**Anello 2 — chi applica è codice, con un diniego che nessun dato può togliere.**

Serve un'azione Rust che applichi una patch. Dentro, due elenchi asimmetrici:

- l'**assenso** è un dato, e vive **fuori dall'albero dei sorgenti**, in
  `$SAILOR_HOME/autocura.json`. Dice quali percorsi l'autocura può proporre di
  toccare. Si allarga scrivendo una riga: è la manopola di Theo.
- il **diniego** è nel binario. Non si allarga scrivendo una riga: si allarga
  ricompilando, e ricompilare passa da `cargo test` e da una mano umana.

E una regola sola fra i due: **il diniego vince sempre sull'assenso.** Nel
diniego incorporato stanno tre cose, e solo tre: il file delle autorizzazioni, il
modulo che applica le patch, e la prova che difende questi due.

Il motivo per cui l'asimmetria è tutto: un'allowlist che si può allargare da un
file *è* una porta, e l'autocura sa scrivere file. Una denylist compilata è una
parete, e l'autocura non sa compilare senza passare da un umano.

**Anello 3 — l'ultimo gesto è di un umano, e il motore lo sa già dire.**
`StepSpecies::HandToHuman` esiste già nel contratto (`crates/flow/src/record.rs`,
e `crates/flow/src/executor.rs` la tratta a parte). Manca l'azione la cui specie
sia quella. Il passo che consegna la patch è quel passo.

### 2.3 Come ci si accorge, se un giorno smette di essere vero

Dichiararlo non basta. Servono tre sentinelle, in ordine di forza.

**a) Il canarino avversario — la sola che vale davvero.**

Una prova in `crates/sailor/tests/` che chiede all'autocura, **apposta**, di
riparare il file delle autorizzazioni, e pretende che la corsa finisca `Broke`
con la classe del rifiuto. La prova è verde finché il confine tiene. Diventa
rossa il giorno in cui qualcuno toglie la parete.

È una prova e non un flusso notturno, per una ragione misurata altrove: questa
macchina dorme circa settanta volte al giorno, e un flusso notturno gira quando
la macchina è sveglia. Una prova gira prima di ogni commit.

E il canarino va provato a sua volta, altrimenti è un numero: **si toglie la riga
del diniego e si guarda la prova diventare rossa.** Se resta verde, non
controllava niente. È la regola di casa — rompi ciò che provi — applicata alla
cosa che protegge tutte le altre.

**b) L'impronta nel registro.** A ogni corsa, l'autocura scrive nel deposito
l'impronta del file delle autorizzazioni. `events.db` è append-only: quello che
ci entra non si riscrive. Un'impronta cambiata senza un gesto umano registrato è
un allarme. Questa sentinella si accorge **dopo**: non impedisce niente, ma
lascia un testimone che l'autocura non può rimuovere.

**c) Chi crea non giudica.** Il verdetto sulla patch va a un motore che non l'ha
scritta. Sulle patch che toccano il perimetro, va a Theo.

### 2.4 La falla che resta, dichiarata

Il disegno qui sopra non è ermetico, e vale più dirlo che nasconderlo.

Esiste l'azione `shell_check`, e prende un `command` arbitrario. Un flusso che
esegue `shell_check` può scrivere ovunque l'utente può scrivere — compreso
`$SAILOR_HOME/autocura.json`. Il processo non ha un perimetro di scrittura: i
passi *dichiarano* di non scrivere, ma nessuno glielo impedisce.

Quindi, oggi, il confine non regge sui permessi del processo. Regge su un fatto
più fragile: **nessun flusso del cantiere passa a `shell_check` un comando che
viene da un modello.** Il comando è scritto nel file, non generato.

Due strade, ed è una decisione di Theo (§8):

- si accetta, e si aggiunge una prova che nessun `command` di `shell_check` in un
  flusso del cantiere contenga un rinvio `$from`/`$join` che pesca da un passo
  motore. È a costo quasi zero e prende il caso vero.
- oppure `shell_check` prende un perimetro di scrittura vero, e diventa codice
  nuovo nel motore. Costa di più e vale di più.

---

## 3. Sailor vivo mentre ci si sviluppa sopra

### 3.1 Il verdetto: conviene, e lo strumento è già qui

`cargo-tauri` **2.11.4** è installato (`~/.cargo/bin/cargo-tauri`). Il suo aiuto
dichiara letteralmente: *«Run your app in development mode with hot-reloading for
the Rust code»*. Non serve installare niente.

`cargo-watch`, `bacon`, `watchexec`, `entr`, `fswatch`: **assenti tutti e cinque**.
Non vanno installati: sarebbero una seconda strada per la stessa cosa.

### 3.2 I numeri

Macchina: 19.327.352.832 byte di RAM (18 GB), 33–35% libera durante le misure,
carico 5,13 / 4,62 / 4,33 con tre cantieri al lavoro.

| Cosa | Tempo | Note |
|---|---|---|
| Modifica al solo guscio (`src/main.rs`), `-j 1` | **1,40 s / 2,30 s / 1,34 s / 2,58 s** | mediana ~1,8 s |
| Stessa cosa, parallelismo pieno | 1,82 s | il collo è il collegamento, non il parallelismo |
| Modifica a crate condivisi (`actions` + `toolbox` + guscio) | **9,78 s** | il caso peggiore ordinario |
| `cargo build` senza modifiche | 2,66 s | il guscio si rifà comunque: `tauri-build` rigenera a ogni giro |

Memoria di una ricompilazione del guscio: **451.395.584 byte di picco residente
(430 MiB)**, impronta di picco 90 MiB.

Interpretazione, che è il punto: **la memoria non è il vincolo qui.** 430 MiB su
18 GB non è ciò che ferma le compilazioni su questa macchina; ciò che le ferma è
la compilazione dell'intero workspace in `--release`, che è un'altra cosa e resta
un'altra cosa. Il guscio ha un `target/` proprio (3,1 GB, contro i 6,7 GB della
radice) perché sta fuori dal workspace di proposito: ricompilarlo di continuo non
tocca la cache degli altri cantieri.

Un ricompilamento ogni salvataggio costa **meno di due secondi** e mezzo giga di
memoria per un paio di secondi. Il rilancio a mano costa un cambio di contesto
ogni volta. Non c'è partita.

### 3.3 I comandi esatti

Un solo comando, dalla cartella del guscio:

```
cd /Users/theo/personal/sailor/desktop/src-tauri
cargo tauri dev --additional-watch-folders ../../crates
```

`cargo tauri dev` lancia **lui** Vite, perché `tauri.conf.json` dichiara
`"beforeDevCommand": "npm run dev"` e `"devUrl": "http://localhost:5183"`.

### 3.4 Le due trappole, misurate

**Non avviare Vite prima.** `vite.config.ts` ha `strictPort: true`. Se la 5183 è
già occupata, il secondo Vite esce con codice **1** e il messaggio `Port 5183 is
already in use` — e con lui muore `cargo tauri dev`, che l'aveva lanciato.
Misurato oggi.

**Fermare Vite col `kill` sbagliato lascia la porta occupata.** Ucciso il
processo `npm run dev`, la 5183 rispondeva ancora **HTTP 200**: il figlio `vite`
resta vivo e orfano. La volta dopo, `cargo tauri dev` fallisce con il messaggio
qui sopra e nessuno capisce perché. Si chiude il figlio, non il padre:
`lsof -ti tcp:5183` e si ferma quel pid.

### 3.5 Cosa non ho provato, e come si prova in un minuto

Il watcher di `tauri dev` osserva la cartella del guscio **più le dipendenze
locali del workspace**: nel sorgente della CLI la riga è
`watch_folders.extend(get_in_workspace_dependency_paths(tauri_dir)?)`. Il
problema: `desktop/src-tauri/Cargo.toml` dichiara un `[workspace]` vuoto, quindi
è un workspace a sé, e i crate `flow`, `ui`, `actions`, `ledger`, `toolbox` sono
dipendenze per percorso *fuori* da quel workspace.

**Non ho verificato se il watcher le copra.** Il codice che ho letto è del ramo
`dev` della CLI, non del tag 2.11.4 esatto. Per questo il comando in §3.3 porta
`--additional-watch-folders ../../crates`, che rende la domanda irrilevante.

La prova, se la si vuole: si lancia il comando, si cambia una riga in
`crates/ui/src/lib.rs`, si guarda se la finestra si riavvia. Sessanta secondi.

---

## 4. L'autocura

### 4.1 Ripara due materiali, non uno

Il sorgente Rust, e i flussi. Un `.flow.json` è un file di dati con regole
verificabili: è materiale riparabile esattamente come il codice, e per certi
versi meglio, perché il suo oracolo è più secco.

### 4.2 Il difetto vero, misurato oggi

Ho scritto un flusso rotto di proposito, con tre difetti dichiarati: uno strumento
che nessun descrittore conosce, una `answer_shape` pretesa ma mai messa nel
prompt, e un `accept` che nomina un esito impossibile.

```
sailor flow check rotto-apposta   →  «azioni mancanti: nessuna»,  uscita 0
sailor flow run   rotto-apposta   →  stato failed,                uscita 1
```

Nel deposito, il passo risulta: `Broke` / `invalid_input` / *«`accept` nomina
«esito_che_non_puo_esistere», che questo passo non può produrre; i valori
possibili sono: exit_error, timed_out, spawn_failed»*.

**Il controllo statico dà via libera a un flusso che non può girare.** Tre
difetti su tre invisibili. Questo è il lavoro numero uno dell'autocura dei
flussi, e non richiede nessun modello per essere trovato:

- ogni `tool` chiesto è dichiarato da un descrittore di `toolbox`? (la prova
  esiste già, ma solo per `smista-il-lavoro`, dentro
  `crates/sailor/tests/smista_il_lavoro.rs`: va resa generale)
- ogni `answer_shape` compare davvero nel prompt via `{"$json": "/answer_shape"}`?
  (a runtime è `shape_not_in_prompt`, e si paga scoprendolo tardi)
- ogni `accept` nomina solo esiti che quel passo può produrre?
- nessun passo nomina un `bin` con un percorso assoluto di questa macchina.

### 4.3 La rilevazione è deterministica, la proposta no

È la pratica migliore del playbook (fase 6, `bands.yaml`): *«detection is 100%
deterministic, no model»*. Qui si traduce senza sforzo, perché l'oracolo esiste
già ed è `cargo`.

- **rileva** un comando: `cargo test`, `cargo clippy`, `sailor flow check`. Rosso
  o verde. Nessun modello.
- **propone** un modello, dentro una forma dichiarata.
- **applica** il codice, dentro un perimetro.
- **giudica** un secondo modello, che non ha scritto la proposta.

Il vantaggio non è teorico: **se il rilevatore è verde, nessun modello viene
pagato.** Il flusso si ferma al primo passo.

### 4.4 I quattro cancelli: la prova che la riparazione era giusta

Questa è la risposta alla domanda difficile. La memoria di questo progetto porta
un caso preciso: venticinque righe sane riscritte perché un controllo aveva dato
un numero falso. Il flusso di autocura deve rendere quel caso impossibile, non
improbabile.

**Cancello 1 — il difetto si vede prima.** Il flusso non parte se non esiste un
comando che **fallisce adesso**. Non «un modello dice che c'è un problema»: un
comando rosso. Se il comando è verde, la corsa si chiude qui, e si chiude
`Broke`, non verde: un'autocura che non trova niente ha finito, un'autocura che
ripara ciò che non si vede rotto è il guasto.

Questo cancello, da solo, uccide il caso delle venticinque righe sane.

**Cancello 2 — dopo, quel comando è verde e nient'altro è diventato rosso.** La
riparazione si misura sul comando che falliva **e** sull'intera batteria. Un
verde locale con una regressione altrove è un rosso.

**Cancello 3 — la prova prova qualcosa.** Si applica la patch **inversa** e si
pretende che il comando torni rosso. Se resta verde, il controllo non stava
controllando la riparazione, e la patch viene rifiutata anche se tutto è verde.
È «rompi apposta ciò che provi», messo dentro il flusso invece che nella testa di
chi lo esegue.

**Cancello 4 — il perimetro.** La patch tocca solo i file che il piano aveva
nominato. Un file fuori elenco è un rosso, senza discussione. È il cancello che
prende il caso in cui il modello ha ragione sul difetto e torto sul confine.

E, sopra i quattro: **il verdetto va a un motore che non ha scritto la patch.**
Nessuno dei quattro cancelli è un giudizio: sono quattro misure. Il giudizio
viene dopo, e da un altro.

### 4.5 Il flusso, nel formato vero

Ho scritto la bozza e **l'ho fatta validare dal motore vero**, non a occhio:

```
sailor flow check autocura-dei-flussi
  passi: 4      cicli: nessuno      dipendenze: 3
  innesco <- nessuna
  il_difetto_si_vede <- innesco
  proposta <- il_difetto_si_vede
  verdetto <- proposta
  azioni mancanti: nessuna          uscita 0
```

La bozza vive fuori dall'albero (non ho scritto in `flows/`). Il passo che conta
è il primo dopo l'innesco, ed è il cancello 1 in forma di dato:

```json
{
  "id": "il_difetto_si_vede",
  "deps": ["innesco"],
  "action": "shell_check",
  "max_attempts": 1,
  "when": null,
  "with": {
    "command": "…il controllo che deve essere rosso adesso…",
    "env": { "FLUSSO": { "$from": "/text" } },
    "timeout_secs": 60
  },
  "input_schema": { "type": "any" },
  "output_schema": { "type": "any" }
}
```

Due cose da notare nel formato, perché cambiano come si scrivono i passi:

- il bersaglio arriva dall'innesco con un rinvio `{"$from": "/text"}`. Non è
  scritto dentro il flusso. Per riparare un altro flusso si cambia la consegna,
  non il grafo.
- il passo motore dichiara `answer_shape`, e la stessa forma finisce nel prompt
  con `{"$json": "/answer_shape"}`. Scritta una volta. Il motore rifiuta di
  spendere la chiamata se la forma non è nel prompt (`shape_not_in_prompt`), e
  pota la risposta ai soli campi dichiarati: i preamboli non attraversano la
  catena.

### 4.6 Cosa manca al motore perché questo flusso esista davvero

Dichiarato come mancante, non come dettaglio:

1. **un'azione che applica una patch** con il perimetro di §2.2 e specie
   `HandToHuman`. Oggi non esiste: l'autocura può proporre, non consegnare.
2. **un controllo statico dei flussi** che veda i tre difetti di §4.2.
3. **il file delle autorizzazioni** e il diniego incorporato.
4. **il canarino avversario** in `crates/sailor/tests/`.

Nessuno dei quattro è grande. Il quarto è quello da scrivere per primo, perché è
l'unico che si accorge se gli altri tre vengono disfatti.

---

## 5. Le buone pratiche, dal playbook a qui

### 5.1 Le sei che valgono

Ho letto il playbook per intero. Queste sei sono operative e traducibili oggi:

1. **`intent.md` → `spec.md` → `plan.md`, versionati.** Il piano si approva
   *prima* del codice, quando cambiare rotta è economico. Qui il piano è già
   parte del flusso: è l'uscita di un passo, con una forma dichiarata.
2. **«Quando Claude sbaglia due volte, la correzione va in `CLAUDE.md`.»** È una
   regola, non un consiglio, e questo albero ha già il posto: `AGENTS.md`, con la
   sua sezione di trappole pagate. Un flusso può proporre la riga; scriverla
   resta un gesto umano.
3. **Un bersaglio verificabile unico**, con l'uscita sana scritta accanto. Qui è
   `cargo test`, ed è già l'oracolo dichiarato.
4. **Per un difetto: prima il test che fallisce, poi la riparazione** — ed è
   letteralmente il cancello 1.
5. **Un gancio impedisce di modificare i test durante la riparazione.** È la
   difesa contro il modo più comune di far tornare verde una batteria. Qui
   diventa il cancello 4: i file dei test non stanno nel perimetro di una
   riparazione, mai.
6. **Chi scrive il codice non approva il codice.** Coincide con «chi crea non
   giudica», già scritto in `AGENTS.md`.

### 5.2 Le sei che ho scartato, e perché

- **PR, branch protection, `REVIEW.md`, revisione da bot in CI**: presuppongono
  una CI e più di una persona. Qui non ci sono. La loro sostanza sopravvive nel
  verdetto affidato a un motore diverso.
- **`bands.yaml` con soglie a 1σ/2σ/3σ su una metrica**: il meccanismo giusto,
  ma servono trenta giorni di serie storica per avere una media e una deviazione.
  Il deposito ha iniziato a riempirsi ieri. Va ripreso quando i dati esistono.
  **La parte che vale subito** è l'altra metà della stessa pratica: la rilevazione
  deterministica senza modello.
- **20–50 eval in CI a ogni modifica di `CLAUDE.md`**: qui la batteria `cargo
  test` fa già quel lavoro sul codice. Sui *prompt* dei flussi non lo fa nessuno,
  ed è un buco vero — ma è lavoro per dopo, non per ora.

### 5.3 Il flusso delle buone pratiche

Sei passi, e i primi tre non toccano il codice:

`innesco` (la richiesta di modifica) → `orientamento` (dove vive questa cosa:
ricerca semantica, §6) → `piano` (motore, forma dichiarata: file toccati, ordine,
prova che lo dimostrerà) → `esecuzione` → `verifica` (i quattro cancelli) →
`verdetto` (motore diverso).

Il passo che il playbook aggiunge e che l'autocura non ha: **il piano è un
artefatto, non un pensiero.** Esce dal passo `piano` come dato, entra nel passo
`verifica` come perimetro. Il cancello 4 confronta i file toccati con i file che
il piano aveva nominato — e quel confronto esiste solo se il piano è stato scritto
prima.

---

## 6. SocratiCode: a quale domanda risponde, e a quale no

Provato su questo repository, non descritto. Stato: 3.412 frammenti indicizzati,
watcher attivo.

**Dove vince, misurato.** Domanda: *come si impedisce che due flussi con nomi che
differiscono solo per le maiuscole si sovrascrivano?* La ricerca semantica dà la
risposta esatta **al primo posto, punteggio 1,00**:
`desktop/src-tauri/src/flows.rs`, funzione
`reject_a_name_that_collides_only_by_case`.

Con `grep` bisogna indovinare la parola. Le alternative, misurate sull'albero:
`case` → **212 righe in 66 file**; `eq_ignore_ascii_case` → 4 righe, ma è il nome
della funzione di libreria, cioè bisogna già sapere la risposta; `maiuscol` → 27
righe in 13 file, e funziona solo perché in questa casa i commenti sono in
italiano.

La regola che ne ricavo: **la ricerca semantica serve quando non conosci la
parola.** Il codice qui è in inglese e i commenti in italiano: è esattamente il
caso in cui `grep` fallisce due volte.

**Dove perde, misurato.** Domanda: *dove si decide che un passo è fallito invece
che riuscito?* Il file giusto — `crates/actions/src/lib.rs`, funzione `tolerates`
— arriva **ultimo, ottavo su otto, punteggio 0,20**. Al primo posto (0,70) un file
`.tsx` che non risponde. Un `grep accept` dentro quel file dà venti righe e la
risposta è la quarta. `grep` costa **0,10 s** su 4,5 MB di sorgente.

Quando la parola esiste — un identificatore, un campo, una costante — `grep` è
migliore, più veloce e completo.

**Dove mente, e questo va scritto in grande.**

`codebase_impact` su `crates/flow/src/graph.rs` risponde:

> *«Total impacted files: 0 — No callers found — nothing else depends on this.»*

`graph.rs` ha **493 righe e 12 elementi pubblici**. Otto `Cargo.toml` dichiarano
il crate `flow` come dipendenza. **22 file** scrivono `flow::`. Il grafo intero
conta **99 archi su 218 file**.

È il falso orfano già noto in questa casa, riprodotto oggi sul file che sta al
centro del formato dei flussi.

**La regola operativa per i flussi**, e va nel prompt del passo che usa
SocratiCode: *la ricerca semantica serve a orientarsi; il grafo delle dipendenze
non decide mai un perimetro.* Il perimetro lo decide `cargo` — chi dipende da un
crate lo dice il compilatore, non l'indice.

---

## 7. Ciò che non ho provato

Detto qui, tutto insieme, perché nessuno lo scambi per misurato.

- Che il watcher di `tauri dev` copra `crates/` senza `--additional-watch-folders`
  (§3.5). Prova: sessanta secondi.
- Il costo della **prima** compilazione a cache fredda del guscio. Le mie misure
  sono tutte incrementali, su un `target/` da 3,1 GB già caldo.
- Il flusso di §4.5 non è mai stato **eseguito**: è validato staticamente
  (`flow check`, uscita 0), non corso. Nessun motore è stato pagato.
- Nessuna misura del comportamento con più cantieri che compilano insieme lo
  stesso `target/`: le mie compilazioni hanno usato quello separato del guscio.
- Le pratiche del playbook sono state lette, non applicate: nessuna delle sei di
  §5.1 è in servizio.

---

## 8. Le decisioni che restano a Theo

**1. La falla di `shell_check` (§2.4).** Un passo può eseguire un comando
qualunque, quindi può scrivere ovunque — compreso il file che dice cosa
l'autocura può toccare. Due strade: una prova che vieta ai flussi del cantiere di
passare a `shell_check` un comando che viene da un modello (costo quasi zero,
prende il caso vero), oppure un perimetro di scrittura dentro il motore (costa di
più, chiude la classe). Finché non si sceglie, l'anello 2 del gate è una
convenzione, non una parete.

**2. Dove vive il file delle autorizzazioni.** La mia proposta è
`$SAILOR_HOME/autocura.json`, fuori dall'albero dei sorgenti, quindi fuori da ciò
che l'autocura ispeziona. L'alternativa è tenerlo nell'albero e versionarlo, dove
si vede nei diff ma è a portata di mano dell'autocura. Non sono equivalenti: la
prima difende, la seconda documenta.

**3. Se i flussi si spediscono, e da quale cartella.** Oggi `crates/release` non
ne spedisce nessuno (0 occorrenze di «flow»). Serve la decisione prima che
qualcuno scriva quella riga: `flows/` spedito, `flows-cantiere/` mai — e le due
prove di §1.2 che lo rendono vero invece che scritto.
