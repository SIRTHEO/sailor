# Le cose da fare

**Questo file è provvisorio, e la sua sparizione è l'obiettivo.** Esiste perché
oggi le cose da fare stanno in quattro documenti da 1.334 righe complessive, nei
prompt di chi lavora, e nella testa di chi sta leggendo — cioè in posti che
scadono. Il giorno che Sailor ha la sua sezione delle voci sospese, questo file
si cancella e non si rimpiange.

**Non ripete numeri che il sistema sa dare**: dove un fatto è già registrato, qui
c'è il rimando, non la copia. Una copia a mano invecchia da sola, e lo sappiamo
perché è già successo.

Aggiornato il 29/08/2026, dopo la prima corsa in cui Sailor ha sviluppato un pezzo di se stesso.

## Aspettano una decisione di Theo

Nessuna di queste è bloccata da lavoro: sono bloccate da una scelta.

1. **La soglia di un flusso che accompagna: prezzo o qualità.** Misurato: il
   degrado della qualità non è osservabile (una moneta, 21 sessioni su 44), il
   prezzo di continuare sì (+34%, monotono su 37 sessioni su 45). Raccomandato il
   prezzo. *Per esteso*: `docs/2026-08-28-il-flusso-che-accompagna.md`.
2. **Prima l'orologio o prima la staffetta.** Ciò che calcola quando un flusso è
   dovuto esiste già; nessuno esegue ciò che calcola. *29/08: questa voce è
   diventata più urgente.* Senza di essa un flusso a ronda non è ripetibile da
   dentro Sailor, e il rimedio ovvio — uno script che rilancia — è stato scritto
   e poi cancellato lo stesso giorno: sarebbe stato un cerotto fuori dal sistema
   su un buco dentro il sistema, e i cerotti restano.
