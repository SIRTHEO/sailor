# Le decisioni

**Questo file è la memoria delle scelte, e i flussi lo leggono.** Non è un
diario: ogni voce è una decisione che vincola il lavoro futuro, con chi l'ha
presa e perché. Un flusso che sta per scegliere cosa fare, o che sta per
implementare qualcosa, lo consulta **prima** — altrimenti riprende una strada
già scartata, e nessuno se ne accorge finché non è scritta.

**Perché esiste.** La notte del 29/08/2026 sono state prese sette decisioni. Non
esistevano da nessuna parte se non nei messaggi di commit e nella conversazione
in cui erano nate: il flusso lanciato il giorno dopo non poteva conoscerle.
Questo è il difetto che separa un sistema che impara da uno che ricomincia.

**Una decisione si scrive qui quando vincola qualcuno che non era presente.**
Se riguarda solo chi l'ha presa e finisce con lui, non è una decisione: è una
scelta di lavoro, e sta nel commit.

## I vincoli permanenti

Non sono decisioni prese una volta: sono il metro con cui ogni altra si giudica.
Una proposta che li viola si scarta, anche quando è migliore sotto ogni altro
aspetto.

| vincolo | cosa vuol dire in pratica |
|---|---|
| **Indipendenza dal modello** | Sailor funziona con qualunque strumento a riga di comando, compresi quelli che non esistono ancora. Una soluzione che funziona solo su un motore preciso va **dichiarata come capacità** di quello strumento, e chi non ce l'ha deve continuare a funzionare pagando di più. |
| **Chiarezza per chi guarda** | Sailor esiste perché una persona veda e controlli cosa fanno i suoi strumenti. Un'ottimizzazione che rende opaco come i passi si passano le informazioni è **peggio del costo che risparmia**. Vale anche per l'aspetto: un'interfaccia che nasconde cosa succede è il contrario del prodotto. |
| **Lo schermo è il giudice** | Una regola di progetto che non si può verificare guardando un'immagine è un'opinione. Viene dai due difetti che né i tipi né le prove hanno visto. |
| **Chi crea non giudica** | Il verdetto su un lavoro lo dà chi non l'ha scritto. Un motore che verifica se stesso ha già in contesto le proprie conclusioni: non è distratto, è compromesso. |
| **Una prova vale solo se poteva venire diversa** | Dopo averla scritta si rompe apposta ciò che prova, e si guarda che diventi rossa. Chi dichiara di non averlo fatto viene respinto. |
| **Programmiamo a codice solo ciò che tocca il mondo** | Il motore che esegue, il deposito che registra, il gate che autorizza. Tutto il resto è un flusso, modificabile senza ricompilare. Il confine è **il potere**, non «esegue contro decide». |

## Le decisioni prese

### English everywhere, restoring the charter the project was founded with

**2026-09-01**, decided by Theo.

Identifiers, comments, documentation, commit messages, and every message a user
of the tool can see are in **English**. There is no inside language and no
outside language: the repository is public, and what is committed here is
world-readable permanently.

**This is not a new rule. It is one that was lost.** It lived in a `CLAUDE.md`
on an orphan branch with an unrelated history — one commit, never published,
reachable from nothing. It also said: no absolute paths from a developer
machine, no employer or client names, no internal repository names, no
transcripts or logs copied out of private tooling, no framing of this work as a
reaction to or comparison with somebody else's product — and it named
`~/personal/.sailor-notes/`, a directory with no git remote, as where notes that
cannot meet those rules belong. The branch is kept as the tag
`archivio-primo-abbozzo`.

**Why it matters more than its content.** This project spent days
rediscovering, one incident at a time, things that were written in its first
commit. The morning of the same day a partial version of this very rule was
decided again from scratch — English for what a stranger reads, Italian inside
— by looking at a CI file. That is the most expensive shape of the defect the
project keeps chasing: not a rule nobody interrogates, but **a rule nobody
could read**. A rule on an unreachable branch is worse than a missing one,
because everybody assumes the ground was covered.

**What does not change.** Flow and step ids, and the `.flow.json` filenames,
stay as they are — see the 2026-08-31 entry. That is not an exception to the
language rule: what the compiler reads is language, what the ledger keeps is
data.

**How it gets done, and the order.** Prune first, translate after. The six-line
comment cap already requires 636 blocks to shrink or go, and those carry two
thirds of the comment volume: translating a comment that should be deleted pays
for the same line twice. The measure is
`cargo test -p sailor --test comments_do_not_crowd_out_the_code`, whose Italian
count — 11,854 lines the day of the decision — can only go down. This file, and
the rest of `docs/`, convert as they are touched.

### La lingua si sceglie su chi legge: inglese ciò che vede uno sconosciuto, italiano ciò che vede chi lavora qui

**01/09/2026**, decisa da Theo guardando la CI.

`README`, file della CI e **i messaggi che un utente dello strumento vede** —
quello che `sailor` stampa, quello che dice quando rifiuta, i testi di
`--help` — vanno in **inglese**. Commenti dentro il codice, messaggi delle
prove e tutto ciò che sta sotto `docs/` restano in **italiano**.

**Cosa cambia rispetto a prima.** `AGENTS.md` diceva «commenti e messaggi in
italiano», in una riga sola, senza distinguere i due tipi di messaggio. Il
confine che c'era — «ciò che il compilatore legge sta in inglese, ciò che il
deposito conserva è un dato» — divideva bene il codice e non aveva niente da
dire sulla vetrina: finché il repo era privato, la vetrina non esisteva. Il
giorno in cui `main` è diventato Sailor la domanda è nata da sola.

**L'occasione.** Il file della CI, scritto il 31/08, aveva tre lavori chiamati
`prove`, `stile` e `finestra`: chiavi che leggono `needs:`, le API di GitHub e
`gh run`, cioè identificatori, su una pagina che chiunque può aprire. La
regola sugli identificatori c'era già; a mancare era chi la interrogasse sui
`.yml`, perché `identifiers_are_in_english` leggeva solo i sorgenti Rust. Ora
legge anche le chiavi dei lavori.

**Il confine è chi legge, non che tipo di file è.** Un `panic!` dentro una
prova parla a chi lavora qui: italiano. Lo stesso `panic!` su un percorso che
un utente può battere parla a lui: inglese. Il caso ambiguo si chiede, non si
risolve scegliendo la lingua comoda.

