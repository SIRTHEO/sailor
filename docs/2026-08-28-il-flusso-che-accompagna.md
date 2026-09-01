# Il flusso che accompagna

**28/08/2026 — cantiere ARCHITETTURA.** Nessuna riga di codice toccata: questo
documento serve a decidere, e le decisioni che restano a Theo stanno in fondo,
separate.

---

## In una pagina

**1. La cosa nuova non è un flusso lungo. È un flusso corto eseguito molte
volte.** La prova di concetto che Theo ha già costruito — la staffetta — non era
un processo che restava vivo accanto a una sessione: era un processo che nasceva
ogni 60 secondi, guardava, decideva, agiva una volta e moriva. Tutta la sua
memoria stava su disco. Questo cambia il progetto: non serve un nuovo tipo di
flusso, serve **un orologio** e **un posto dove ricordare**. Sailor ha già il
secondo (il deposito) e non ha il primo.

**2. Il riempimento del contesto si può misurare da fuori. Il degrado no.**
Misurato oggi su 88 sessioni vere e 42.591 punti: contando solo i byte che
scorrono, un osservatore esterno stima il punto di soglia con **errore mediano
del 6,0%**, purché usi un modello affine invece che proporzionale. Il proxy più
ovvio del degrado — l'agente che si ripete — su 44 sessioni e 16.209 turni **non
misura niente**: aumenta in 21 sessioni su 44, cioè è una moneta. Un terzo
segnale invece regge: il **prezzo di continuare** cresce del 34%.

**3. Il confine non è «esegue» contro «decide». È il potere.** Delle 32 avarie
della staffetta catalogate qui, **21 stanno nei fatti e negli effetti, 6 nelle
decisioni**. Trasformare la decisione in un flusso non avrebbe evitato quasi
nessun guasto. Ne avrebbe però reso visibile la famiglia più insidiosa: la
staffetta ha candidato 2.834 volte e agito 31, e **nessuno lo sapeva**, perché
un rinvio non lasciava traccia. Il guadagno del flusso non è la correttezza: è
la visibilità.

**4. Il paradosso del fondo è già risolto una volta, dentro Sailor.** Non serve
inventare la risposta: `crates/toolbox` incorpora i suoi 36 descrittori nel
binario e li lascia sovrascrivere da disco. Un flusso di sistema che manca non
blocca la macchina, perché il binario ne porta una copia di serie. Il fondo ha
esattamente tre assi: **il vocabolario, l'orologio, il freno.** Tutto il resto
può essere un flusso.

---

## 1. Come si chiama la cosa

Il documento di mandato propone **flusso che monta** contro **flusso che
accompagna**. La distinzione è giusta, il nome no: «accompagna» descrive un
sentimento, non un meccanismo, e non dice a chi programma cosa deve cambiare.

Propongo **flusso a corsa** e **flusso a ronda**, e l'asse che li separa non è la
durata:

| | flusso a corsa | flusso a ronda |
|---|---|---|
| **cosa lo fa avanzare** | gli eventi che produce lui stesso | un orologio esterno |
| **dove tiene la memoria** | nel proprio processo | nel deposito |
| **quando finisce** | quando il lavoro finisce | mai; si ferma quando si ferma il soggetto |
| **di chi parla** | del proprio lavoro | di un esecutore che non ha creato |
| **come si prova** | lo esegui una volta | lo **uccidi a metà** e il battito dopo dev'essere identico |

**La riga che conta è la seconda.** Un flusso a corsa può tenere la memoria in
RAM perché quando finisce non gli serve più. Un flusso a ronda non può: deve
ritrovarla al risveglio, e deve ritrovarla anche se in mezzo la macchina si è
addormentata — cosa che questa macchina fa circa **70 volte al giorno**
(`pmset sleep 1`, memoria `la-macchina-dorme-e-i-subagent-muoiono`).

Da qui discende l'unica invariante di progetto che vale la pena scrivere:

> **Un flusso a ronda deve poter essere ucciso in qualunque istante senza che il
> battito successivo si comporti diversamente.**

La staffetta reale la rispettava, e non per eleganza: girava sotto `launchd` con
`StartInterval 60`, **un passo per invocazione**, e ogni cosa che ricordava
stava in `state/sessioni-vive/`, `state/riprendi-da/`, e nei marcatori di
raffreddamento.

