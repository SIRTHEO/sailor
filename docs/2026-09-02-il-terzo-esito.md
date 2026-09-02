# Il terzo esito: «non ancora, riprova al battito dopo»

**02/09/2026.** Proposta motivata, non un fatto compiuto: la forma qui sotto è
scritta e provata, ma **la scelta resta ribaltabile da Theo**. Se la forma non
convince, quello che si butta è un `enum` con una variante in più e due campi
opzionali in `Step` — non un impianto.

## Il buco, e perché non è locale

Il motore conosce cinque esiti (`crates/flow/src/record.rs`), e per decidere
cosa è pronto ne guarda due (`decision_from`, `crates/flow/src/executor.rs`):

- **`Broke`** rientra fra i pronti **nello stesso giro del ciclo di `execute`,
  senza nessuna attesa.** Non c'è nessun `sleep` e nessun backoff da nessuna
  parte nell'esecutore. Un passo con `max_attempts: 3` brucia i tre tentativi
  in una frazione di secondo, sulla stessa condizione del mondo.
- **`Waiting` non rientra mai.** Finisce nel suo secchio e ci resta: nessuna
  ripresa e nessun battito lo riprovano.

`take_mandate` (`crates/relay/src/lib.rs`) è costruito su `Waiting`, perché la
pausa *è* il suo scopo: aspetta che l'agente scriva il proprio mandato. Quindi
la staffetta `passa-il-testimone` arriva a `raccogli-il-mandato`, parcheggia, e
resta parcheggiata per sempre — un mandato scritto dopo la pausa non viene mai
riletto. È il guasto 62.

**Il motore non ha un esito che significhi «non ancora, riprova più tardi».** E
la forma che gli si dà **la ereditano tutte le azioni registrate**, comprese
quelle che nessuno ha ancora scritto: `ActionOutcome` è il tipo di ritorno di
ogni `Action::execute`. Per questo si disegna prima.

## Le forme possibili, e cosa costa ognuna

### A — Nessun esito nuovo: si usa `Broke` con un'attesa

Un'azione che deve aspettare restituisce un errore, e il motore ritarda i
tentativi. Costa **zero varianti nuove**.

Si scarta, e non per eleganza. `Broke` scrive `failure_class`, entra nel conto
dei passi rotti del cruscotto (`crates/ui/src/dashboard.rs`), e dopo
`max_attempts` diventa `Decision::Failed`. Una staffetta che aspetta il suo
turno registrerebbe una fila di guasti che non sono guasti, e un flusso a ronda
apparirebbe rotto a ogni giro — che è esattamente la ragione per cui il tetto di
spesa ha una parola sua (`cap_reached`) invece di chiamarsi `failed`: *«un flusso
notturno che tocca il proprio tetto ogni notte apparirebbe rotto ogni notte, e
chi guarda smetterebbe di guardare»* (`docs/decisioni.md`). Qui vale identico.

### B — Nessun esito nuovo: si fa rientrare `Waiting` dopo un'attesa

Ancora zero varianti. Si scarta, ed è la scelta più pericolosa delle cinque.

`Waiting` oggi ha **un significato preciso e usato**: il passo è in mano a una
persona o a un agente vivo. `sailor step open` apre **solo** i passi in quello
stato (`crates/sailor/src/step_cmd.rs`), `Ledger::waiting_runs()` li elenca
perché qualcuno li raccolga, e la riconciliazione dopo un crash ci manda i passi
il cui effetto non si sa ispezionare.

Farlo rientrare fra i pronti vuol dire **rilanciare col motore un passo che una
persona sta tenendo in mano**. Ed è precisamente il difetto da cui il recupero
dai crash è stato spostato *via*: il commento in `reconcile` lo dice ancora —
«l'unica scelta sicura era waiting, e un passo in attesa non torna mai pronto».
Chi sceglie B ripara il guasto 62 riaprendo quello per cui `Waiting` esiste.

### C — Un terzo esito, con **l'attesa dichiarata dall'azione**

`ActionOutcome::NotYet { reason, retry_after_secs }`: l'azione dice sia *che*
deve aspettare sia *quanto*.

Si scarta per il vincolo permanente «programmiamo a codice solo ciò che tocca il
mondo», e per un precedente scritto dieci righe dentro il codice della staffetta
stessa. `MeasureSpec::ceiling` è obbligatorio e **senza valore predefinito**, con
questa motivazione accanto: *«cosa conti come troppo pieno è una decisione, e una
presa dentro un nodo non si potrebbe più discutere dal flusso che lo usa.»*
Quanto aspettare è la stessa specie di decisione. Con C, ogni autore di azione si
inventa un numero, e chi legge il `.flow.json` non lo vede.