3. **Se rimettere le istruzioni globali di Claude Code** (`~/.claude/CLAUDE.md`,
   cancellate nella pulizia del 28/08, presenti nell'archivio). Finché mancano,
   ogni sessione nuova parte senza sapere niente di questo progetto.
4. **L'ambiente Python del plugin sicurezza**, 240 MB, si ricrea da solo.
5. **Il nome del marcatore di progetto, e cosa può contenere `checks`.** Il
   31/08 il guasto 25 ha portato dentro `sailor.json`: un file che dichiara la
   radice, così un flusso non se la scrive più addosso. Il meccanismo c'è ed è
   provato; **due scelte non le ha prese nessuno** e finché mancano il terzo
   strato delle regole non si costruisce. (a) `sailor.json` è il nome giusto, o
   deve stare sotto `.sailor/`? Cambiarlo dopo che qualcuno l'ha scritto è una
   migrazione. (b) `checks` è oggi una mappa `nome → comando`: se Sailor un
   giorno la **esegue**, un file di progetto diventa capace di far girare un
   comando qualunque su chi apre quel progetto. Oggi nessuno la legge, e nasce
   **vuota** apposta — `workspace init` non indovina `cargo test`, perché
   indovinare una verifica è deciderla al posto di chi lavora lì.
   Il terreno è pronto: `Declaration` ha già `rules`, `checks` ed `equipment`, e
   tiene i campi che non conosce invece di rifiutare il file (guasto 8).

### Decise il 29/08, da costruire

- **Il potere di un passo: modello Bazel, introdotto in osservazione.** Un passo
  dichiara cosa gli serve e il resto per lui non esiste; il controllo parte come
  avviso e diventa barriera con un cambio di configurazione. **Costa**: i flussi
  esistenti vanno riscritti per dichiarare cosa toccano. *Non ancora iniziato.*
- **I flussi di sistema si spediscono dentro il binario.** Già fatto: stanno in
  `crates/flow/system/` e sono incorporati alla compilazione.
- **Il file delle autorizzazioni non esiste**, e la decisione è di Theo: se il
  modello Bazel vale per ogni passo, l'autocura non ha bisogno di un gate suo —
  è un flusso come gli altri, con i poteri che dichiara. Un pezzo in meno.

## Chieste da Theo, non ancora iniziate

- **I processi non muoiono quando si chiude la finestra o la riga di comando**, e
  si riprendono da soli. Lo stesso problema visto dall'altro lato: oggi Sailor
  avvia processi e **non sa quali ha avviato**, quindi non può né spegnerli né
  riprenderli (è il guasto 4).
- **Profili e account multipli**, per qualunque strumento a riga di comando.
- **La repo contiene solo i flussi che si spediscono.** Oggi contiene anche i
  nostri di lavoro e di prova. La parte che conta non è spostarli: è il controllo
  che impedisce che ci tornino.
- **I flussi personali si condividono**: in locale o su una repo versionata che
  si crea da sola; se pubblica altri li importano, se privata no. Il pezzo
  difficile è **rifiutare di pubblicare un flusso che contiene un segreto**:
  senza quel controllo la condivisione è una fuga di dati con l'aspetto di una
  funzione.
- **Una sezione delle voci sospese dentro Sailor**, che rende questo file
  inutile.
- **Ricerca su tutto ciò che il sistema conserva, anche per significato**, dalla
  finestra e da qualunque agente. Come nodo di sistema, non come funzione della
  finestra.
- **Spazi di lavoro come pagine**: una pagina per i flussi di sistema, una per i
  generici, una per ogni spazio di lavoro, coi nodi collegabili fra pagine.

## Difetti noti e non riparati

I guasti, con come si sono visti e cosa li impedirebbe, stanno in
`docs/guasti-incontrati.md`, che porta anche **quanti sono e quanti aperti**.
(Qui il numero c'era, scritto a mano, ed era già scaduto: diceva «dieci aperti
su trenta» mentre la tabella ne portava altri due. La prova nata il 31/08/2026 —
`cargo test -p sailor --test the_fault_table_holds_together` — confronta la
prosa **dentro quel file** con la tabella da cui viene, e questa copia non la
vedeva nessuno. Un numero che il sistema sa dare non si ricopia: ci si rimanda,
ed è la decisione del 29/08.) Qui solo quelli che non sono guasti ma limiti
dichiarati:

- ~~**I passi girano in fila, non insieme.**~~ Riparato il 30/08/2026: il fronte
  parte insieme, con un tetto dichiarato di quattro passi per ondata. Misurato:
  due passi da sei secondi in 6,07 secondi, tre in 6,05. ~~Resta da decidere se
  il tetto debba diventare una conseguenza di un tetto di spesa invece di una
  costante.~~ Sciolto il 31/08/2026: quattro è il **soffitto**, e sotto un tetto
  di spesa la larghezza la calcola `how_many_fit` dal residuo diviso la chiamata
  più cara vista in quella corsa.
- **La cassetta dei passi offre otto tipi, il motore ne esegue tre.** Chi usa uno
  degli altri cinque costruisce un flusso che non parte. La cura è la stessa
  degli strumenti: chiedere al motore, non tenere una lista.
- **Un passo che esegue un comando non conserva il proprio testo**: di lui resta
  solo l'esito.
- **`sample.ts` contiene dati finti** scritti «finché la finestra non legge dal
  motore», che ormai legge.
- **Il crate dei ganci di Claude Code serve un mondo che stiamo smontando**, e
  tre delle sue prove leggono la configurazione della macchina di chi le esegue.
  *29/08: è diventato bloccante e poi è stato aggirato.* Quelle tre prove sono
  rosse dal 28/08 a codice invariato, e hanno bocciato un lavoro del flusso di
  sviluppo che ne passava 692 su 695. Il gate ora esclude quel crate — che è un
  debito, non una cura: **finché resta, una regressione vera lì dentro non
  ferma nessuno**. O le prove smettono di leggere la macchina, o il crate se ne
  va con il mondo che serve.
- **In modalità viva, un errore di compilazione in un crate qualunque uccide la
  finestra** invece di lasciarla all'ultima versione buona.
- **Un motore che dice di non poter lavorare ed esce ZERO non fa scattare
  nessun ripiego**, e la catena non scala anche con tutti i descrittori a posto.
  Trovato il 01/09/2026 da un giudice che verificava la chiusura del guasto 31,
  cioè **dal lato da cui quella chiusura non guardava**.
  `says_it_cannot_work` è interrogato solo dentro il ramo `ExitError`
  (`crates/actions/src/lib.rs:2218`): nel ramo `Ok` la risposta è presa per
  buona, si ritorna `Asked::Answered`, e la riga del deposito nasce con
  `error_type: None`. Non è ipotetico su questa macchina — è il guasto 39:
  `CODEX_HOME=<cartella vuota> codex exec < /dev/null` risponde «No prompt
  provided via stdin» **ed esce zero**. Con un `answer_shape` dichiarato — ce
  l'hanno tutti i passi di questi flussi — il passo muore poi su un errore di
  forma, cioè **sul sintomo sbagliato, tre gradini più in là**.
  La sonda a secco la distinzione ce l'ha già (`judge_dry_run`, `lib.rs:818`,
  applicato a `Ok` *e* a `ExitError`): il controllo statico e la corsa vera
  divergono, ed è la forma del guasto 39 su un altro campo. Nessuna prova lo
  vede: quelle ermetiche fanno uscire il motore esaurito **sempre** in errore.
  **Va portato nella tabella dei guasti alla prossima fusione** — sta qui e non
  là solo perché due rami stanno numerando righe nuove nello stesso momento, e
  un numero preso due volte è un conflitto che nessuno vede.
- **Un passo che scrive `"tool": "agy"` come stringa invece che come elenco
  esce da ogni controllo sulle catene**: `engines_in_chains()` legge solo
  `as_array()` e lo salta, `fallbacks_into` gli lascia zero motori da guardare.
  Succede oggi in `smista-il-lavoro.flow.json`, passo `engine_b`, dove un `agy`
  esaurito si registra `exit_error` invece di `exhausted` — la distinzione che
  il guasto 14 ha pagato per avere.

## Risposte già trovate, da mettere in pratica

Il flusso `come-lo-risolvono-gli-altri` ha coperto sette guasti su undici con
pratiche collaudate altrove. Le risposte complete stanno nel deposito, nella
corsa di quel flusso; qui l'indice:

| guasto | dove è già risolto |
|---|---|
| 1, 3 — strumenti e argomenti sbagliati scoperti solo eseguendo | actionlint: validare nome e argomenti contro l'auto-descrizione dello strumento |
| 2 — passo fallito registrato come riuscito | HyProv: strato di provenienza separato dal risultato grezzo |
| 4 — il processo orfano capitato due volte | Sillito & Kutomi: il seguito di un guasto è un controllo, non un compito |
| 5 — prove che leggono la macchina | Bazel: ambiente chiuso, dichiarato |
| 7 — la descrizione dice il falso sul codice | DocPrism: contraddizione locale con filtro deterministico |
| 8 — un campo ignoto scarta il componente | n8n: la versione del costrutto è scritta nell'artefatto |
| 9 — l'elemento invisibile | Playwright: lo schermo di riferimento nasce dal primo giro rosso |
| il caso di prova | Temporal: la storia della corsa rotta si scarica e si rigioca |
| introdurre un controllo senza rompere | OPA Gatekeeper: `warn` prima di `deny`, promozione come configurazione |

### I marcatori di prompt OSC 133, per togliere il compromesso del terminale

**01/09/2026, dal cantiere del terminale nella finestra.** La metà React che è
stata costruita smista una riga solo in un modo che costa: mentre la finestra
tiene la riga, la `readline` della shell non ce l'ha, quindi **saltano la
cronologia, le frecce e il Tab** — che dopo le lettere è il tasto più premuto di
tutti — e niente accorge la finestra che lì dentro è appena partito un programma
a schermo intero, cioè `claude` dentro Sailor: **esattamente il caso per cui il
cantiere esiste.** Per questo un terminale nasce in modalità diretta e comporre
è una scelta esplicita: il compromesso è stato *scelto*, non risolto.

**Chi l'ha già risolto, e come.** VS Code e Warp usano i **marcatori di prompt
OSC 133**: la shell emette una sequenza che dice «qui comincia il prompt», «qui
comincia ciò che l'utente scrive», «qui il comando parte». La finestra allora
non tiene niente e non disegna nessun eco — cronologia, completamento e frecce
restano alla `readline`, dove sono sempre state — e all'Invio **legge la riga
dal buffer dell'emulatore**, che sa esattamente dove comincia. La manda a
`terminal_submit`: se torna `{kind: "command"}` la esegue la shell come sempre,
e se torna `{kind: "flow"}` la finestra manda un Ctrl-U e la riga non parte.

**Perché questo toglie il prezzo invece di spostarlo.** Nessuna riga scritta due
volte — la shell ha sempre avuto i caratteri, quindi non c'è niente da
riscrivere; nessun modo da scegliere, perché fuori da un prompt i marcatori non
ci sono e il terminale è diretto **da sé**; e il caso del programma a schermo
intero si risolve senza doverlo indovinare. Il costo è che la shell va
configurata per emetterli, ed è un lavoro che Sailor già sa fare: legge e migra
la configurazione delle righe di comando.

*Non ancora costruito, e da fare prima che qualcuno si abitui al compromesso.*

## Nato dalla serata del 29/08

- **Unire i quattro flussi.** `smista-il-lavoro`, il flusso di sviluppo, la
  ricerca e le future chiamate a SocratiCode sono le fasi di un ciclo unico.
  Deciso di **non** fonderli in un file: si compongono, e serve **`subflow`** —
  un passo che esegue un altro flusso. È uno dei cinque tipi che la finestra
  offre da sempre e che il motore non ha mai avuto: costruirlo sblocca anche gli
  altri quattro.
- **Il flusso di sviluppo deve smettere di essere «di Sailor».** Vale per
  qualunque progetto, quindi cambia nome. Conseguenza da affrontare: **non può
  sapere come si provano le cose nel progetto in cui gira** — oggi dice
  `cargo test`, che vale per Rust e per nient'altro. Serve che il progetto lo
  dichiari, come i descrittori dichiarano gli strumenti.
- **Il costo del passaggio di contesto.** Misurato su una corsa vera: il 96,2%
  di ciò che entra nei prompt è contesto ereditato, il 3,8% sono le istruzioni.
  Ricerca in corso. Le tre piste già viste: mettere il contesto comune in testa
  invece che in coda (la cache dei prefissi paga un decimo, e oggi l'ordine la
  rende inutile); passare riferimenti invece di contenuti (ora possibile, l'anello
  è chiuso); dichiarare «so riprendere una sessione» come capacità di uno
  strumento, così chi ce l'ha risparmia e chi no funziona lo stesso.
- **La fonte di un flusso non deve cambiare mentre gira.** Il 29/08 il file dei
  guasti è stato modificato durante una corsa che lo citava: l'analisi parlava di
  dieci guasti, il file ne aveva undici, e il verificatore ha respinto per
  incoerenza. Aveva ragione, e nessun altro se n'era accorto.

## Il costo di un flusso: nove lavori con il punto d'innesto