**Perché rifiuto «un flusso all'interno di un agente».** È la formula di Theo,
ed è quella che il §4 smonta con le misure: un flusso che vive dentro l'agente
non può azzerarlo, perché l'agente non ha quel potere su se stesso. La ronda sta
**accanto**, non dentro. Il soggetto è dentro; l'osservatore è fuori.

**Nota sul nome.** «Ronda» è già una parola di questa casa
(`guards/ronda_trigger.rs`, «la ronda delle novità»), dove indica un innesco su
`SessionStart`. La sovrapposizione non mi sembra un difetto: in entrambi i casi
vuol dire *un giro periodico che guarda se qualcosa è cambiato*. Se dà fastidio,
l'alternativa è **flusso a battito**.

---

## 2. Domanda 1 — come si misura «il modello sta peggiorando»

Le due grandezze vanno tenute separate, perché una si misura e l'altra no.

### 2.1 Il riempimento del contesto: si misura da fuori, con un modello affine

**Il problema agnostico.** Claude Code scrive un `.jsonl` con dentro il campo
`usage`, e la staffetta lo leggeva: `input_tokens + cache_read_input_tokens +
cache_creation_input_tokens` (`guards/src/handoff.rs:135-180`). Codex non lo
scrive. Gemini nemmeno. Una riga di comando che non esiste ancora, tanto meno.
Se Sailor dipende da quel file, è un prodotto per una CLI sola.

**Cosa Sailor può vedere senza chiedere niente a nessuno.** Sailor lancia il
processo, quindi tiene il tubo: `actions::run_with_timeout` già drena `stdout` e
`stderr` con due thread, e `StepRecord` registra già `bytes_seen` e
`bytes_discarded`. **I byte li conta già.** La domanda è se bastino.