### D — Un terzo esito, con **l'attesa dichiarata dal passo nel flusso** ← scelta

L'azione dice **se** («non ancora, e perché»); il flusso dice **quanto**.

```
ActionOutcome::NotYet(String)      // il motivo, dall'azione
Outcome::NotYet                    // l'esito registrato
Step.ask_again_after_secs: u32?    // quanto aspettare dopo un «non ancora»
Step.retry_after_secs:     u32?    // quanto aspettare dopo un guasto
Decision::NotYet { steps, due_at } // e quando quel gruppo torna pronto
```

Costo: una variante in due `enum` pubblici, due campi opzionali nel grafo, un
parametro `now` in `decision_from`. E **un errore di compilazione** in ogni
`match` esaustivo su `ActionOutcome` o su `Outcome` dell'albero — che è il costo
giusto, perché è rumoroso: nessun posto resta indietro in silenzio.

### E — La D, ma con un'attesa sola per i due casi

Un solo campo che vale sia per `NotYet` sia per `Broke`. Più piccolo, e sbagliato
per un motivo concreto: «chiedi di nuovo fra un minuto» e «riprova questo guasto
fra dieci minuti» sono due politiche diverse, e un campo solo le lega. Il costo di
tenerle separate è un campo opzionale in più; il costo di legarle lo paga chi
scriverà il primo flusso che vuole le due cose diverse, e non potrà.

## La scelta: D, e le regole che ne discendono

### 1. L'azione dice se, il flusso dice quanto

`NotYet(String)` porta solo il motivo, che finisce in `said` come per ogni altro
esito. Nessun numero nasce dentro un'azione.

### 2. `NotYet` non consuma un tentativo — `max_attempts` conta i **guasti**

`raccogli-il-mandato` dichiara `max_attempts: 1`. Se un «non ancora» contasse
come tentativo, il primo giro lo farebbe fallire, e la riparazione sarebbe
peggiore del guasto. Quindi il tetto dei tentativi si misura **sui record
`Broke`**, non sul numero d'ordine del tentativo.

**Questo non cambia niente per i flussi di oggi, ed è verificabile a mente**: il
numero d'ordine parte da 1 alla prima esecuzione e cresce di uno a ogni
esecuzione successiva; un passo `Went` finisce, uno `Skipped` non si ritenta, uno
`Waiting` non torna pronto. Quindi finché non esiste `NotYet`, «numero d'ordine»
e «quanti `Broke` ci sono» sono lo stesso numero.

Il prezzo, dichiarato: un passo che risponde sempre «non ancora» **interroga per
sempre**. Il motore non ha un tetto sui sondaggi, e non deve averne uno: chi si
ferma è chi smette di far girare il battito. Un tetto sui sondaggi lo si può
aggiungere il giorno che qualcuno lo chiede, e sarà un terzo campo del passo.

### 3. Il motore non dorme mai, e non è un dettaglio d'implementazione

Quando niente è pronto e qualcosa è «non ancora», la corsa **finisce** con
`Decision::NotYet`. L'esecutore non aspetta: aspettare dentro `execute` vorrebbe
dire un processo fermo a tenere una corsa, che è il guasto 4 in un'altra forma.

`due_at` — l'istante più vicino in cui qualcosa torna pronto — viaggia **dentro
la decisione**, così chi la mostra non deve ricalcolarlo leggendo i record.
Stessa forma di `SpendStop`: la decisione porta i dati, mai la frase.

**Chi rilancia non è deciso qui, e questo lavoro non installa niente.** Né
`launchd`, né demoni, né script che rilanciano, né cron. Quella scelta è di Theo
ed è aperta; nel frattempo `sailor flow resume <corsa>` è il battito a mano, e
funziona.

### 4. `ask_again_after_secs` assente vuol dire «non in questa invocazione»

Asimmetria voluta fra i due campi, e va detta perché sorprende:

| campo | assente vuol dire |
|---|---|
| `retry_after_secs` (dopo un `Broke`) | **il comportamento di oggi**: il passo rientra fra i pronti nello stesso giro |
| `ask_again_after_secs` (dopo un `NotYet`) | il passo **non** rientra in questa invocazione; torna pronto alla prossima |

`retry_after_secs` ha un «oggi» da conservare, e lo conserva parola per parola.
`ask_again_after_secs` non ce l'ha: se assente volesse dire «subito», `execute`
girerebbe in tondo sullo stesso passo per sempre. Un blocco vivo non è un valore
predefinito ragionevole.

In pratica il motore somma un secondo, che è la grana del suo orologio — conta
secondi interi — e quindi il più piccolo intervallo che sa esprimere. **Non è una
politica**: è la differenza fra «adesso» e «dopo».