**Cosa NON tocca.** Gli `id` dei flussi e dei passi e i nomi dei file
`.flow.json` **restano in italiano**: sono dati che il deposito conserva, e la
decisione del 31/08/2026 che li protegge vale ancora — rinominare un passo
farebbe apparire le corse già registrate come passi sconosciuti. Chi legge
questa voce e pensa che i flussi debbano seguire la vetrina, chieda prima.

### I rinvii si sciolgono in un posto solo, dopo la condizione; e `input` è ciò che il passo ha ricevuto

**01/09/2026**, dal guasto 28.

Come un passo riceve il lavoro del passo prima **non è una scelta della singola
azione**: è la semantica del grafo, e sta in `flow::step_input` — l'unico punto
attraversato da ogni passo di ogni corsa. Un'azione non risolve i propri rinvii,
li riceve già sciolti, come riceve già risolto il `workdir`. Ogni azione
registrata la eredita, comprese quelle che nessuno ha ancora scritto.

**L'ordine dentro quel punto è la decisione, e non è un dettaglio
d'implementazione**: comporre le dipendenze col `with`, risolvere il `workdir`,
valutare il `when`, e sciogliere i rinvii **solo se il passo gira**. *Perché il
`when` prima*: un passo saltato riceve l'ingresso monco della dipendenza che non
c'è, quindi i suoi puntatori non trovano niente — e un passo che non gira non
deve rompersi per un lavoro che non farà. Misurato su
`flows/chiedi-all-indice.flow.json`, che con l'ordine opposto passava da
«completato» a «terminato con stato failed». *Il prezzo, dichiarato*: un
`workdir` scritto come `{"$from": …}` non viene attaccato alla radice. Era già
così, e vale meno del caso qui sopra.

**Che cosa questo obbliga chi scrive un controllo.** Una prova di comportamento
resta verde con una copia della risoluzione rimessa dentro un'azione: il
comportamento non cambia, cambia solo il numero di posti in cui vive la regola —
che è il guasto. Quindi la guardia **conta i posti**
(`crates/sailor/tests/references_are_resolved_in_one_place.rs`), e ha a sua
volta delle prove che interrogano lei: due volte quel lettore è stato cieco in
silenzio, e un controllo che si può spegnere da solo non è un controllo.

**E che cosa vuol dire `StepRecord::input`, che questa decisione ha cambiato.**
Una regola sola: *l'ingresso come il passo l'ha ricevuto nel momento in cui ha
smesso di essere elaborato.* Sciolto se il passo gira; **non** sciolto se è
saltato o se si è rotto proprio sciogliendo un rinvio — nel secondo caso
apposta, perché chi legge deve vedere il puntatore da correggere e non il vuoto
che ne è uscito. **Quale dei tre sia non lo dice quel campo: lo dice `outcome`,
nello stesso record.** Va scritto qui perché un `{"$from": …}` letto in un
record non è di per sé un difetto — su `Skipped` è la norma, su `Broke` è la
diagnosi, su `Went` sarebbe un guasto — e chi legge il deposito senza questa
riga leggerebbe le tre cose allo stesso modo.

**Il residuo, misurato e non nascosto**: una corsa iniziata prima del 01/09 e
ripresa dopo confronta un'impronta vecchia grezza con una nuova sciolta, e
dichiara `DifferentInput` sullo stesso lavoro. È un'etichetta sbagliata su un
tentativo, non un dato perso, e si esaurisce da sé quando quelle corse finiscono.

### Un passo si può consegnare all'agente vivo; il giudizio no
**31/08/2026.** Un passo può dichiarare l'azione `handed_to_agent`: descrive il
lavoro e **non avvia niente**. A eseguirlo è l'agente già vivo nel terminale, che
poi rientra nel sistema con `sailor step open` e `sailor step close`. Il record
del passo resta quello di sempre — intenzione scritta prima, esito scritto dopo —
e la corsa non si accorge di chi c'era in mezzo.

**Perché, con la misura.** Un flusso di quattro passi costa **2,79 volte** un
singolo prompt sullo stesso compito, e il rapporto dei consumi **è il rapporto
dei turni**: 62 contro 30. Non legge di più per turno (+8%): fa il doppio dei
turni, perché ogni passo avvia un processo che riscopre il repository da zero.
Ingrossare il passaggio fra i passi peggiorerebbe le cose; la cura è non
riaprire una conversazione che è già aperta.

**Che cosa questo non concede, ed è il punto.** Consegnare l'esecuzione non è
consegnare il verdetto. Chi ha chiuso un passo **non può aprire né chiudere** un
passo che da quello dipende: è il vincolo permanente «chi crea non giudica»
applicato al gesto in cui il giudizio si scrive. Il rifiuto vale in tutti e due i
punti apposta — solo all'apertura si aggirerebbe aprendo con un nome e chiudendo
con un altro.

**La negazione è il predefinito, non una lista di permessi.** Un flusso che vuole
davvero la stessa mano lo dichiara passo per passo con `"same_holder_ok": true`.
Il verso conta: una lista di permessi dimenticata lascia passare tutto e nessuno
se ne accorge; una negazione dimenticata al massimo ferma un lavoro, e si vede
subito.

**Chi tiene un passo consegnato è una scadenza, non un processo.** `held_by_pid`
resta vuoto e nessuno chiede niente al sistema operativo: è il guasto 12, dove
`pgrep` dentro il perimetro rispondeva vuoto *senza errore*. La ripresa
(`sailor flow resume`) confronta `handoff_timeout_secs` con `started_at`; ciò che
non sa vedere — un record con un pid, o senza scadenza leggibile — **non lo
dichiara morto**.

**Due debolezze dichiarate, scritte nel codice e non solo qui.** (1) `--as <chi>`
è un nome che se lo sceglie chi lo scrive: Sailor non ha nessun identificativo di
sessione da leggere, quindi il rifiuto qui sopra vale contro la distrazione, non
contro chi vuole aggirarlo. (2) Su un flusso con consegne il **tetto di spesa
smette di essere una garanzia**, perché il consumo dell'agente è autodichiarato
(`sailor step close --turns`). Per questo quella riga porta `cost_micros` vuoto e
non un numero stimato: così entra in `Spend::calls_without_cost`, `is_complete()`
diventa falso, e ogni posto che mostra il tetto dice già che la spesa vera è più
alta. Un costo inventato renderebbe *completa* una somma che non lo è.