**La misura.** 88 sessioni vere prese a caso da `~/.claude/projects`, 42.591
punti. Per ogni turno ho calcolato i byte che sarebbero passati dal tubo (testo
dell'agente, argomenti degli strumenti, risultati) e li ho confrontati con i
token dichiarati. Reset del contesto (compattazione, `/clear`) trattati come
segmenti separati.

| domanda | risposta misurata |
|---|---|
| i byte crescono come i token, dentro una sessione? | **sì**: r mediano **0,998**; solo 2 sessioni su 88 sotto 0,9 |
| la costante byte→token è la stessa fra sessioni? | **no, ma quasi**: p05 0,55 — mediana 1,07 — p95 1,41 (**2,5×** fra gli estremi) |
| calibrando sul primo quinto e stimando `token = k·byte`, dove cade la soglia? | **sbagliata del −35,6%** (mediana), e **39 sessioni su 42 intervengono troppo presto** |
| e con `token = k·byte + C`? | **errore mediano +2,1%**, assoluto **6,0%**, p90 19,7%; **33 su 42 entro ±15%** |

**Perché il modello affine vince, e perché la costante è interessante.** `C` è il
contesto che costa token senza far passare un byte dal tubo: prologo, regole,
memorie, definizioni degli strumenti. Stimato sui dati: **mediana 60.129 token**
(p10 52.593, p90 74.321). La memoria `staffetta-clear-e-ripresa` misurava
indipendentemente, il 19/08, che dopo un `/clear` «il contesto riparte da 65k, il
13%». Due misure prese in modi diversi a nove giorni di distanza danno 60k e 65k.
Il pendio è `k` ≈ **0,68 token per byte** (p10 0,58, p90 0,80).

**Cosa significa in pratica.** Sailor può sorvegliare una CLI qualunque così:

1. conta i byte che entrano e escono dal processo (li conta già);
2. per i primi turni, se e solo se la CLI espone una misura vera, la usa per
   calibrare `k` e `C`; altrimenti parte dai valori mediani sopra;
3. da lì in poi stima il riempimento dai soli byte, e ricalibra quando può.

**Cosa questa misura NON prova, e va detto.** I byte li ho ricostruiti dai
transcript, non letti da un tubo vero: è una simulazione su dati reali, non una
prova su un involucro. La differenza morde in tre punti — la TUI tronca l'uscita
degli strumenti prima di mostrarla, il prologo non passa mai dal tubo (ed è
esattamente il `C` che il modello stima), e una CLI che ricicla il contesto in
modo diverso avrebbe un `C` diverso. **Il primo esperimento vero è
avvolgere una CLI e confrontare i byte contati con la sua misura dichiarata.**

### 2.2 Il degrado della qualità: il proxy ovvio non funziona

L'ipotesi da provare: se il contesto degrada, l'agente comincia a ripetersi.
Misurata su 60 sessioni, sovrapposizione di 5-grammi fra un turno e i 10
precedenti:

| riempimento (budget 500k) | turni | ripetizione mediana |
|---|---|---|
| 0–20% | 796 | 8,3% |
| 20–40% | 3.938 | 12,0% |
| 40–60% | 4.250 | 11,2% |
| 60–80% | 3.704 | 10,4% |
| 80–100% | 3.521 | 10,0% |

Dentro la stessa sessione, confrontando sotto 200k e sopra 400k su 44 sessioni:
9,8% contro 9,7%, differenza mediana **−0,5 punti**, e **aumenta in 21 sessioni
su 44**. È una moneta.

**Conclusione onesta: da fuori, con questo proxy, il degrado non si vede.** Non
sto dicendo che non esista — la ricerca su cui Theo ha fondato le soglie (Chroma
«Context Rot», RULER, NoLiMa) lo misura con compiti costruiti apposta e una
risposta giusta nota. Sto dicendo che **Sailor non ha una risposta giusta nota**,
e senza quella la qualità non è osservabile dal flusso dei byte.

Questo ha una conseguenza sul progetto, ed è la più importante di tutto il
paragrafo: **non progettare un segnale «qualità». Progetta un segnale «prezzo».**

### 2.3 Il prezzo di continuare: cresce, si misura, ed è agnostico

| riempimento | turni | millisecondi di attesa per token prodotto |
|---|---|---|
| 0–20% | 2.447 | 5,8 |
| 20–40% | 9.615 | 9,2 |
| 40–60% | 9.739 | 11,1 |
| 60–80% | 7.953 | 11,2 |
| 80–100% | 6.956 | 11,8 |

Monotono su cinque fasce, 36.710 turni. Dentro la stessa sessione: 9,0 → 12,0
ms/token, **rapporto mediano 1,34×**, e **rallenta in 37 sessioni su 45**.

Questo segnale non richiede niente a nessuno: un orologio e un contatore di byte.
E dice una cosa che si può difendere davanti a chiunque — *continuare qui costa
un terzo in più che ricominciare di là* — mentre «il modello fa schifo» non si
può difendere senza un test.

**La proposta di sostanza: la soglia sia sul prezzo, non sulla qualità.** Il
riempimento la calcola (perché il prezzo è funzione del prompt che si rilegge
ogni volta), il prezzo la giustifica.

---

## 3. Domanda 2 — dove vive un flusso a ronda

Quattro case possibili. Tre hanno già fallito su questa macchina, con i numeri.

### Casa A — dentro l'agente (un gancio della CLI). **Scartata, con le prove.**

È dove la staffetta è nata, ed è dove è rimasta zoppa.

- **Un gancio blocca, non invoca.** Limite strutturale, misurato il 13/08: un
  gancio può iniettare testo o negare uno strumento, non chiamare una competenza.
  «Automatico» al massimo *costringe*.
- **Un gancio scatta su un gesto, e chi non fa quel gesto è invisibile.**
  `handoff-arms-successor` scattava su `PostToolUse Write|Edit`: una sessione
  piena che non scrive file non armava nessun successore. Misurato: una sessione
  al **106% del budget** non rigenerata da nessun gancio in-sessione.
- **L'agente non ha il potere di azzerare se stesso.** Dichiarato il 13/08:
  «Claude non ha nessuno strumento per `/clear` o `/compact`».
- **E vale per una CLI sola.** I ganci nativi non esistono per Codex né Gemini:
  «metà del lavoro delegato a un'altra CLI esce oggi senza nessun presidio».

**Il modo di fallire, in una riga:** un flusso che azzera la sessione che lo
esegue si azzera da solo.

### Casa B — un residente per ogni sessione sorvegliata

Un processo lungo per soggetto, che tiene il tubo e la memoria in RAM.

- **Costo**: N processi, N riconciliazioni, N modi di morire.
- **Modo di fallire**: il sorvegliante muore e nessuno se ne accorge. È il
  guasto **B2** del catalogo, nella sua forma peggiore: la staffetta ha
  candidato 462 volte e agito 10 in un giorno — **452 uscite mute** — e la cosa è
  emersa contando le righe di un registro, non da un allarme.
- **Aggravante misurata**: ~70 sonni al giorno. Un residente per sessione è un
  residente che dorme quando dorme la macchina, e al risveglio non sa quanto ha
  dormito se non l'ha scritto.

**Non scartata, ma non prima:** ha senso solo per il tubo (qualcuno deve tenere
`stdout` aperto), non per il giudizio.

### Casa C — un residente solo, che a ogni battito rifà lo stesso flusso su tutti i soggetti

È la staffetta reale: `launchd`, `StartInterval 60`, un passo per invocazione,
stato interamente su disco.

- **Costo**: latenza fino al periodo del battito. Con 60 s è irrilevante rispetto
  alla scala della grandezza sorvegliata (il contesto si riempie in ore).
- **Modo di fallire**: *«uno stato scritto che nessuno riverifica»* — meta-pattern
  che il catalogo trova in quattro guasti distinti (A4, E4, G1, D1). Il caso
  peggiore: la staffetta ha **chiuso il pannello di una sessione viva che
  aspettava una risposta umana** (E4), perché il registro diceva una cosa e il
  mondo un'altra.
- **Cura, già scoperta là**: ogni fatto si rilegge dal mondo al momento di
  decidere, mai dal record; e *illeggibile* non è *morto* — se l'elenco dei
  pannelli non si legge, non si conclude che siano morti tutti.

### Casa D — nessun residente: il battito lo dà il sistema operativo

Uguale a C, ma senza processo che resta: `launchd` avvia un processo nuovo a
ogni giro.

- **Costo**: un avvio di processo per battito (irrilevante a 60 s).
- **Modo di fallire, misurato**: `launchctl list` dentro il perimetro risponde
  **zero servizi**, senza errore né diniego (memoria
  `launchctl-dentro-il-perimetro-risponde-vuoto`). Il 25/08 questo ha nascosto
  che **4 automazioni su 8 non erano caricate**, scoperto per caso durante
  un'altra indagine.

### La raccomandazione

**Casa C con la disciplina di D.** Un residente sottile che è **solo un
orologio**: non ricorda niente, non tiene stato in RAM, e se muore e riparte il
battito dopo è indistinguibile. Il tubo delle sessioni sorvegliate è un'altra
cosa e sta in casa B, dove deve stare.

E il controllo che dimostra che la disciplina è vera, da scrivere *insieme* al
residente e non dopo:

> **La prova del sonno.** Uccidi il residente in un istante qualunque del ciclo,
> riavvialo, e il battito successivo deve produrre lo stesso deposito che avrebbe
> prodotto senza l'interruzione. Se non lo produce, c'è memoria in RAM che
> dovrebbe stare nel deposito.

**Perché questo semplifica tutto.** Se la ronda è un flusso corto ripetuto,
allora **non serve un nuovo tipo di flusso**. Serve un `.flow.json` aciclico,
un campo `schedule`, e qualcuno che lo esegua quando è dovuto. Il motore di
Sailor rifiuta i cicli per costruzione (`GraphError::Cycle`, Kahn, `graph.rs:171`)
— e questo smette di essere un limite: **il ciclo sta nel tempo, non nel grafo.**

---

## 4. Domanda 3 — cosa resta codice e cosa diventa flusso

### 4.1 Il criterio di Theo non regge alla prova dei guasti

La proposta del mandato: *resta codice ciò che esegue, diventa flusso ciò che
decide*. Ho provato a smontarla, e si smonta in due punti.

**Primo: sposterebbe lo 0,9% del codice.** La decisione della staffetta è una
funzione pura, `guards::handoff::evaluate` — **69 righe vive** su **7.695 righe
vive** di tutto l'apparato della staffetta. Il resto tocca il mondo. Spostare la
decisione in un flusso muove meno dell'uno per cento.

**Secondo, e più grave: i guasti non stavano lì.** Delle 32 avarie catalogate:

| famiglia | istanze | dove sta il difetto |
|---|---|---|
| A — il segnale misurato è sbagliato | 5 | nei **fatti** |
| E — l'azione non arriva a destinazione | 7 | negli **effetti** |
| F — il successore nasce menomato | 6 | negli **effetti** |
| G — due lavoratori, un lavoro | 3 | negli **effetti** |
| B — rinvii muti, soglie irraggiungibili | 5 | nell'**osservabilità** |
| C — la guardia nega a tutti | 4 | nella **decisione** |
| D — l'ordine delle soglie | 2 | nella **decisione** |

**21 su 32 nei fatti e negli effetti. 6 nella decisione.** Fare della decisione
un flusso non avrebbe evitato quasi nessuno di questi guasti.

**Ma un guadagno c'è, ed è la famiglia B.** Un passo di flusso deposita
l'intenzione *prima* dell'effetto e l'esito *dopo* (`append_step_started` /
`close_step`, `executor.rs:495` e `:534`). Le 452 uscite mute su 462
candidature sarebbero state 462 righe nel deposito. La staffetta ha passato **31
testimoni su 2.834 candidature (1,09%)** in sette giorni e nessuno lo sapeva.
Non perché decidesse male: perché **non lasciava traccia di ciò che non faceva.**