Vengono dalla ricerca del 29/08 e **sono lavori, non un indice**: fino a quel
giorno stavano in una sezione chiamata «risposte già trovate», che il flusso di
sviluppo leggeva e non poteva scegliere — perché sceglie fra le cose da fare, e
quelle non lo erano. È lo stesso difetto che la ricerca studiava, applicato a
noi: trovata la risposta, lasciata dove nessuno la esegue.

Il costo misurato: **il 96,2% di ciò che entra nei prompt è contesto ereditato**,
il prompt cresce venti volte dal primo passo all'ultimo, e nessuno di questi
nove è stato ancora fatto.

1. **Il taglio della storia dichiarato: un nodo di taglio nel grafo che nomina con `$from` i soli campi che attraversano, e da lì in poi i passi a valle dipendono da lui e non più dai progenitori**
   *Primo passo*: In `flows/come-lo-risolvono-gli-altri.flow.json` inserire un passo di taglio fra `nostro` e `sintesi` che dipende dai tre progenitori e il cui `with` nomina con `{"$from": ...}` i soli campi che servono a valle; poi rifare la corsa e mettere accanto ai 204.306 byte il nuovo totale. Nessun codice.
   *Riduce*: Riduce l'1, e di riflesso il 3. Vincolo A rispettato: è forma del grafo, nessun motore lo vede. Vincolo B rispettato meglio che in Temporal, perché la corsa resta una sola e non ci sono due tronconi da ricucire: chi guarda vede il nodo di taglio come un passo qualunque con scritto dentro cosa ha lasciato passare. Costo vero, da non addolcire: quello che il taglio butta lo sceglie una persona a mano, e se sbaglia il passo a valle lavora peggio senza saperlo. Non va applicato ai passi che giudicano (`sintesi`, `verifica`), dove il 96,2% non è spreco ma il lavoro.

2. **Inversione del predefinito: le `deps` dichiarano l'ordine di esecuzione, i rinvii dichiarano il contenuto. Un passo il cui `with` contiene rinvii riceve solo quelli, non l'uscita intera delle dipendenze**
   *Primo passo*: In `step_input` (crates/flow/src/executor.rs:756-782): se il passo dichiara l'inversione, l'ingresso composto serve solo a risolvere i rinvii e non viaggia oltre. La dichiarazione va sul passo, non nel motore, perché l'inversione cambia il significato dei flussi già scritti. Subito dopo, la difesa: validare i puntatori `$from` contro l'`output_schema` del passo di provenienza dentro `Graph::validate`, accanto a `GraphError::IncompatibleInput` — oggi `crates/flow/` non sa nemmeno che i rinvii esistono.
   *Riduce*: Riduce il 3 direttamente, l'1 di conseguenza, e spinge il 6 nel verso giusto: l'`answer_shape` smette di essere solo un contratto del produttore e diventa la superficie su cui il consumatore punta. Vincolo A rispettato in pieno: la selezione avviene nell'orchestratore prima che il prompt esista. Vincolo B è esattamente ciò che questa pratica dà — il rinvio è scritto nel `.flow.json` e leggibile. Guasto documentato da n8n e trasportabile qui: un `$from` che punta a un campo che il motore quel giorno non ha prodotto oggi si scopre a corsa avviata dentro `reference::look_up`, cioè dopo aver speso la chiamata; senza la validazione al caricamento, questa pratica trasforma un costo in un guasto.

3. **Registrare il prompt reso e i byte visti/scartati, non solo la ricetta**
   *Primo passo*: Far portare a `ActionOutcome`/`Completion` il testo reso e i due contatori — oggi `Completion` li mette a `None` in tutti i rami perché l'azione non ha modo di dichiararli — e riempirli in `ExternalEngineAction` subito dopo `resolve_references`. Il prompt reso va scritto accanto a `input`, non al suo posto: servono entrambi, la ricetta e il testo.
   *Riduce*: Da sola non riduce nessuno dei sei: abilita l'1, il 2, il 5 e il 6 rendendoli misurabili. Vincolo A rispettato: si misura ciò che Sailor produce, non si presuppone niente del motore, e vale identica su uno strumento che non si può osservare. Vincolo B: è il vincolo B — oggi la vista della corsa mostra la ricetta e non il testo spedito, e la distanza fra le due è cresciuta esattamente del 96,2%. Limite: il prompt reso è grande, quindi va deciso subito se si conserva per intero o per digest più lunghezza, e la scelta va scritta dove chi guarda la vede.