### La lingua: identificatori in inglese, tutto il resto in italiano
**31/08/2026.** Ogni cosa che il compilatore legge sta in inglese — funzioni,
tipi, campi, variabili, moduli, costanti, **nomi di file**, classi CSS, chiavi
JSON. Ogni cosa che legge una persona sta in italiano: commenti, messaggi
d'errore, testo nella finestra, documenti, e i **dati** delle prove.
**Perché sta qui e non solo in `AGENTS.md`.** Ci stava solo lì, e il 31/08 se ne
contavano 136 violazioni — quasi tutte scritte nei tre giorni precedenti, da
sessioni che avevano ricevuto «rispondi in italiano» come istruzione forte e
questa riga come una fra molte in un documento. Questo file è la memoria che si
rilegge prima di correggere qualunque cosa: se una regola non è qui, non è
vincolante nei fatti, qualunque cosa dica altrove.
**E soprattutto ha una misura.** `cargo test -p sailor --test
identifiers_are_in_english` cerca parole italiane in posizione di dichiarazione,
e conosce anche i nomi dei file. Non è un analizzatore: è un elenco di parole,
che non ha falsi positivi e lascia passare quelle che non conosce. Il prezzo è
dichiarato; l'alternativa era continuare a non misurare niente.
**La lezione, che vale oltre la lingua.** Una regola che nessun controllo
interroga non diventa rossa mai — è lo stesso difetto del puntatore morto che
`AGENTS.md` racconta di sé, e del guasto 22, dove uno zero mai calcolato è
passato per una misura. Chi scrive una regola nuova scrive anche ciò che la
rende rossa.

### Gli identificativi dei flussi e dei passi restano in italiano
**31/08/2026 — Theo.** `sviluppa-sailor`, `verdetto`, `implementa`, i nomi dei
file `.flow.json`: restano come sono. Il confine non è fra codice e dati in
astratto — è questo: **ciò che il compilatore legge sta in inglese; ciò che il
deposito conserva è un dato, e i dati non si rinominano per stile.**
**Perché**, con le due conseguenze che nessun compilatore prende. (1) Il
deposito ha corse già registrate con quegli `step_id`: un passo `verdetto`
diventato `verdict` fa apparire il vecchio come sconosciuto e il nuovo come mai
eseguito, e la ripresa dopo crash non ritrova più i propri passi. (2) La
decisione «i flussi di sistema stanno dentro il binario» dice che chi ne vuole
uno diverso ne scrive uno **con lo stesso nome** in casa propria, e vince il
suo: cambiare il nome spedito farebbe smettere di vincere, **in silenzio**, un
flusso che qualcuno ha già scritto.
**Cosa ne discende.** Il controllo `identifiers_are_in_english` non guarda i
`.flow.json` e non guarderà mai gli `id`: non è una dimenticanza da completare.
Chi in futuro lo estende ai dati sta rompendo questa decisione, non applicandola.
Resta l'asimmetria dichiarata: `flows/dispatch-the-work.flow.json` ha l'id in
italiano e i passi in inglese, e va bene così — sono tutti e due dati.

### Il tetto di spesa è del flusso, e la larghezza del fronte ne discende
**31/08/2026.** Un flusso può dichiarare `spend_cap_micros`: quanto una sua
corsa può spendere. Prima di aprire ogni fronte l'esecutore chiede al deposito
quanto è stato speso; se il tetto è raggiunto la corsa si ferma con una parola
sua — `cap_reached`, non `failed` — e dice quali passi non sono partiti.
**Perché prima di aprire e non dentro l'azione**: un passo che scopre a metà di
aver sforato ha già pagato. L'unico istante in cui fermarsi costa zero è prima
di aprire il fronte.
**Perché una parola sua e non un guasto**: un flusso notturno che tocca il
proprio tetto ogni notte apparirebbe rotto ogni notte, e chi guarda smetterebbe
di guardare.
**Che cosa il tetto non promette**: si misura sui costi che i motori
dichiarano. Codex dichiara il totale dei token e non i due lati, quindi la sua
riga resta senza costo e non entra nel conto. Il tetto è una garanzia **su ciò
che si sa**, e la corsa fermata scrive quante chiamate erano fuori — perché chi
sta per alzarlo e rilanciare deve saperlo prima, non dopo.
**Il predefinito è nessun tetto.** `None` non è `Some(0)`: il primo è «nessuno
ha messo un limite», il secondo è «questo flusso non deve spendere niente». Un
tetto che comparisse da sé fermerebbe corse che nessuno ha chiesto di fermare, e
lo farebbe la notte.

### Un tetto non si tara su meno di tre corse costate, e oggi non se ne tara nessuno
**31/08/2026.** `sailor flow cap <nome>` suggerisce un valore **solo** con
almeno tre corse di quel flusso che abbiano speso qualcosa di noto. Sotto la
soglia rifiuta di suggerire e dice cosa c'è. Il suggerimento, quando c'è, è
*peggiore corsa osservata + chiamata più cara osservata*.

**Perché tre, e perché il secondo addendo.** Con due campioni il massimo e il
minimo sono gli unici due valori: chiamare «peggiore osservata» il maggiore di
due è un dato inventato con la faccia di una misura, ed è il guasto 22 in
un'altra forma. Il secondo addendo non è prudenza: il controllo scatta *prima*
di aprire un fronte, mai dentro una chiamata, quindi la corsa si ferma con la
grana di una chiamata e non di un micro — la somma dice «la corsa più cara che
ho visto, più la grana con cui so fermarmi».

**E oggi nessun flusso raggiunge la soglia. Misurato sul deposito di questa
macchina il 31/08/2026, in sola lettura**: 34 corse, e **6 con un costo diverso
da zero** — `come-lo-risolvono-gli-altri` 2, `esamina-la-repo` 2,
`prova-dei-turni` 1, `sviluppa-sailor` 1. Le altre 28 sono il guasto 22, dove il
costo era la costante zero fino al 30/08. **La proposta scartata era «mediana +
50%»**: su quella colonna la mediana darebbe zero per ogni flusso, cioè un tetto
che ferma ogni corsa prima del primo passo — e lo farebbe di notte, con l'aria
di una taratura su molti campioni. Chi vorrà tarare i tetti lo farà quando i
campioni ci saranno, non prima.