> **Il guadagno del flusso non è la correttezza. È la visibilità.** Vale la pena
> dirlo così a Theo, perché cambia cosa si converte per primo e come si giudica
> se è andata bene.

### 4.2 Il criterio che propongo: il confine è il potere

> **È codice ciò che ha bisogno di un potere sul mondo. È flusso ogni frase
> composta con quei poteri.**

Poteri: lanciare un processo, leggere e scrivere un file, aprire una porta,
battere un tasto su un pannello, depositare. *Decidere non è un potere*: è
composizione.

**Il test operativo, che si applica senza discutere:** puoi scriverlo componendo
azioni che esistono già? è un flusso. Ti serve un potere che nessuna azione
concede? è un'azione, cioè codice.

Questo criterio non è mio: è la conseguenza di una decisione che Theo ha già
preso e che `AGENTS.md` non lascia riaprire — *«un flusso è un file di dati; i
nodi sono azioni registrate in Rust. Nessun interprete dentro Sailor.»* Il
criterio del potere è semplicemente quella decisione detta in modo che si possa
applicare a un caso nuovo.

**E spiega il buco che il documento di stato aveva già visto senza spiegarlo.**
`shell_check` esegue `sh -c`: è **un'azione che concede ogni potere**. Con lei
nel vocabolario, il confine evapora — chiunque scriva un flusso può fare
qualunque cosa, e il freno al confine del processo lo scavalca chi scrive il
flusso. Non è un rischio futuro, è già la scelta predefinita: contati oggi sui
tre flussi dell'albero, i passi sono **6 `shell_check`, 4 `external_engine`, 2
`trigger`** — la scappatoia è **metà di tutto ciò che esiste**, e
`flows/prova-della-vista.flow.json` la usa su tre passi su tre.

