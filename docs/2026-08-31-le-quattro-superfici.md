# Le quattro superfici: cosa Sailor espone, e cosa invece si compone

**31/08/2026.** Nato da una domanda di Theo — «quali nodi di prodotto mancano al
sistema?» — e dalla risposta sbagliata che ha ricevuto per prima: un elenco di
buchi. Un elenco di buchi invecchia in una settimana e non dice dove mettere la
cosa successiva. Questo documento prova a dire la stessa cosa in modo che regga:
non *quali nodi mancano*, ma **quale superficie il sistema espone**, così che la
domanda diventi «quale potere non abbiamo ancora, e quale flusso me lo
dimostra».

## Da dove viene, con i numeri

Il 31/08 è stato censito `dev-stack`, l'orchestratore dell'ambiente di sviluppo
di un altro progetto: 27 script, ~2.400 righe di shell, 35 ricette. La domanda
iniziale era «quali di questi diventano flussi di Sailor». Il censimento ha
risposto un'altra cosa, ed è questa: **22 voci su 52 sono migrabili, 15 sono
ferme dietro una capacità che Sailor non ha.** Ma le quindici non chiedevano
quindici nodi diversi: chiedevano **cinque poteri** — tenere vivo un processo,
terminarne uno, parlare in rete, portare un segreto senza scriverlo, restituire
un valore invece di un esito.

Nello stesso momento sette cantieri aperti su Sailor stavano costruendo
`supervisor`, `terminal`, `presence`, `mcp` — cioè quei poteri, ognuno con la
propria forma, senza un criterio comune su cosa fossero.

Il difetto che questo documento vuole impedire ha già un precedente misurato:
**la finestra offre otto tipi di passo e il motore ne esegue tre**, e nessuno se
n'era accorto perché i tipi non stanno in un posto solo.

## Il confine, che è già scritto

Il vincolo permanente dice: *«programmiamo a codice solo ciò che tocca il mondo…
il confine è il potere, non "esegue contro decide"»*. Da lì discendono due
frasi, e sono tutto il documento:

- **Il codice espone poteri.** Non nodi: poteri.
- **I flussi compongono poteri.** Un'orchestrazione è un file di dati che mette
  in fila poteri che il motore già espone. **Se un'orchestrazione richiede
  codice nuovo, manca un potere — non manca un flusso.**

## Le quattro superfici

Ogni azione registrata appartiene a una sola di queste, e lo dichiara.

### 1. `sense` — leggere il mondo senza toccarlo

Processi vivi, porte, carico, disco, rete, stato di un repository, l'indice del
codice, e il consumo: quanto è stato speso, quanto resta, su quale fornitore.

Due proprietà rendono un'azione un sensore, e la seconda è quella che si
dimentica:

1. non cambia niente;
2. **distingue «zero» da «non posso vedere»**.

La seconda viene dal guasto 12: `pgrep` dentro il perimetro rispondeva vuoto
*senza errore*, e una sorveglianza ha dichiarato «nessun flusso in esecuzione»
mentre due giravano. Un sensore cieco che risponde zero è peggio di un sensore
assente, perché il flusso a valle si fida.

### 2. `act` — toccare il mondo

Avviare e fermare un processo, scrivere un file, invocare un motore, fare una
richiesta di rete, committare.

Un attuatore dichiara **cosa può rompere**: è il modello Bazel già deciso il
29/08 — un passo dichiara cosa gli serve e il resto per lui non esiste. Un
attuatore che non lo dichiara non si registra.

### 3. `remember` — il deposito, come fonte a cui si fanno domande

Corse, costi, guasti, decisioni. Non un archivio: una cosa che si interroga. La
distanza da colmare è scritta in un mandato di agosto — *«Sailor registra tutto
quello che succede e non torna mai a leggerlo»*.

### 4. `gate` — chi può cosa, e dove entra una persona

Il permesso umano non è un nodo speciale: è la dichiarazione che certi poteri,
in certi contesti, vogliono una firma. Discende da due vincoli permanenti già
scritti — «chi crea non giudica» e il giudizio umano che resta sopra il ciclo.
Finché vive fuori dal sistema, ogni cancello è un'usanza e non un meccanismo.

## Cosa deve dichiarare un'azione, per essere registrata

1. **la superficie** — una sola fra `sense`, `act`, `remember`, `gate`;
2. **i poteri che pretende** — rete, disco, processi, denaro, segreti;
3. **cosa risponde quando non può rispondere** — obbligatorio per `sense`, ed è
   il guasto 12 reso impossibile.

I nomi delle superfici stanno in inglese perché li legge il compilatore; tutto
ciò che legge una persona resta in italiano, come già deciso il 31/08.

## Cosa vuol dire «aperto», in tre proprietà

**Si chiede, non si sa.** Un flusso non contiene l'elenco di ciò che esiste: lo
interroga. È la cura già scritta per gli strumenti — *«chiedere al motore, non
tenere una lista»* — estesa a poteri, competenze e fornitori. Quando vale
ovunque, la domanda «quali nodi mancano» non serve più: risponde il sistema.

**Si aggiunge senza ricompilare chi lo usa.** Un potere nuovo, anche di terzi,
entra con un descrittore — la stessa forma con cui entra un motore. È già la
direzione di prodotto numero 3: *«un progetto esterno si collega come azione
nuova; non serve inventare un meccanismo, serve scegliere il progetto»*.

**Si porta via.** Un flusso dichiara tutto ciò che pretende, e chi lo riceve o
lo esegue o **gli viene detto perché non può**. Oggi è il contrario: il guasto 17
(competenze presenti su una macchina sola, non dichiarate) e il guasto 25 (la
radice del repository scritta dentro il flusso) sono lo stesso difetto visto da
due lati.

## Perché non basta scriverlo qui

*«Chi scrive una regola nuova scrive anche ciò che la rende rossa.»* Questa ha il
suo controllo, e senza non entra:

> una prova che scorre il registro delle azioni e **fallisce se un'azione non
> dichiara la propria superficie e i propri poteri**; e, per le sole `sense`,
> fallisce se non dichiara cosa risponde da cieca.

Nasce rossa sulle nove azioni di oggi — nessuna dichiara niente — ed è il modo
in cui questa pagina resta viva invece di diventare la descrizione di ciò che è
già successo.

## Il debito che questo documento dichiara

I sette cantieri aperti il 31/08 hanno prodotto crate nuovi **prima** di questo
criterio. Adeguarli è una decisione di Theo: se non si adeguano prima di
chiudere, la regola nasce con quattro eccezioni non scritte, che è il modo in cui
la finestra è arrivata a otto tipi contro tre.