### 5. Cosa cambia per un'azione che non lo usa: niente

Va dichiarato esplicitamente, perché la forma la ereditano tutte.

- **A esecuzione: niente.** Un'azione che non restituisce mai `NotYet` non può
  raggiungere il ramo nuovo. Un passo che non dichiara nessuno dei due campi si
  comporta come prima, guasto per guasto e ritentativo per ritentativo.
- **Alla compilazione: un'arma in più** in ogni `match` esaustivo. Sono nove
  posti nell'albero, tutti già scritti come `match` esaustivi apposta.
- **Nel deposito: nessuna migrazione.** `steps.outcome` è testo, `runs.status` è
  testo libero. Un record vecchio si rilegge identico; un record nuovo con
  `"NotYet"` non lo sa leggere un binario vecchio, che è il verso giusto in cui
  sbagliare.
- **Nei file di flusso: nessuna riscrittura.** I due campi hanno
  `#[serde(default)]` e non si serializzano quando sono assenti, quindi un
  `.flow.json` scritto ieri si rilegge e si riscrive byte per byte.

### 6. Perché `NotYet` e non `Deferred`

Il vocabolario del motore è di parole piane e corte: `Went`, `Broke`, `Waiting`,
`Stopped`, `Skipped`. `NotYet` sta in quella fila; `Deferred` è la parola di un
manuale. Il secondo parere (Codex, in sola lettura) proponeva `Deferred`: preso
tutto il resto del suo parere, questa sola cosa no.

## Il secondo parere, e cosa gli ho preso

Chiesto a Codex CLI in sola lettura prima di scrivere una riga. Quattro obiezioni
su cinque sono entrate nel disegno:

1. **«La decisione deve portare l'istante assoluto, non solo i nomi dei passi»** —
   preso: `Decision::NotYet { steps, due_at }`.
2. **«`NotYet` con attesa assente è ambiguo: se vuol dire subito, il difetto
   torna»** — preso, ed è il punto 4 qui sopra. Era il buco più serio della prima
   stesura.
3. **«Il tetto dei tentativi va applicato solo ai guasti»** — preso, punto 2.
4. **«Un'attesa sola per i due casi lega due politiche diverse»** — preso, è la
   ragione per cui la forma E è scartata.
5. **«Chiamalo `Deferred`»** — non preso, punto 6.

Ha segnalato anche di validare la durata e l'aritmetica: un'attesa negativa
renderebbe tutto pronto subito, e la somma può traboccare. Risolto **col tipo**
invece che con un controllo: `u32` non rappresenta un negativo, serde rifiuta un
JSON negativo da solo, e la somma si fa in `i64` con `saturating_add`.

## Il residuo, dichiarato

- **La finestra non conosce questo esito.** `STATE_OF_OUTCOME` in
  `desktop/src/runstate.ts` non ha `NotYet`, quindi il nodo ricade su «in
  attesa» — che è il ripiego che quel file dichiara per un esito ignoto, ma dice
  «parcheggiato» di un passo che invece torna. Non l'ho toccato: aggiungere uno
  stato al disegno vuol dire una tinta, due cataloghi di parole e la prova sul
  contrasto, e `desktop/src-tauri` dichiara un workspace suo che **nessun `cargo
  test --workspace` compila** — cioè non c'è modo di renderlo rosso dal gate.
  È un lavoro, non una riga.
- **La staffetta non dichiara nessuna attesa**, e resta con il campo assente.
  Mettere un numero lì dentro vorrebbe dire decidere il periodo del battito, che
  non è deciso: una cifra inventata è peggio di una mancante.
- **Quanti sondaggi siano troppi non lo sa nessuno.** Vedi il prezzo al punto 2.
- **`sailor flow resume` esce diverso da zero su una corsa «non ancora»**, come
  già fa su una corsa in attesa: `run_status` risponde `false` e `resume_run_in`
  restituisce un errore. Non l'ho cambiato — è la semantica che `waiting` ha da
  sempre, e un codice d'uscita è una decisione a sé — ma chi scriverà il battito
  deve saperlo prima, non scoprirlo da un allarme notturno.
- **La staffetta intera non è stata fatta girare, e non si può su questa
  macchina**: il perimetro nega `openpty`, e 21 prove cadono lì dentro — le
  stesse 21 prima e dopo questo lavoro, verificate mettendo il lavoro da parte e
  rimisurando. È provato il passo che parcheggiava, con l'azione vera e il motore
  vero (`crates/relay/tests/the_handover_gets_picked_up.rs`); gli altri tre nodi
  scrivono in una sessione viva e restano non provati qui.