Regola che ne discende: **le letture possono passare dalla shell, gli effetti
no.** Un effetto passa da un'azione tipizzata o non passa.

### 4.3 L'esempio di Theo: il rilevamento degli strumenti è **già** un flusso

Theo porta `crates/toolbox` come esempio di ciò che dovrebbe diventare un flusso
di sistema. Misurato, è già quasi tutto dall'altra parte del confine:

- **il dato è dato**: 36 descrittori in `descriptors/default.json`, tre famiglie
  (25 `tool`, 6 `ai_cli`, 5 `mcp_server`), estendibili da `~/.sailor/tools.d/`
  senza ricompilare, e un `id` ripetuto sovrascrive quello spedito;
- **la logica è già un'azione registrata**: `detect_tools`
  (`toolbox/src/action.rs:21`), con i suoi parametri come dato —
  `DetectSpec{descriptor_paths, include_defaults, family, version_probes}`;
- **il vincolo è già dichiarato e rispettato**: `lib.rs:5-11` dice che nessun
  nome di strumento compare nel codice. Il codice sa fare tre verifiche —
  cercare un eseguibile, guardare se un file esiste, leggere le chiavi di un
  JSON — e il resto è dato.

Delle ~1.412 righe di `toolbox/src`, quelle che restano codice per il criterio
del potere sono le sonde (`probe.rs`, 309 righe: esegue, legge, analizza) e la
risoluzione (`resolver.rs`, 228). Sono poteri. Restano.

**Quindi la risposta a Theo è: non c'è niente da spostare qui, e il fatto che
sembri esserci è la scoperta utile.** «Diventare un flusso di sistema» per il
rilevamento strumenti non significa riscriverlo: significa **che qualcuno lo
invochi come passo di un flusso invece che come funzione**. Oggi nessun flusso
lo fa.