4. **Il consumo dichiarato dal motore entra nel registro come per ogni altra chiamata, e il contesto di partenza si misura per sottrazione**
   *Primo passo*: Dare il deposito a `ExternalEngineAction` alla registrazione, come già fa `register_store` in `crates/sailor/src/flow_cmd.rs:453-490, e leggere il consumo secondo il campo dichiarato nel descrittore dello strumento, scrivendo un `ModelCallRecord`. Poi la taratura: una invocazione con il prompt più corto possibile per strumento — quel numero è il fondo fisso su questa macchina, misurato e non stimato.
   *Riduce*: Attacca il 5 direttamente e trasforma il 2 e il 6 da congetture in misure. Vincolo A rispettato solo come capacità dichiarata: quale bandiera produce il conto e quale campo leggere sta nel descrittore, e chi non lo espone ricade sulla misura per differenza; `TokenUsage` è già scritto perché un campo mancante resti `None` e non zero, che è la disciplina che A chiede. Vincolo B rafforzato: il consumo per passo è dato da mostrare accanto al nodo, e la dashboard lo somma già per le corse che quei record ce l'hanno. Limite da dichiarare: sono token contati dal motore su sé stesso, coprono solo ciò che sceglie di riportare, e il rapporto byte/token va rimisurato per strumento invece di essere assunto costante — gran parte del 96,2% è JSON.

5. **Il confine fra il lavoro intero e il ritaglio che scorre: `output_from_path` in `EngineSpec`, cioè l'uscita di un passo è il file che il passo nomina, non il suo stdout**
   *Primo passo*: Aggiungere `output_from_path` a `EngineSpec` (crates/actions/src/lib.rs:687-720) e farlo leggere da `invoke_external_engine` al posto di stdout quando è presente, con la stessa validazione contro l'`answer_shape` che c'è oggi.
   *Riduce*: Spinge il 6 dalla validazione all'estrazione, e a cascata riduce il 3 e l'1. Vincolo A rispettato: il ritaglio è deciso e imposto fuori dal motore. Vincolo B rispettato: il confine è un campo del passo e il lavoro intero resta sul disco della corsa. Costo che Argo documenta e che vale identico qui: `when` valuta `Condition::PointerEquals`/`PointerExists` sull'ingresso tipato, quindi un valore spostato fuori dal canale tipato smette di poter essere letto dalle condizioni del grafo — un passo con un `when` che ci dipende va tenuto fuori dall'estrazione, e questa è una regola da scrivere, non da scoprire in corsa.

6. **Soglia in byte dichiarata nel flusso che decide da sola cosa va in linea e cosa va per riferimento, con l'uscita divisa in indice e corpo e il corpo scritto come file nella cartella della corsa**
   *Primo passo*: Mettere la soglia in `Step.with` — non in una variabile d'ambiente, perché un numero fuori dal file è la sola parte della decisione che chi apre il flusso non vedrebbe — e in `step_input` far sì che sopra soglia si scriva un file nella cartella della corsa e nel prompt entri il percorso più il peso. Taratura sulla corsa vera: 2.805 e 4.283 byte restano in linea, 63.239 e 82.550 sono il caso per cui la soglia esiste.
   *Riduce*: Riduce il 3 e di conseguenza l'1, e spinge il 6 perché l'`answer_shape` dichiara anche cosa è indice e cosa è corpo. Vincolo A rispettato come capacità dichiarata — «sa aprire un file che gli nomino» è una riga nel descrittore, e chi non ce l'ha resta sempre sotto soglia pagando pieno. Vincolo B rispettato solo con la correzione del file: un indice più file in chiaro è più ispezionabile di un prompt monolitico da 82.550 byte. Guasto che si sposta e non sparisce: sotto soglia troppo bassa il costo passa dal prompt al numero di letture che il motore deve fare, e quel costo non compare nel conteggio dei byte. Guasto peggiore e non misurabile oggi: un passo che sceglie di non aprire il corpo che gli serviva sbaglia in silenzio — `EngineResult` porta stdout e stderr e nient'altro, Sailor non vede le chiamate a strumento del motore. Finché è così, questa pratica sta fuori dai passi che giudicano.

7. **La classe del passo dichiarata nel grafo: produttore contro giudice, con le riduzioni spente sui giudici**
   *Primo passo*: Un campo accanto ad `action` in `Step` (crates/flow/src/graph.rs:8-24: con `deny_unknown_fields` è una riga in Rust più il dato nei flussi), e marcare subito `sintesi` e `verifica` come giudici nel flusso misurato — si riconoscono dal testo che portano già («Non riassumere: scegli», «non fidarti del racconto di chi l'ha fatto»).
   *Riduce*: Non riduce da sola nessuno dei sei: delimita dove le pratiche su 1, 2, 3 e 6 sono lecite e impedisce che il risparmio si paghi con decisioni sbagliate che nessuna misura di byte rivelerebbe. Vincolo A rispettato: è un campo della specifica, vale identico su un motore che non si può osservare. Vincolo B rispettato: la classe è leggibile nel `.flow.json`. Il modo in cui fallisce è che nessuno la applichi, e una corsa verde non lo mostra; l'unico controllo che morde è far girare i passi giudicanti nelle due forme su un campione di corse e riportare quanto spesso la scelta diverge. Da adottare insieme al rendiconto che dichiara separatamente quanti byte sono stati lasciati sul tavolo sui giudici: quella cifra è la prova che la disciplina è stata applicata e non aggirata.

8. **Un campo `capabilities` nel descrittore dello strumento, dove le capacità si dichiarano una per una con la loro forma e la loro degradazione**
   *Primo passo*: Aggiungere `capabilities` a `Descriptor` (crates/toolbox/src/descriptor.rs:170-215, `deny_unknown_fields`: una riga in Rust più il dato nel JSON) e popolarlo per le tre voci esistenti, con la regola che una capacità assente non è un errore ma un prezzo pieno.
   *Riduce*: Non riduce da sola nessun costo: è il meccanismo con cui il 3, il 5 e il 2 si possono ridurre su chi può senza rompere chi non può. Vincolo A: è A. Vincolo B rispettato: le capacità sono un file di dati leggibile e non un ramo di codice. Limite da scrivere accanto ai numeri: soglia minima e durata di una cache dei prefissi sono numeri di *un* fornitore, e portarli su un altro motore senza rimisurare è precisamente l'errore che A esiste per impedire — vanno nel descrittore per strumento, mai in Rust.

9. **Cache per hash dell'ingresso: un passo il cui digest coincide con un `Went` precedente esce dal fronte come «riusato» invece di essere rieseguito**
   *Primo passo*: In `decision_from` (crates/flow/src/executor.rs:702-731) riconoscere il riuso, con la chiave estesa all'eseguibile risolto e alla sua versione — `tool` si risolve al momento tramite `toolbox` e `Finding.version` la misura già, quindi senza quel pezzo la chiave vale solo su questa macchina. E prima ancora del salto, il comando che dice **quale** componente della chiave è cambiato: il confronto fra due digest non lo dice, e il modo normale in cui una cache così fallisce è funzionare al 3% senza che nessuno se ne accorga.
   *Riduce*: Riduce l'1 e il 5, e alleggerisce il 4 perché i passi in cache escono dalla fila. Vincolo A rispettato in pieno: tutto fuori dal motore. Vincolo B rispettato a una condizione — che la vista della corsa distingua un passo eseguito da uno riusato, e il record ha già `attempt_relation` per dirlo. Guasto in più rispetto a Nextflow, da dichiarare senza addolcirlo: un motore di intelligenza artificiale non è deterministico, quindi la cache promette «la stessa risposta di prima» e non «la risposta che avresti adesso» — va spenta sui passi che giudicano, e va spenta per dichiarazione, non per buon senso.

## La finestra: dieci lavori con il punto d'innesto

Dalla ricerca del 29/08 sul progetto di una finestra a nodi. Due di questi
avrebbero preso i guasti 9 e quello del campo troncato — i due che solo
un'immagine aveva visto — trasformandoli da aneddoto in controllo.

1. **Dare corrente alla mappa degli stati: costruire `Map<string, StepRun>` dagli eventi del guscio dentro `absorb` in App.tsx, invece di passare `new Map()` a `buildUnifiedLayout` su ogni flusso vero**
   *Primo passo*: In `absorb` (App.tsx ~250) accumulare `step_started` e `step_closed` in uno stato `Map<string, StepRun>` con la stessa identità stabile del `useMemo` di oggi, e passarlo a `buildUnifiedLayout` al posto della mappa vuota; il commento su perché non si scrive `new Map()` in linea resta valido e va conservato.

2. **Uno solo insieme di stati, dichiarato nel motore e derivato dalla finestra; e una prova che oggi è rossa che confronta i due elenchi per uguaglianza, non per inclusione**
   *Primo passo*: Scrivere in Rust la prova che serializza l'elenco degli esiti del motore e lo confronta con l'elenco che la finestra dichiara di saper disegnare, guardandola fallire; poi far salire `Stopped` e `Skipped` fino a `flow.ts` e togliere `OUTCOME_LABEL` da RunConsole facendogli leggere `STATE_LABEL`.

3. **Far salire `origin` fino alla tela e trasformare la corsia in nodo genitore vero (`parentId` + `extent: 'parent'`), raggruppando per origine sopra le corsie per flusso**
   *Primo passo*: Aggiungere `origin` a `FlowEntry` in `flow.ts` e scriverlo nell'intestazione di ogni corsia; poi, in `layout.ts:185`, dare ai passi `parentId` della corsia e posizione relativa, togliendo la somma a mano di `BAND_PAD_X`.

4. **Il confronto `scrollWidth <= clientWidth` su ogni discendente con testo di un nodo reso, con una lista di eccezioni esplicita per i due troncamenti voluti**
   *Primo passo*: Introdurre nel `desktop` un solo comando di prova con browser vero e scrivere l'unica asserzione, applicata al render di un nodo per ciascun tipo, con dati di prova volutamente lunghi; nella lista delle eccezioni entrano `.step-node__tool-why` e `.flow-band__desc`, e quella lista è il posto dove i troncamenti voluti diventano scritti.

5. **Il contrasto calcolato sulle costanti del tema, non fotografato e non delegato ad axe: rapporto fra ogni tinta disegnata e il fondo su cui finisce, con soglia dichiarata**
   *Primo passo*: Scrivere la funzione di rapporto di contrasto e la prova che scorre `STATE_COLOR`, `FLOW_COLORS` e le tinte di `ToolMark` contro i fondi effettivi del tema, con la soglia dichiarata per classe (testo 4.5:1, segni e bordi 3:1), e correggere il bordo `waiting` che oggi fallisce.

6. **Il collegamento fra spazi come passo `subflow` col nome della destinazione scritto sopra sempre, non come arco che attraversa un confine né come filo invisibile finché non lo selezioni**
   *Primo passo*: Registrare l'azione `subflow` nel motore e disegnare il passo come un nodo qualunque con il nome del flusso di destinazione nel corpo; poi la prova sul modello: ogni `subflow` nomina un flusso presente in una delle sorgenti di `flow_places`, e nessun flusso nominato è orfano.

7. **L'innesco con peso visivo che sopravvive al rimpicciolimento, la minimappa che distingue i tipi, e la prova scritta a risoluzione ridotta**
   *Primo passo*: Dare a `MiniMap` un `nodeColor` derivato dal tipo di nodo e all'innesco una barra del titolo piena invece di un bordo; poi asserire sul render a scala ridotta che esistano esattamente N regioni con la tinta d'ingresso, N pari al numero di inneschi, e che nessun'altra classe produca quella tinta.

8. **Legare la riga letta al nodo che l'ha detta: il passo aperto in `RunConsole` seleziona ed evidenzia il proprio nodo sulla tela**
   *Primo passo*: Passare il passo osservato da `RunConsole` alla tela e marcarlo come selezionato; poi l'asserzione: nella modalità di lettura, il nodo a cui il testo si riferisce è visibile e identificato.

9. **Il riuso senza riesecuzione come parola nel piede del nodo con la stessa tinta di «andato», derivata da `attempt_relation`, più la prova sul divieto d'ambito**
   *Primo passo*: Far salire `attempt_relation` in `StepPassage`, mostrarlo in `StepHistory` e aggiungere la parola sul nodo con la tinta di «andato»; poi la prova sul comportamento, non sulla documentazione: una corsa fuori dall'ambito dichiarato esegue i passi invece di riusarli.

10. **Le regole strutturali dentro `sailor flow check`, con l'esito marcato sul nodo che le viola — non un pannello nuovo**
   *Primo passo*: Aggiungere a `sailor flow check` la regola «almeno un innesco» e «nessun passo irraggiungibile», e disegnare il passo che le viola col motivo scritto sopra, riusando lo schema già in piedi di `step-node__tool-why`.

### Cosa la ricerca dichiara scoperto

- Il problema dichiarato all'inizio — una finestra coerente per disciplina e non per costruzione — resta scoperto per intero. Nessuna delle trentasette pratiche impedisce che il prossimo pezzo venga aggiunto fuori disegno: tutte verificano proprietà di pezzi esistenti, nessuna verifica che l'insieme sia stato pensato. Non esiste una prova di «coerenza», e chi guarda continuerà a sentire la differenza.
- Il vincolo B non ha nessuna verifica automatica, in nessuna delle tre ricerche. Il costo di insegnare una convenzione si misura solo con una persona che non ha mai visto Sailor, cronometrata; e `docs/decisioni.md` esige che non sia chi ha costruito. Finché quella persona non esiste, ogni dichiarazione «B: rispettato» in questo documento è una previsione, non una misura.
- Problema 6, tutto ciò che non è contrasto né troncamento: sovrapposizione fra due elementi entrambi visibili (il distintivo del nodo contro l'angolo del riquadro dello spazio), elemento disegnato fuori dal proprio riquadro, ordine di sovrapposizione sbagliato, spaziatura collassata. Le due asserzioni deterministiche adottate non li vedono, e la sola pratica che li vedrebbe è quella rimandata. È la classe che ha già colpito Sailor due volte su undici guasti.
- Problema 1 nella sua sostanza. La prova a scala ridotta dice che l'innesco si distingue, non che chi apre il flusso sappia da dove si comincia a leggere. Che una gerarchia sia capita nessuna delle pratiche trovate lo verifica — l'unica fonte che ci prova (il punteggio di leggibilità) misura grafi di relazione, non flussi orientati.

La prima riga è la più importante: le dieci pratiche risolvono casi puntuali,
**non** il problema da cui la ricerca era partita — una finestra coerente per
costruzione invece che per disciplina. Quello resta.

- **Un motore può esaurire una soglia, non solo rompersi.** Il 29/08 Claude
  Code ha risposto «You've hit your weekly limit · resets 7am» e il flusso ha
  reagito bene — ha rotto il passo col motivo, non ha finto. Ma un limite di
  quota è diverso da un errore: si sa *quando* torna a funzionare, quindi un
  flusso o una ronda potrebbero aspettare fino a quell'ora invece di fallire e
  basta, o instradare su un altro strumento dichiarato per lo stesso compito.
  Oggi `engine_exit_error` non distingue le due cose.

## In corso adesso

- Il flusso di ricerca, sul costo del passaggio di contesto.
- **Un flusso può guardare com'è andata**: fatto il 29/08/2026. L'azione
  `history_ask` risponde a quattro domande nominate — quante volte un passo è
  fallito e con che classe, quali guasti sono i più frequenti, com'è andata
  l'ultima corsa chiusa di un flusso, quanto ci mette di solito un passo — e
  compare fra le «azioni disponibili» di `sailor flow check`. Niente SQL nei
  file di flusso: lo schema resta in `ledger`, e le domande sono chiuse.
  `input` e `output` non escono mai; `said` esce solo su richiesta esplicita,
  dai soli passi rotti di una corsa sola, troncato. Deposito assente, deposito
  vuoto e zero guasti sono tre risposte diverse, e nessuna delle tre rompe il
  passo.
  Restano aperti due limiti che nessun controllo locale mostra — restano verdi:
  - **Le durate sono in secondi interi**, perché l'orologio del motore conta
    secondi: un passo che dura meno di un secondo misura zero, e due zeri non
    si confrontano. La risposta lo dichiara nel campo `unit`, ma dichiararlo
    non lo risolve.
  - **I passi di corse mai registrate in `runs` restano fuori da ogni finestra**:
    il flusso di appartenenza vive solo nell'intestazione della corsa, quindi
    senza intestazione quei passi non si possono attribuire e non si contano.

## Le voci nel deposito, non nei file — misurato il 29/08/2026

Theo: «il ranking dovrebbe essere un flusso ben fatto dal sistema che scrive le
voci su db a quelle che le va a prendere… ma noi scriviamo su db? o ancora tutta
questa cosa non c'è?»

**C'è, ed è già la forma giusta.** Il deposito è a due file: `events.db` tiene la
verità in sola aggiunta — con due innesti che fanno abortire qualunque modifica o
cancellazione — e `state.db` tiene le proiezioni. Il segnalibro della proiezione
è in pari con l'ultimo evento (254 su 254), quindi la macchina funziona.

C'è già una tabella generica `store`, fatta di collezione, chiave, valore, **chi
l'ha scritto e quando**, e tre azioni che i flussi possono usare — scrivere,
leggere, elencare. Una voce di lavoro può viverci **oggi**, senza codice nuovo.

**Ma nessuno la usa.** Numeri veri del 29/08/2026:

| | |
|---|---|
| corse registrate | 16 |
| passi registrati | 61 |
| righe nella tabella `store` | **1**, in una sola collezione |
| chiamate a modello registrate | **1**, e non da un flusso |
| costo totale sulle 16 corse | **0 su tutte** |
| voci d'inventario | 391 |

La sola chiamata registrata viene dal lavoro notturno, non dall'esecutore dei
flussi, e ha ingresso a zero e costo a zero: l'unico numero vero è l'uscita.

**La conseguenza da guardare in faccia.** La tabella `model_calls` ha già le
colonne per token d'ingresso, d'uscita, cache, costo, prezzi e mandato; il
deposito ha già la funzione per scriverla; e la finestra ha già un cruscotto che
somma il costo per modello. Tutto costruito. **Gli unici che lo riempiono sono le
prove e un esempio che semina dati finti.** Nel percorso vero non lo chiama
nessuno. È peggio del non averlo: un cruscotto che mostra numeri finti sembra
funzionare.

E le voci di lavoro oggi stanno in tre file markdown che un modello rilegge da
capo a ogni corsa. Non sono dati: non si confrontano fra due corse, non si
possono ordinare, e non si sa perché la classifica di ieri era diversa.

### Cosa manca perché il ranking diventi un flusso

1. **Le voci nella collezione `store`**, una per riga, con lo stato. Il
   meccanismo c'è; va usato.
2. **Un flusso che scrive le voci** — dai guasti, dalle corse fallite, da ciò che
   un verificatore respinge — e uno che le pesca. Sono due flussi, non uno: chi
   scrive non ordina.
3. **La classifica come dato, non come prosa dentro un prompt.** Oggi le quattro
   regole di scelta vivono nel testo del passo `scegli`. Un modello le riapplica
   a occhio ogni volta.
4. **L'impatto misurato prima di scegliere.** SocratiCode sa già dire cosa
   toccherebbe un cambiamento, e sarebbe il numero che rende la classifica
   confrontabile invece che opinabile. ~~Oggi un flusso non può chiederglielo:
   Sailor *riconosce* i server MCP — il rilevatore ha la famiglia `mcp_server` —
   ma non esiste nessuna azione che ci parli. È l'anello mancante fra i due.~~
   **L'anello c'è dal 31/08/2026**: `mcp_ready` e `mcp_ask` in
   `crates/actions/src/mcp.rs`, con l'esempio in
   `flows/chiedi-all-indice.flow.json`. **Ma il numero che serve qui non si può
   ancora prendere da lì**, e la corsa vera del 31/08 lo mostra: `codebase_impact`
   su `crates/flow/src/graph.rs` ha risposto «Total impacted files: 0 — No
   callers found», sullo stesso file di tre giorni prima. Un impatto che dice
   zero su un crate che 22 file usano non rende una classifica confrontabile: la
   rende sbagliata con sicurezza. Quel numero lo deve dare `cargo`, e l'indice
   serve a orientare la ricerca — è la regola che l'azione si porta dietro nel
   proprio campo `caveat`.
5. **Il consumo scritto davvero**, che è il punto 1 di `docs/profili-e-consumo.md`
   e serve anche qui: senza costo per voce, «cosa conviene fare prima» resta
   un'opinione.