**Il tetto non si collega a `native_spend_cap`**, la capacità dichiarata dal
solo claude-code: portata diversa (una corsa contro un'invocazione), parola
diversa per fermarsi, e un motore su quattro ce l'ha. Farne dipendere il freno
significherebbe che il tetto vale o non vale a seconda di chi risponde.

**E la cifra si chiama «costo equivalente» dovunque si mostri.** Resta in micro
di valuta, ma «spesi 5,00 su un tetto di 5,00» fa credere che sia stata fermata
una fattura: con una riga di comando locale si paga un abbonamento, e quello che
si consuma è quota. `sailor flow cost` lo diceva già; `why_it_stopped` no, e lo
stesso numero si leggeva in due modi a seconda del comando che lo mostrava.

### Le capacità di uno strumento sono un dato, e l'assenza si scrive
**31/08/2026.** Un descrittore dichiara, oltre a `detect`, `version`, `ask` e
`usage`, un blocco **`capabilities`**: che cosa quel motore sa fare oltre a
rispondere — riprendere una sessione, ramificarla, imporre una forma alla
risposta, isolarsi dalla configurazione di chi lo ospita, ricevere una dotazione,
tenere un tetto di spesa suo, scegliere il modello, ripiegare su un altro. È una
mappa da nome a dichiarazione: **il codice non conosce nessun nome di capacità**,
quindi aggiungerne una a uno strumento nuovo è scrivere un file JSON in
`~/.config/sailor/tools.d/`, mai ricompilare. Vincolo permanente «programmiamo a
codice solo ciò che tocca il mondo», applicato a un vocabolario.

**Scrivere `false` non è la stessa cosa che tacere, ed è il punto di tutto il
blocco.** `false` dice «qualcuno ha guardato e non c'è»; l'assenza della riga
dice «nessuno ha guardato». Un blocco che permettesse solo di elencare ciò che
c'è farebbe passare per misurata ogni omissione — ed è la stessa distinzione che
il rilevamento tiene fra «non c'è» e «non ho potuto guardare». Per questo i
quattro motori spediti rispondono su **tutte e nove** le capacità del
vocabolario, e una prova lo pretende.

**Chi non ce l'ha continua a funzionare, e il ripiego resta quello di oggi.** Una
capacità assente non è un errore: chi non sa imporre una forma alla risposta se
la fa chiedere nel prompt con `answer_shape` e paga più token. Vincolo permanente
«indipendenza dal modello». Un passo dichiara ciò che gli serve con
`needs_capabilities`, e `sailor flow check` **avvisa** nominando passo, motore e
capacità — non fallisce: un flusso scritto per un motore più capace non è rotto,
è un flusso che qui costa di più, ed è la stessa ragione per cui uno strumento
non installato è un avviso e un nome inesistente è un errore.

**Cosa questo non fa, e non deve sembrare che faccia.** Le azioni non usano
ancora nessuna capacità: il vocabolario e il controllo che lo interroga esistono,
l'uso no. `needs_capabilities` è dichiarato in `EngineSpec` perché un passo
onesto non venga accusato di un refuso, e non è letto a esecuzione.

### `flow check` esegue: monta ogni riga di comando e la prova senza la domanda
**31/08/2026.** Dal guasto 1 in poi la cura scritta accanto a ogni guasto sulle
righe di comando è la stessa — «una prova che esegue davvero ogni riga di comando
prima che finisca in un flusso» — ed è rimasta scoperta per tre giorni, perché
eseguire sembrava voler dire spendere. Non vuol dire. **Un motore invocato con la
riga vera e senza la domanda non chiama nessun fornitore, e percorre lo stesso
parsing di argomenti di una chiamata vera**: se la riga è malformata lo dice lì,
gratis. Da oggi `sailor flow check` monta la riga di ogni motore di ogni catena,
la esegue senza la domanda, e riporta come sta messa.

**Il verdetto sta nel testo, mai nel codice d'uscita.** Misurato su questa
macchina: `agy` esce **2** sia quando rifiuta bene («flag needs an argument:
-print») sia quando la riga è quella malformata del guasto 27 («--print took
"--output-format" as its prompt»). Una sonda che giudicasse dall'esito avrebbe
visto i due casi identici e sarebbe passata sopra al guasto 27 — che è
esattamente ciò che è successo. Per questo il descrittore dichiara
`ask.refuses_without_prompt`, **le parole del motore**, come già fa per
`unusable_when`; e per questo `judge_dry_run` non riceve nemmeno il codice
d'uscita, così non c'è modo di usarlo per sbaglio.

**`--help` è la forma innocua sbagliata.** `agy --mode nonsense-value
--not-a-real-flag --help` esce **0**: cortocircuita prima di leggere gli
argomenti, quindi approva un valore invalido e una bandiera inventata. La forma
giusta è montare la riga vera e non dare la domanda.

**Cambia la natura del comando, e va detto.** `resolver.rs` dichiara che
risolvere un nome non deve eseguire niente, e resta vero: è il controllo che
avvia processi, non la risoluzione. `flow check` non è più solo statico — senza
rete, senza denaro, con un tetto di tempo esplicito, perché su questa macchina
`timeout` e `gtimeout` non esistono.

**Acceso in modo predefinito, con `--no-engines` per spegnerlo.** Un controllo
dietro una bandiera è un controllo che nessuno interroga: nessuno avrebbe scritto
`--engines` per cercare un difetto che non sapeva di avere. Spento, il rapporto
**tace** invece di dichiarare sane righe che non ha guardato — stessa regola del
rilevatore assente.

**Cinque esiti, cinque frasi, perché sono cinque riparazioni diverse:** sana;
rotta (con le parole del motore per intero e la riga montata); non provata (tre
motivi distinti: il descrittore tace, il motore non è qui, nessuna risposta); non
montabile (nessun blocco `ask`); non può lavorare adesso. E `unusable_when` si
legge **prima** di `refuses_without_prompt`: un motore esaurito non è un motore
rotto, e letto al contrario manderebbe a correggere un descrittore sano.

**Cosa questo non dice.** Che un motore sia stato **chiamato davvero**: quello lo
sa il deposito, e resta un asse separato. Mescolarli farebbe passare per usato un
motore che nessuna corsa ha mai nominato — che è il guasto 32.

### Il potere di un passo: modello Bazel, in osservazione
**29/08/2026 — Theo.** Un passo dichiara cosa gli serve, e il resto per lui non
esiste. Il controllo entra come **avviso** e diventa barriera solo con un cambio
di configurazione, dopo averlo visto funzionare.
**Perché**: un divieto specifico si aggira, un mondo ristretto no; e la fase di
osservazione toglie la paura che rende queste cose impossibili da introdurre.
**Cosa ne discende**: ogni passo dei flussi esistenti dovrà dichiarare cosa
tocca. Non è gratis. *Non ancora costruito.*

### Il file delle autorizzazioni non esiste
**29/08/2026 — Theo.** L'autocura non ha un gate suo: è un flusso come gli
altri, con i poteri che dichiara.
**Perché**: se il modello Bazel vale per ogni passo, un meccanismo speciale per
l'autocura sarebbe difendere due volte la stessa cosa. Ed è coerente col fatto
che i flussi che usiamo per sviluppare Sailor non si spediscono a nessuno.

### I flussi di sistema stanno dentro il binario
**29/08/2026 — Theo.** Incorporati alla compilazione, non installati come file
accanto al programma. Chi ne vuole uno diverso ne scrive uno con lo stesso nome
in casa propria o nel progetto, e vince il suo.
**Perché**: un flusso spedito come file può mancare, invecchiare o essere
cancellato, e allora il prodotto si comporta diversamente su macchine diverse
senza che si capisca perché. *Fatto: `crates/flow/system/`.*

### Niente briglie sul flusso che sviluppa
**29/08/2026 — Theo.** Il passo che implementa scrive senza chiedere permesso.
**Perché**: il perimetro non è ancora applicato dal motore, e aspettarlo avrebbe
fermato tutto. Chi lancia lo sa. **Attenzione**: in un ciclo questo conta il
doppio — chi lascia girare da solo per ore deve poter vedere cosa fa mentre lo
fa, e da questo giro il testo di un passo esce su stderr mentre il passo gira.

### Le prove rosse rompono il passo
**29/08/2026 — dopo il primo giro fallito.** Nessuna tolleranza sul passo che
esegue le prove nel flusso di sviluppo.
**Perché**: la tolleranza c'era perché il verificatore vedesse l'esito anche
quando fallivano, e così **un lavoro che non compilava ha superato il gate** —
con cinque minuti di verifica spesi su codice che non stava in piedi. Un lavoro
che non compila non ha niente da far giudicare a nessuno.

### I flussi si compongono, non si fondono
**29/08/2026 — Theo.** Ricerca, smistamento, sviluppo e interrogazione del
codice sono le fasi di un ciclo unico, ma restano flussi separati che si
chiamano fra loro.
**Perché**: un flusso di dieci passi che fa tutto non si può usare a metà, e la
ricerca serve anche da sola. **Cosa ne discende**: serve `subflow`, un passo che
esegue un altro flusso. *Non ancora costruito.*

### Il ciclo sta dentro Sailor, non accanto
**29/08/2026.** Un flusso a ronda non è un flusso lungo: è un flusso corto
eseguito molte volte, e chi lo riesegue deve essere Sailor.
**Perché**: uno script che rilancia è stato scritto e cancellato lo stesso
giorno. Sarebbe stato un cerotto fuori dal sistema su un buco dentro il sistema,
e i cerotti restano. **Cosa ne discende**: serve che qualcuno esegua ciò che
`sailor flow due` già calcola. *Non ancora costruito.*

### Il testo non ripete numeri che il sistema sa dare
**29/08/2026.** Dove un fatto è già registrato, il testo ci rimanda invece di
copiarlo.
**Perché**: una copia a mano invecchia da sola. È già successo: un documento
diceva «dieci guasti» mentre il file ne elencava undici, e un verificatore ha
respinto un'intera ricerca per quell'incoerenza — a ragione.

### L'ordine di sblocco è cambiato: prima usare Sailor, poi non servirsi d'altro
**31/08/2026 — Theo.** L'ordine scritto il 29/08 — chiamate, orchestrazione,
ciclo — resta valido come sequenza tecnica, ma **non è più il criterio con cui
si sceglie cosa fare**. Il criterio nuovo è uno solo: *cosa manca perché Theo
possa passare una giornata di lavoro dentro Sailor.* Tre blocchi, in
quest'ordine, e il terzo è la conseguenza dei primi due:

1. **Sailor si sviluppa senza morire mentre lo si usa.** Si deve poter
   aggiustare la macchina di sotto mentre qualcuno ci lavora sopra: niente
   riavvii, niente finestra che sparisce. Oggi è impedito da due guasti aperti —
   il **4** (Sailor non sa quali processi ha avviato, quindi non può né
   spegnerli né riprenderli) e l'**11** (in modalità viva un errore di
   compilazione in un crate qualunque uccide la finestra invece di lasciarla
   all'ultima versione buona).
2. **I terminali.** Un terminale si apre **legato a uno spazio di lavoro** — una
   repo, un progetto — e ciò che l'utente scrive viene **smistato**: se la
   richiesta riguarda un flusso, va al flusso; altrimenti resta un terminale
   normale. Oggi non esiste niente: `desktop/src-tauri` ha quattro file e nessuna
   riga di pseudo-terminale, e la sorgente d'innesco `sailor-terminal` è
   dichiarata nel catalogo come «la forma che avrà, non una misura».
3. **Non servirsi più d'altro**, che non è un lavoro a sé: è ciò che succede
   quando i primi due sono fatti.

**Perché quest'ordine e non quello di prima.** Il vecchio ordine ottimizzava la
correttezza del motore; questo ottimizza il momento in cui il sistema smette di
essere un progetto e diventa lo strumento con cui si lavora. Finché Theo sviluppa
Sailor altrove, ogni difetto di Sailor lo paga qualcun altro — e nessuno dei
suoi guasti viene trovato usandolo, che è l'unico modo in cui i guasti di questo
repo sono stati trovati finora.

**Cosa ne discende, e va detto perché cambia le priorità di chi legge.** Un
lavoro che rende Sailor più corretto ma non più *usabile da dentro* non viene
prima di uno che lo rende usabile. Vale anche per i flussi: scriverne di nuovi
non è nei primi due blocchi, e chi ne scrive uno mentre questi tre sono aperti
sta lavorando fuori dall'ordine.

### L'ordine di sblocco: prima le chiamate, poi l'orchestrazione, poi il ciclo
**29/08/2026 — Theo.** Tre blocchi, in quest'ordine, e ognuno si vede funzionare
prima del successivo:

1. **Le chiamate ai modelli**, profili e fornitori insieme. Comprese le quote
   gratuite che i fornitori dichiarano e che oggi non sfruttiamo, e le righe di
   comando che non abbiamo ancora (DeepSeek, Grok, OpenRouter e le altre).
2. **Orchestrare bene**: mandare il lavoro sul modello giusto per quel lavoro, e
   disegnare flussi che si reggano.
3. **Fortificare i flussi di sviluppo**, farli girare in un ciclo, e sotto una
   catena di smistamento vera che usi la macchina invece di un passo alla volta
   — sapendo se la macchina è occupata da chi ci lavora o è libera.

**Perché quest'ordine**: senza il primo blocco ogni corsa dipende da un solo
abbonamento e si ferma quando finisce, come è successo il 29/08. Senza il
secondo, avere più motori vuol dire solo avere più modi di sprecare. Il terzo è
quello che rende il tutto un sistema che va avanti da solo, e va per ultimo
perché fino ad allora ogni difetto si moltiplica per il numero di corse.

**Dopo questi tre**, il resto è miglioria: si seguono le voci in programma.

### Ogni cosa costruita come flusso ha un flusso che la cura
**29/08/2026 — Theo.** L'autocura e lo sviluppo non sono un progetto a parte:
sono la coppia di flussi che tiene in piedi tutto ciò che teniamo a livello di
flusso.
**Perché**: ciò che non è codice non ha né compilatore né prove che lo
sorveglino. Un flusso rotto resta rotto in silenzio finché qualcuno non lo
lancia. Se i flussi sono il posto dove mettiamo tutto ciò che non tocca il
mondo — ed è il vincolo permanente in cima a questo file — allora la loro
manutenzione dev'essere altrettanto seria di quella del codice, e automatica per
la stessa ragione.

### Una voce può essere deprecata o ridecisa, e non da sola
**29/08/2026 — Theo.** Mentre si sviluppano i flussi, le voci di lavoro
cambiano: alcune non hanno più senso, altre vanno ripensate. **Questo si fa
insieme a chi usa il sistema, non in autonomia.**
**Perché**: una voce che sparisce senza che nessuno lo sappia è indistinguibile
da una voce dimenticata, e la seconda è un guasto. Vale anche al contrario: un
flusso che cancella da solo ciò che gli sembra superato decide al posto di chi
deve decidere — ed è lo stesso motivo per cui la prima regola di scelta è «mai
una voce che aspetta una decisione».
**Cosa ne discende**: quando le voci passeranno nel deposito, lo stato non è
«aperta/chiusa». Serve almeno **deprecata** — non si fa più, e c'è scritto
perché — e **da ridecidere**, che è una voce che aspetta te e che nessun flusso
può prendere. E serve che il passaggio a quegli stati sia registrato con chi
l'ha fatto, come ogni altra cosa nel deposito.

### Il multi-fornitore si costruisce in casa, e non è un proxy
**30/08/2026 — Theo, dopo aver guardato free-claude-code.** Non si integra
`free-claude-code` né nessuno degli altri intermediari (Claude Code Router,
LiteLLM, OmniRoute, 9router). Il pezzo si fa qui.

**Perché, coi numeri che l'hanno deciso.** Quel progetto è 143.000 righe di
Python 3.14 con 157 pacchetti bloccati e un server sempre acceso, da mettere
sotto un workspace Rust che tiene tre dipendenze per scelta scritta nel
`Cargo.toml`. Ha 51.600 stelle e **una persona sola** che lo scrive. E soprattutto:
**il pezzo per cui lo si voleva non c'è dentro.** Il suo catalogo ha
identificativo, URL e nome della variabile d'ambiente — nessuna quota, nessun
limite, niente su cosa il fornitore fa dei dati che riceve. L'«oltre 1,3
miliardi di token gratis al mese» è **una riga di README senza un dato che la
sostenga**. Il pezzo caro è la traduzione dei formati fra fornitori: dodicimila
righe che loro riscrivono due volte e mezza a trimestre, e che diventerebbero
nostre per sempre.

**Cosa si prende comunque, e cosa si rifiuta.** Da rifiutare senza discussione:
due dei loro cinquanta fornitori si presentano come un altro programma — il
client OAuth della CLI di Codex e il suo `User-Agent` — per far passare un
agente sull'abbonamento di qualcun altro. Non è aggirare una quota, è fingere di
essere un altro software, e non entra qui. Da prendere, invece, **un dato che
esiste già ed è sotto MIT**: il catalogo delle fasce gratuite di OmniRoute, che
è l'unico dei quattro a portare quote mensili documentate per fornitore, la
metodologia con cui le ha misurate, e — la cosa che vale di più — un verdetto
sui termini d'uso che marca diciassette fornitori come «da evitare, i loro
termini vietano il passaggio da un intermediario». Si prende il dataset, non il
programma.

**La strada che questo apre, e che costa quasi niente.** Sailor lancia già gli
agenti come sottoprocessi con un ambiente configurabile (`launch.env`), e
esistono endpoint che parlano **nativamente** il protocollo che quelle CLI già
usano: si fa puntare lì una variabile, e non c'è nessuna traduzione da scrivere
né da mantenere. Il lavoro che resta nostro è quello che nessuno ha fatto:
un catalogo dei fornitori che porti **quanto danno gratis**, **a che patto sui
dati**, e **quanto ne resta**. Sta in `crates/models`, che già tiene i modelli.

**Cosa ne discende, e non è ancora costruito**: dove vivono le credenziali (oggi
i profili spostano file e fanno collegamenti simbolici, cioè il segreto sta in
chiaro sul disco); e la dimensione che va messa fin da subito nella regola di
instradamento — **non tutti i lavori possono andare ovunque**, perché su certe
fasce gratuite il patto è che i tuoi dati addestrino il modello, e un flusso che
legge codice privato non ha lo stesso insieme di destinazioni ammesse di uno che
riassume un documento pubblico. Aggiungerla dopo vuol dire aver già mandato
qualcosa nel posto sbagliato.

### Un'azione dichiara la superficie a cui appartiene, e i poteri che pretende

**31/08/2026 — Theo**, dopo il censimento di `dev-stack` (27 script di un altro
progetto, candidati a diventare flussi).

Le superfici sono quattro, e un'azione ne dichiara **una sola**: `sense` legge il
mondo senza toccarlo, `act` lo tocca, `remember` è il deposito interrogabile,
`gate` è dove entra il permesso di una persona. Insieme alla superficie, ogni
azione dichiara **i poteri che pretende** — rete, disco, processi, denaro,
segreti — e le `sense` dichiarano in più **cosa rispondono quando non possono
vedere**.

**Perché non è una tassonomia estetica.** Il censimento cercava «quali script
diventano flussi» e ha trovato un'altra cosa: 15 voci ferme su **cinque poteri
mancanti**, non su quindici nodi. La domanda giusta non è quale nodo manca, è
**quale potere non abbiamo e quale flusso lo dimostra**. E la regola che ne
discende è una sola riga: *se un'orchestrazione richiede codice nuovo, manca un
potere — non manca un flusso.*

**Perché la terza dichiarazione esiste.** Viene dal guasto 12: un comando zittito
dal perimetro rispondeva «vuoto» senza errore, e la sorveglianza ha detto
«nessun flusso in esecuzione» mentre due giravano. Un sensore che confonde zero
con cieco è peggio di un sensore assente, perché chi sta a valle si fida.

**Cosa la rende rossa** — senza questo non sarebbe vincolante, come la regola
sulla lingua prima del 31/08: una prova che scorre il registro delle azioni e
fallisce se una non dichiara superficie e poteri, e se una `sense` non dichiara
la propria risposta da cieca. **Nasce rossa su tutte e nove le azioni di oggi.**

**Il debito, dichiarato.** I sette cantieri aperti il 31/08 (`supervisor`,
`terminal`, `presence`, `mcp`, e gli altri) hanno prodotto crate **prima** di
questo criterio. Se non si adeguano prima di chiudere, la regola nasce con
quattro eccezioni non scritte — che è esattamente come la finestra è arrivata a
offrire otto tipi di passo mentre il motore ne esegue tre.

*Per esteso, con le tre proprietà di un sistema aperto e i numeri del
censimento*: `docs/2026-08-31-le-quattro-superfici.md`.

### Chi non dichiara come si esaurisce non sta in mezzo a una catena

**01/09/2026**, dai guasti 16, 31 e 32 — che erano lo stesso difetto visto da
tre lati.

Un motore che non dichiara `ask.unusable_when` **non può occupare una posizione
di ripiego**: va in fondo alla catena, o non ci va. Non perché il suo descrittore
sia sbagliato — l'elenco vuoto dice «nessuno ha guardato», ed è la verità — ma
perché `says_it_cannot_work` su un elenco vuoto è `false`: il suo esaurirsi passa
per un fallimento qualunque, il passo muore su di lui, e chi sta dietro non parte
mai. Una catena `claude-code → agy → codex` aveva l'aria di due ripieghi e ne
aveva zero: `codex`, che dichiara il proprio 401, **non è mai partito**.

**Cosa la rende rossa**, senza cui non sarebbe vincolante: la regola sta in
`toolbox::Descriptor::cannot_be_a_fallback`, in un posto solo, e la interrogano
`every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted` sui flussi
dell'albero e `sailor flow check` sui flussi di chi lo lancia. È nata rossa su
dodici posizioni in quattro flussi.

**La cosa da ricordare, che vale oltre questo caso.** La regola era scritta da un
giorno, e la prova che la conteneva era `#[ignore]` con una ragione buona:
misurare come `agy` dice di aver finito la quota è impossibile finché non lo si
vede farlo, e inventare quella parola manderebbe un mandato malformato giù per
tutta la catena. Ma *quella era la ragione per non inventare un dato, ed era
diventata la ragione per non avere un controllo* — e una regola ha quasi sempre
**due modi di essere rispettata**. Misurare le parole di chi sta in mezzo, o non
mettere in mezzo chi non le ha. Il secondo non chiede nessun dato che non esista.

**E il controllo non serve solo a trovare il difetto: serve a rendere sicura la
riparazione.** `gemini-cli` dichiarava di saper rispondere a una domanda secca e
non aveva nessuna riga con cui fargliela (guasto 32). Scriverla era considerato
pericoloso, perché un `ask` senza `unusable_when` avrebbe fatto entrare gemini
nelle catene senza ripiego — un quarto guasto 31 creato per chiudere il 32.
Appena la regola sulla posizione esiste, quel pericolo non esiste più, e la riga
si è potuta scrivere **misurata**: `gemini --prompt` senza la domanda esce 1 e
dice «Not enough arguments following: prompt», gratis e senza chiamare nessun
fornitore. Il suo `usage` e le sue parole di esaurimento restano non misurati, e
quindi non scritti.

**Cosa questo non concede.** Che un `agy` esaurito **in fondo** a una catena
dica di essere esaurito: non lo dice, muore col proprio messaggio d'errore. Non
si perde nessun ripiego — dietro di lui non c'è nessuno — e la differenza si
legge nel motivo del guasto, non nel comportamento. La misura mancante resta
scritta nel descrittore di `agy`, dove la troverà chi la farà.

### La misura si cerca prima di scegliere la strada che la evita

**01/09/2026, deciso da Theo**, poche ore dopo la voce qui sopra e contro la sua
seconda metà.

La voce qui sopra dice il vero e resta: una regola ha due modi di essere
rispettata, e prendere il secondo — non mettere in mezzo chi non dichiara — non
chiede nessun dato che non esista. Ma quel giorno il dato **si poteva
misurare**, e nessuno ci aveva provato. «Non inventare un dato» era diventato
«non cercarlo»: sono due cose diverse, ed è la stessa forma dell'errore che
quella voce racconta, ripetuta un gradino più in là.

**La regola, adesso.** Davanti a un dato mancante si dichiara **che cosa si è
provato**. Le strade, in ordine di costo: la documentazione e l'aiuto del
comando, compresi i sottocomandi annidati — `codex exec fork` non compariva
nell'aiuto di primo livello, quindi l'aiuto si guarda in profondità; una
invocazione vera che provochi la condizione senza spendere; il comportamento con
una casa vuota o senza credenziali. Se la misura viene, si scrive **esattamente
come è uscita**. Se non viene, si scrive **cosa si è provato e cosa ha
risposto**: un'assenza misurata vale più di una supposizione, e vale molto più
di un'assenza non cercata, che non si distingue dalla pigrizia.

`agy` è stato misurato così: `HOME` su una cartella vuota e la riga che Sailor
monta davvero. Dice di non poter lavorare con parole sue, quelle parole sono nel
descrittore, e la sua posizione in catena non è più una conseguenza del suo
silenzio.

### Dove sta un motore in catena si decide su una misura, non su un'abitudine

**01/09/2026, deciso da Theo.**

Dodici posizioni in quattro flussi dichiaravano lo stesso ordine e **nessun
documento diceva perché**. Un ordine che nessuno ha deciso non è una scelta: è
un'abitudine, e si difende da sola perché nessuno sa cosa smentirebbe.

Ciò su cui si decide, e che va misurato prima di riordinare: **quanto costa**
ciascun motore (il listino, `~/.config/sailor/pricing.json`), **quanta quota
gli resta** (`sailor remaining`), **se è autenticato davvero** — sulla casa del
profilo attivo, non su quella di chi ha aperto il terminale — e **quante volte
ha risposto**, dal deposito. Un ordine che non si appoggia ad almeno uno di
questi quattro non si applica: si scrive in `docs/da-fare.md` come proposta.

**E il primo esito di questa regola è che due dei quattro numeri non ci sono.**
Il listino conosce un fornitore su tre, e `sailor remaining` risponde per un
motore solo: un ordine per costo o per quota residua **oggi non è calcolabile**,
e chi lo proponesse starebbe indovinando. L'unica cosa che la misura impone oggi
è che un motore misurato **non autenticato** non stia davanti a uno misurato
autenticato. I numeri e le proposte che ne restano stanno in `docs/da-fare.md`.

**Il limite di questa decisione, dichiarato.** Le credenziali sono uno stato che
cambia; l'ordine scritto in un flusso no. Scrivere il primo dentro il secondo è
una cura che invecchia, e la forma giusta — che Sailor misuri e scavalchi a
esecuzione — è una proposta, non una decisione.

### Un totale con dentro un'incognita si mostra come pavimento, mai come cifra

**01/09/2026**, dal guasto 37.

Un totale di costo che contiene anche **una sola** chiamata senza `cost_micros`
non si stampa come numero. Si legge da `Spend::reading()`, che restituisce uno
dei tre casi — niente, il totale, **almeno** questo — e chi mostra scrive la
frase che ne discende: «almeno 1,6674, e il vero è più alto: 3 chiamate su 4 non
sono misurate». Senza nemmeno una misura si dice **sconosciuto**, non «almeno
0,0000»: quest'ultimo è vero, non dice niente, e si legge come una spesa piccola.

**Perché la nota accanto non bastava, ed è la parte da ricordare.** La nota
c'era. `sailor flow cost` stampava già «parziale: 3 chiamate senza costo noto»,
una riga sotto il numero, e la corsa consegnata dell'A/B del 31/08 è stata letta
come **1,6674 $** mentre ne era costati **7,2080** — 4,3 volte. *Chi legge un
totale legge il numero.* Un qualificatore che non occupa il posto della cifra non
qualifica niente. Vale per ogni cifra che Sailor mostra, non solo per questa.

**E la regola sta in un posto solo.** `Spend` distingueva i tre casi dal giorno
in cui è nato: la distinzione era giusta nel motore e non arrivava a chi legge,
perché l'unico modo di interrogarla era un booleano. Chi rifà il confronto nel
proprio `format!` crea una seconda regola che diverge — e a divergere sarebbe
quella che una persona legge, cioè la sola che nessun tipo controlla.

### La quota di una persona non è il costo di una corsa, e non stanno insieme

**01/09/2026**, dalla seconda metà del guasto 37.

Sailor sa leggere quanta quota una persona ha già consumato: `models::remaining`
interroga il canale OAuth di Claude Code — solo lettura, nessun costo — e ne
ricava `Remaining { engine, unit, used_fraction, resets_at, observed_at }`. È la
prima cosa in tutto il sistema che **misura** un consumo invece di chiederlo a
chi lavora.

**Non sostituisce il costo di un passo, e le due non vanno nello stesso
riquadro.** Quella quota conta *tutte* le sessioni di quella persona: la corsa di
Sailor, il terminale accanto, il lavoro di ieri che ricade nella stessa finestra
di sette giorni. Fra due istanti se ne ricava quanta quota è passata, mai quanta
ne ha consumata una corsa — non c'è modo di sapere chi altro scriveva in mezzo.
Un numero preso da lì e scritto accanto a un passo sarebbe una misura con la
faccia giusta e il significato sbagliato, cioè il modo in cui il guasto 37 è
nato, non la sua cura. Per questo sta in `sailor remaining` e non in `flow cost`:
due numeri nello stesso rapporto si sottraggono.

**Il consumo autodichiarato resta, marcato.** `sailor step close --turns` non si
tocca: la dichiarazione di un agente è un dato che vale, purché non si confonda
con un conto. Resta scritta con `cost_micros` a `None`, così il totale che la
contiene si legge come pavimento invece che come somma.

**È dichiarata come capacità dello strumento, non cablata.** Il descrittore di
`claude-code` porta `read_remaining_quota`; `codex` lo porta `false` — provato il
01/09/2026 e non riuscito, con scritto nella sua nota fin dove si era arrivati,
che è diverso da impossibile. Chi non ce l'ha continua a funzionare senza sapere
quanta quota gli resta, che è il ripiego di sempre. Vincolo permanente
«indipendenza dal modello».

## Raccomandato, non ancora deciso

- **La soglia di un flusso che accompagna va sul prezzo, non sulla qualità.**
  Misurato: il degrado della qualità non è osservabile (21 sessioni su 44, una
  moneta); il prezzo di continuare cresce del 34% ed è monotono (37 su 45).
  Aspetta una decisione di Theo.

- ~~**Il terzo blocco ha un antefatto che non è stato ancora fatto.**~~ Fatto il
  30/08/2026: il fronte parte insieme. Due passi indipendenti da sei secondi ne
  impiegano 6,07 invece di 12,07; tre ne impiegano 6,05 invece di 18,14.
  «Sfruttare la macchina» ora ha dove appoggiarsi.

  ~~**Resta una decisione tua**: quanti passi per ondata.~~ **Sciolta il
  31/08/2026, e non con una scelta: con un'aritmetica.** Quattro non è più il
  numero, è il soffitto. Sotto un tetto di spesa la larghezza del fronte la
  calcola `how_many_fit` dal residuo diviso la chiamata più cara vista in quella
  corsa. Il motivo per cui non poteva restare una costante: **un tetto non si
  rispetta con un fronte largo** — quattro chiamate partono nello stesso istante,
  nessuna sa delle altre, e quando la prima registra il proprio costo le altre
  tre hanno già speso. Lo sforamento peggiore non è di una chiamata, è di quante
  ne sono in volo. Senza nessun costo osservato non si stringe: restituire 1 «per
  prudenza» renderebbe seriale ogni corsa con un tetto, per sempre, sulla base di
  un numero che non esiste.