Dove il confine è violato davvero, misurato:

| pezzo | violazione |
|---|---|
| `crates/notte` | 2.562 righe per un flusso di 4 passi; già condannato |
| la staffetta | 6 passi dichiarati in prosa in testa a `relay.rs`, **1.400 righe di funzione che li eseguono a mano** |

Ed è qui la leva vera, che corregge la formula di Theo:

> **Il flusso non prende la decisione. Prende la sequenza.**
> Decisione: 69 righe. Sequenza: 6 nodi al posto di 1.400 righe.

### 4.4 Il paradosso del fondo, e i tre assi che lo reggono

*«Qualcuno deve eseguire i flussi, e quel qualcuno non può essere un flusso. Se
il rilevamento degli strumenti diventa un flusso, e quel flusso ha bisogno di uno
strumento per girare, come si esce dal cerchio?»*

**Il cerchio non si chiude, perché il vocabolario è finito e compilato.** Un
flusso può nominare solo azioni che il binario registra: oggi otto
(`external_engine`, `shell_check`, `trigger`, `detect_tools`, `store_write`,
`store_read`, `store_list`, `notte-task`). Il flusso di rilevamento strumenti non
ha bisogno di *nessuno* strumento esterno per girare — ha bisogno di
`detect_tools`, che è dentro il binario. Il cerchio si chiuderebbe solo se un
flusso potesse **estendere il vocabolario**, ed è esattamente ciò che la
decisione «nessun interprete» vieta.

Il fondo ha tre assi, e nessuno dei tre può essere un flusso:

1. **Il vocabolario** — le azioni. Un flusso le compone, non le crea.
2. **L'orologio** — chi sveglia la ronda. Un flusso può *dichiarare* quando
   vuole essere svegliato (`Schedule{recurrence, weight, perimeter}` esiste già
   in `flow/src/schedule.rs`); il gesto di svegliare è codice.
3. **Il freno** — chi autorizza. Oggi in Sailor è un segnaposto:
   `ExecutionRequest.gates` viene **registrato ma non applicato**, e tutti e tre
   i chiamanti reali passano un elenco vuoto (`flow_cmd.rs:274`,
   `desktop/src-tauri/src/run.rs:382`, `notte/src/main.rs:1423`). `perimeter` è
   dichiarato e non fatto rispettare.

**E il caso «il file del flusso non c'è»?** Non si risponde con un altro flusso.
Si risponde con una **copia di serie compilata nel binario**, che il file su
disco sovrascrive. Questo schema non va inventato: **`toolbox` lo attua già** —
`descriptors/default.json` è incorporato nel binario e sovrascrivibile da
`~/.sailor/tools.d/`. Il crate che Theo indica come esempio di ciò che deve
diventare flusso **ha già risolto il paradosso che teme.** Si copia.

---

## 5. Cosa lascia in eredità la staffetta

Recuperata da un archivio di configurazione del 28/08/2026, tenuto fuori dal
repo, ed estratta in una cartella temporanea (`~/.claude` non è stato
ripopolato).
**12.382 righe di Rust**, 4.160 righe di registro, 33 rapporti di guasto.

### Riusabile quasi parola per parola

| pezzo | dove stava | perché vale |
|---|---|---|
| **le soglie per modello** | `guards/handoff.rs:27-52` | Opus 4.8 200k, Opus 5 500k, Sonnet 5 400k, Haiku 4.5 150k, Fable 5 300k, ignoto **180k**; avviso 0,78, obbligo 0,90. Sono budget di **qualità**, non finestre tecniche, e sono fondati su ricerca citata |
| **la somma giusta dei token** | `context_used_from_lines` | tre campi sommati; contarne uno solo sottostima di un ordine di grandezza con la cache calda |
| **«non lo so» ≠ «no»** | `turn_status_from_lines` | una riga illeggibile ferma la scansione invece di scavalcarla; un elenco illeggibile non vuol dire «sono morti tutti» |
| **l'elenco chiuso di ciò che passa** | `HANDOFF_TOOLS` | si dichiara cosa serve a consegnare, non cosa è vietato: l'elenco dei divieti è sempre in ritardo sullo strumento nuovo |
| **la sequenza a 6 passi** | prologo di `relay.rs` | già scritta, con il verso in cui sbagliare dichiarato: se il `/clear` non parte non si perde niente; se parte e il mandato non arriva, si è distrutto tutto |
| **`/clear` invece di crea-e-chiudi** | dal 19/08 | la vecchia via aveva 5 punti in cui sbagliare e ha sbagliato: **47 sessioni in più in due giorni** |

### Rotto, e da non ricostruire uguale

I quattro guasti più cari, con i numeri:

1. **La spia tace** (B2, aperta). 462 candidature, 10 azioni, **452 uscite
   mute**. Sull'intero registro: **31 testimoni su 2.834 candidature = 1,09%**.
   *Requisito*: ogni battito deposita un esito, anche «non ho fatto niente, e
   perché».
2. **La guardia nega a tutti** (C1/C2, aperte). Il successore si armava nel
   **4,4%** dei casi: 66 aperti, 1.429 fermati, per **8 condizioni in AND tutte
   negative** — `albero-affollato` 421, `fuori-orario` 317, `troppe-sessioni`
   296, `non-piena` 201. Costo su una sola sessione: **37 turni e 15,3 milioni
   di token** dopo la consegna. *Requisito*: un elenco di negazioni cresce e non
   cala mai; ogni freno dichiara quante volte ha morso, o si toglie.
3. **Il segnale misurato è sbagliato** (A2, chiusa). Il giudizio «la sessione è
   ferma» guardava l'ultima riga del transcript, e i ganci di casa ci appendono
   righe che turni non sono: **588 giri identici in 10 ore**, **21 ore senza una
   sola rigenerazione**. *Requisito*: una condizione che non può cadere da sola
   non è un'attesa, è un blocco.
4. **Il successore nasce menomato** (famiglia F, 6 istanze). Il punto di ripresa
   cercava la riga «Procedo con»: **262 sessioni su 348 (75%) non ce l'avevano**.
   *Requisito*: ciò che viaggia col testimone si prova sul testimone arrivato,
   non su quello partito.

E un quinto che riguarda il progetto più della staffetta: **`tui-idle` dice
«zitto», non «libero».** Rispondeva `satisfied: true` con una modale aperta, e
l'Invio della staffetta **ha risposto a una domanda al posto di Theo**, misurato
271 ms prima della risposta registrata, su 2 pannelli su 2.

---

## 6. Cosa manca a Sailor perché una ronda sia scrivibile

Misurato sull'albero a HEAD `3792fec`. Il motore è più avanti di quanto il
problema richieda; mancano quattro cose, e sono piccole.

| # | cosa manca | stato oggi |
|---|---|---|
| 1 | **l'orologio** | `Schedule`, `Recurrence::{EverySeconds, DailyAt}`, `is_due` e `sailor flow due` **esistono**; nessuno esegue ciò che `due` calcola. Nessun `cron`, nessun `.plist`, nessun timer nel motore |
| 2 | **le azioni per intervenire** | ne servono quattro che non esistono: attendi-che-sia-fermo, leggi-il-pannello, batti-una-riga, misura-la-sessione. Le tre di deposito (`store_write/read/list`) ci sono già ed è lì che va la memoria fra un battito e l'altro |
| 3 | **l'ascolto** | `crates/trigger` ha la forma ma non la sostanza: `Kind::Terminal` risponde sempre `listening_not_built`; delle 3 sorgenti dichiarate solo `manual` funziona |
| 4 | **il confronto in una condizione** | `Condition::{Equals, PointerEquals, PointerExists}`: **non esiste il maggiore-di**. La soglia — cioè l'esempio stesso di Theo — oggi **non è esprimibile** in un `when` |

**Il punto 4 non richiede di toccare il formato**, e non deve: l'azione che
misura restituisce il verdetto (`{"oltre_soglia": true, "stimato": 462000}`) e il
`when` legge quello con `PointerEquals`. La soglia sta nei dati del passo
(`with: {"soglia": 450000}`). È coerente col criterio: l'aritmetica è un potere
minuscolo che sta nell'azione, il numero è una decisione che sta nel flusso.

**Il punto 2 è dove il criterio del potere si guadagna il pane.** «Batti una riga
su quel pannello» **deve** essere un'azione tipizzata. Se lo si fa con
`shell_check "orca terminal send ..."` si ottiene la stessa cosa in dieci minuti
e si perde il freno per sempre.

---

## 7. Le decisioni che restano a Theo

Tre, e sono separate perché si possono prendere separatamente.

### Decisione 1 — la soglia si mette sul prezzo o sulla qualità?

**Il fatto**: il degrado della qualità, dall'esterno e senza una risposta giusta
nota, **non è osservabile** (21 sessioni su 44: una moneta). Il prezzo di
continuare **è** osservabile (+34%, 37 sessioni su 45, monotono su 5 fasce), e
così il riempimento (errore mediano 6,0% col modello affine).

**Le due strade**:
- **(a)** Sailor dichiara di misurare il *prezzo*, e la soglia si giustifica con
  «continuare qui costa un terzo in più che ricominciare di là». Onesto,
  difendibile davanti a chiunque, agnostico.
- **(b)** Sailor dichiara di misurare la *qualità*, e allora deve portarsi
  dietro un test con una risposta giusta nota da somministrare all'agente ogni
  tanto — che costa contesto proprio a chi ne ha poco, ed è il primo esperimento
  da fare se si sceglie questa.

**La mia raccomandazione: (a)**, e (b) come misura di ricerca, mai come freno.

### Decisione 2 — `shell_check` resta il vocabolario o diventa l'eccezione?

**Il fatto**: `shell_check` esegue `sh -c`, cioè concede ogni potere; ed è già
**6 passi su 12** di tutto ciò che è scritto oggi nell'albero. Se resta così, il
freno al confine del processo — che è il vincolo da cui nasce tutto il resto —
lo scavalca per costruzione chiunque scriva un flusso.

**Le due strade**:
- **(a)** `shell_check` resta, ma **solo per leggere**: gli effetti passano da
  azioni tipizzate. Serve una regola scritta e un controllo che la faccia
  rispettare, non una buona intenzione.
- **(b)** `shell_check` resta com'è, e si accetta che il confine sia una
  convenzione. Legittimo, ma allora va scritto che il freno non c'è.

**Questa decisione è di Theo perché tocca il vincolo che lui ha posto**, e perché
la strada (a) rallenta ogni conversione futura di un passo.

### Decisione 3 — la prima ronda si converte, o si aspetta l'orologio?

**Il fatto**: la staffetta è già la prima conversione pianificata
(`sailor-adesso.md`, punto 3 dell'ordine dei lavori), e il suo grafo è già
disegnato. Ma senza l'orologio (punto 1 del §6) un flusso a ronda si può solo
lanciare a mano, e a mano non è una ronda.

**Le due strade**:
- **(a)** Prima l'orologio, poi la staffetta. Il primo flusso a ronda nasce già
  vero. Costo: una cosa nuova nel motore prima di aver provato che il modello
  regge.
- **(b)** Prima la staffetta come flusso lanciato a mano, con `sailor flow due`
  a dire se sarebbe dovuta. Si prova il modello prima di costruire l'orologio, e
  la prova costa un lancio a mano ogni tanto. Costo: per un po' la staffetta è
  meno viva di com'era.

**La mia raccomandazione: (b)**, per la ragione che il commento del codice dà già
meglio di me — *«è il gradino prima di lasciarla eseguire a una macchina»*.

---

## Cosa in questo documento è misurato, e cosa no

**Misurato oggi, su questa macchina.** Le tre misure sul segnale (88 sessioni,
42.591 punti per il riempimento; 44 sessioni, 16.209 turni per la ripetizione; 45
sessioni, 36.710 turni per il prezzo). Il rapporto decisione/apparato della
staffetta (69 righe su 7.695). Il catalogo dei 32 guasti e i conteggi del
registro (4.160 righe, 31 rigenerazioni, 2.834 candidature). L'inventario del
motore, delle azioni registrate e dei descrittori.

**Non provato, e va provato.** Che i byte contati su un tubo vero si comportino
come i byte ricostruiti dai transcript: è una simulazione su dati reali, non un
esperimento su un involucro. Che `k` e `C` stimati su Claude Code valgano per
Codex o Gemini: nessuna misura, e c'è ragione di credere che `C` cambi. Che
convertire la staffetta in flusso produca la visibilità promessa: è
un'inferenza dal fatto che ogni passo deposita, non una misura.

**Preso da altri e ricontrollato.** Le soglie per modello e la loro fondazione;
il racconto dei guasti dai rapporti della plancia; l'architettura del motore.
Dove un numero viene da un documento e non dalle mie mani, il documento è citato.
