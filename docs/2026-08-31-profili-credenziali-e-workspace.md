# Profili, credenziali e spazi di lavoro — cosa c'è, cosa manca, cosa va deciso adesso

**31/08/2026.** Quattro analisi in parallelo, tre che misurano questo albero e una
che studia come lo risolvono gli altri. Questo documento tiene **i fatti
misurati** separati dalle **decisioni**, perché le prime sono verificabili da
chiunque rilanci i comandi e le seconde sono di Theo.

Dove un numero compare senza la parola «misurato», è una lettura del codice e non
una prova eseguita: la distinzione è mantenuta ovunque, ed è la sola ragione per
cui questo documento vale più di un'opinione.

## La domanda

Parole di Theo, 31/08/2026:

> «Vorrei avere anche credenziali globali da poter usare in qualsiasi workspace,
> ma avere anche credenziali specifiche che da lì non escono. Le credenziali di
> ogni account CLI per AI le metterei globali, poi farei un flusso integrato alle
> funzioni di Sailor per organizzare chi usa cosa, quando e come. **Non vorrei mai
> che un altro terminale aperto in altri workspace magari potrebbe avere altre
> credenziali.**»

E il perché, che allarga il problema oltre lo sviluppo del software:

> «Nei flussi che farei nel workspace Other-repo ci sarebbero anche gli MCP e le
> credenziali per accedere all'account Slack, Google, Granola. Idem per Notion. In
> Slack mi piacerebbe poter avere Claude che legge le conversazioni: "vedi la
> conversazione in X", "il mio collega mi ha scritto questo, cosa ne pensi". Con
> Linear vorrei aggiungere pezzi al loop di sviluppo. **Sailor non è solo un
> sistema per fare software.**»

## Il difetto ha un nome, ed è vecchio

**Ambient authority**: i permessi sono concessi in base a una proprietà *globale*
del programma in esecuzione, invece che a qualcosa che viaggia con la richiesta.
È la precondizione del **confused deputy**: l'autorità cambia senza un gesto
esplicito di nessuno. Il rimedio in letteratura è uno solo — **designazione
esplicita**: il bersaglio viaggia col comando, non risiede in un file che nessuno
guarda.

Sailor ce l'ha già, in miniatura, e va tolto adesso che le voci sono due invece
che duecento.

## I fatti misurati

### 1. Il profilo attivo è globale, e il conto è del 67,5%

Lo stato è **un file solo per tutta la macchina**: `~/.claude/state/profili.json`,
con `active: cli_id -> nome`. Non esiste nessun campo che nomini un progetto, una
cartella o un terminale.

**Misurato** con un finto `codex` che stampa la propria casa, due profili, e due
processi che fanno ciascuno la cosa ovvia — `switch` al proprio profilo, poi
`run` — quaranta giri a testa:

    terminale che voleva «uno»:
      27 lanci con la casa di «due»   <- il profilo dell'ALTRO
      13 lanci con la casa di «uno»

**Ventisette su quaranta con l'identità sbagliata, uscita 0, nessun avviso.**

Seconda misura sullo stesso stato: **60 profili creati da due processi in
parallelo → 60 cartelle sul disco, 25 righe nel registro.** Trentacinque
creazioni perse, perché lo stato si rilegge-modifica-riscrive senza lucchetto.

### 2. Non esiste nessun confine sull'ambiente

**`grep -rn "env_clear\|env_remove" crates/` → zero risultati.** I tre punti in
cui Sailor genera un processo figlio — il motore, la verifica, il server MCP —
**aggiungono** le variabili dichiarate dal passo *sopra* l'ambiente ereditato.

**Misurato** con un finto server MCP che scrive il proprio `printenv`: il figlio
ha visto **114 variabili**, fra cui nomi che portano segreti — `NPM_GITHUB_TOKEN`,
`CLAUDE_CODE_MESSAGING_TOKEN`, `ORCA_AGENT_HOOK_TOKEN`,
`ORCA_AGENT_LAUNCH_TOKEN`, `CLOUDSDK_PROXY_PASSWORD` — più `SSH_AUTH_SOCK`, che
non è un segreto ma è la capacità di firmare.

Il requisito è «da un workspace non escono». **Oggi succede l'inverso: le
credenziali di tutta la macchina entrano in ogni server che un flusso avvia.**

E non è un indebolimento del requisito: è che il requisito **non ha nessun
meccanismo su cui appoggiarsi**. Non c'è modo di costruirci sopra un flusso
onesto — si costruirebbe qualcosa che ha l'aria di una separazione senza esserlo.

### 3. Il segreto che passa da `env` è scritto su disco e non è cancellabile

Il record di un passo porta l'ingresso intero, e il deposito lo serializza così
com'è **prima** di eseguire.

**Misurato** con due fixture a costo zero. Un canarino finto in `env` compare in
chiaro nell'ingresso del passo e in due eventi. Con un rinvio `{"$from": …}` è
peggio: **quattro** eventi, perché il valore vero sta già nell'uscita di chi lo
produce. Sul deposito vero, **15 righe su 124** hanno già un `env` dentro.

E `events.db` porta i trigger `events_append_only_update` e
`events_append_only_delete`, che **abortiscono ogni UPDATE e ogni DELETE**.

> Una credenziale che passa dal campo `env` di un passo è scritta in chiaro in un
> registro che il sistema stesso impedisce di modificare.

### 4. Il meccanismo per fare la cosa giusta esiste ed è provato

**Misurato** con quattro sonde su Claude Code, circa 0,36 $ in tutto:

| riga di comando | autenticata | server MCP nella sessione |
|---|---|---|
| `claude` | sì | quelli dell'utente |
| `claude --setting-sources ""` | **sì** | **nessuno** |
| `--setting-sources "" --mcp-config <file>` | sì | **quelli del file, col nome scelto** |
| `--setting-sources "" --plugin-dir <plugin>` | sì | **nessuno** |

Le prime due righe sono la verginità che il progetto ha già scelto come
principio. La terza è il pezzo che serve: **un file dichiarato da fuori porta i
server di questo workspace in una CLI che non ha visto la configurazione
dell'utente.**

La quarta è una scoperta che avrebbe fatto perdere tempo: **`--plugin-dir` porta
le competenze di un plugin ma non i suoi server MCP.** Isolata con un canarino —
competenza `SI`, strumenti MCP `NESSUNO` — dopo che un primo tentativo con un
plugin malformato avrebbe dato la conclusione opposta. Il descrittore di Sailor
oggi tiene le due cose sotto la stessa parola `receive_equipment`.

### 5. La dotazione è dichiarata e non consegnata

**Nessuna riga di Sailor appende mai `--setting-sources`, `--mcp-config` o
`--plugin-dir`.** `command_line` monta solo il blocco `ask` del descrittore, più
`usage` e il prompt. `receive_equipment` e `isolate_from_user_config` sono
dichiarazioni che **solo `sailor flow check` legge**, per stampare un avviso.

È il guasto 18, ancora aperto. Il modo in cui la dotazione viene davvero
raggiunta oggi è un percorso assoluto scritto a mano in cima a un passo.

Nello stesso stato: il campo `equipment` esiste in `sailor.json` **ed è letto da
zero righe di codice**. Anzi, `declaration_at` non è chiamata da **nessun** codice
di produzione: del marcatore si usa oggi **solo la posizione**.

### 6. Le credenziali MCP oggi si scrivono nel flusso versionato

L'unico canale previsto per dare un segreto a un server MCP è `server.env`, una
mappa di **valori letterali** dentro il `.flow.json`. Nessuna espansione, nessun
riferimento a un portachiavi, nessuna indirezione. Un token di Slack finirebbe in
chiaro in un file che il progetto versiona e che viaggia col repo.

### 7. Le due azioni MCP funzionano

**Eseguite** contro un server vero, a costo zero, al primo colpo. E con la
distinzione che conta che regge dal vivo: con l'archivio vettoriale spento,
`mcp_ready` ha risposto `could_not_look` — «non ho potuto guardare» — invece di
«il progetto non è indicizzato». È la differenza fra dire una cosa vera e
affermare sul mondo qualcosa che nessuno ha verificato.

### 8. Il pericolo non è «un modello»: è «un dato»

Tre superfici, tutte verificate leggendo il codice:

- **`handed_to_agent` risolve i rinvii sul campo `mandate`.** Un passo può quindi
  cucire testo di terzi dentro il mandato che va a **un agente già vivo nel
  terminale**, che per decisione scritta lavora senza briglie. Non c'è un gradino
  successivo da conquistare: è il gradino finale.
- **Un flusso è un file JSON trovato risalendo l'albero.** Chi scrive in una
  cartella `flows/` sopra quella da cui lanci sostituisce in silenzio un flusso
  di sistema con lo stesso nome. Lo stesso vale per un descrittore in
  `~/.config/sailor/tools.d`, dove «due descrittori con lo stesso `id` non
  convivono: l'ultimo caricato vince» — chi ci scrive ridefinisce cosa significa
  `claude-code` per **ogni** flusso, senza toccarne nessuno.
- **`"tool"` è letto dopo la risoluzione dei rinvii**, quindi la risposta di un
  motore può diventare l'identità con cui il passo dopo agisce. A trattenerlo
  sono `answer_shape` e `output_schema`, che stanno **nello stesso file** che i
  due punti precedenti rendono scrivibile. **Misurato: 7 flussi su 9 hanno
  almeno un passo con `output_schema` aperto.**

### 9. Un'invariante vera che non è scritta da nessuna parte

Oggi nessuna azione registrata emette chiavi di primo livello scelte da un terzo:
tutte serializzano da un letterale o da una struct. È ciò che impedisce a
un'uscita di iniettare `args` o `env` nel passo successivo.

**Non è scritta in nessun documento, non ha una prova, non è nominata in nessun
commento.** La prima azione scritta la settimana prossima che restituisca un
corpo JSON di terzi «così com'è» la rompe, e nessun controllo diventa rosso.

### 10. n8n: la domanda è mal posta, ed è quello il difetto

Theo ha citato n8n come il caso da non ripetere. La domanda era: *un workflow può
usare una credenziale che non gli è stata esplicitamente concessa?*

**Sì — e la domanda non ha senso in n8n, che è precisamente il problema.**
L'unità di autorizzazione è l'**utente**, o al massimo il progetto. Mai il
workflow. Non esiste nessun punto in cui si dichiari «questo flusso può usare
queste credenziali e nessun'altra»: un nodo contiene solo un puntatore per
identificativo, e chi modifica il workflow sceglie dal menu a tendina qualunque
credenziale la *sua* identità raggiunga.

Tre frasi, tutte dalla documentazione ufficiale di n8n:

> «La condivisione di un workflow permette agli editor di usare **tutte le
> credenziali usate nel workflow**. Questo include **credenziali che non sono
> state esplicitamente condivise con loro**.»

> «Chiunque possa modificare un workflow potrebbe potenzialmente leggere il
> vostro database, la chiave di cifratura, le credenziali memorizzate e le
> variabili d'ambiente.»

> [sull'allowlist dei domini] «Non ha alcun effetto quando la credenziale è usata
> nel proprio nodo dedicato.»

La terza è la più istruttiva, perché n8n **aveva l'idea giusta** — legare una
credenziale alle destinazioni ammesse — e l'ha applicata nel posto sbagliato:
nodo per nodo, invece che nel punto in cui il segreto lascia il processo. Il
22/07/2026 sono stati pubblicati **oltre quaranta advisory in blocco**, e nove
riguardano esattamente questo perimetro. Il pattern è **uno solo, ripetuto nove
volte**: il controllo è a monte (al salvataggio, sul tipo dichiarato, sul nodo),
l'uso è a valle (a runtime, sul tipo reale, in un altro nodo). Ogni nodo nuovo è
una nuova occasione di divergenza.

**Due sistemi vicini hanno chiuso la porta, e vale la pena copiare come.**
Zapier, dal 29/05/2026, verifica **all'accensione** che il proprietario abbia
accesso a ogni connessione usata: se manca, lo Zap non si accende. Windmill
risolve il segreto **a runtime con i permessi del chiamante**, e *«il job
fallisce se i permessi ereditati non consentono l'accesso alla variabile»*.

### 11. La lista dei permessi è diventata il contenitore del segreto

Questo riguarda questa macchina, non un'azienda lontana.

GitGuardian, agosto 2026: **4.576 token n8n unici** trovati in commit pubblici su
GitHub; su 896 istanze raggiungibili, **321 accettavano un token trapelato**. E
fra i luoghi da cui i token sono usciti, oltre ai soliti `.env`, i ricercatori
citano **`.claude/settings.json` e `.claude/settings.local.json`** — la lista dei
permessi di Claude Code — dove l'URL e la chiave finiscono dentro un comando
`curl` inserito nell'allowlist, *«senza le stesse protezioni di `.gitignore` che
gli sviluppatori applicano di solito ai file `.env`»*.

> **Una allowlist che memorizza il comando letterale memorizza anche il segreto
> dentro il comando.**

È un avvertimento diretto per qualunque cosa Sailor costruisca in materia di
permessi, di comandi approvati e di verifiche: il posto dove si scrive «questo è
consentito» diventa il posto dove il segreto si posa.

### 12. Il fallback all'ambiente è la vulnerabilità, non la comodità

Il caso più vicino a noi ha un numero: **CVE-2026-45707**, su `n8n-mcp`, maggio
2026. Le richieste che omettevano gli header di identità — **o ne fornivano solo
uno** — *«ripiegavano silenziosamente sulle credenziali a livello di processo
configurate per l'istanza dell'operatore»*. Un utente autenticato eseguiva contro
l'istanza di qualcun altro.

**La correzione non è stata un controllo migliore: è stato il rifiuto.** Il
progetto ora *«si rifiuta di costruire un client con le credenziali d'ambiente»*
quando è in modalità multi-utente.

E la risposta giusta esiste da trent'anni, in uno strumento che tutti usano:
`sudo` ha **`env_reset` attivo per difetto**, ambiente minimo, e una lista
esplicita di ciò che si conserva.

### 13. Come lo risolvono gli altri

**Nessuno l'ha risolto bene, e chi l'ha risolto l'ha fatto nel 2017.**

`git` lo risolve dal maggio 2017 con `includeIf`, per percorso e per URL del
remoto — ed è in uso su questa macchina. `gh` no: l'issue **#326 è aperta dal 7
febbraio 2020 con 501 reazioni**, e chiede letteralmente credenziali per
repository, motivandolo con `includeIf`. Sei anni e mezzo.

La formulazione più esatta del difetto viene da un'altra issue di `gh`, e
riguarda direttamente gli orchestratori:

> «La selezione dell'account è **globale**, il che rende difficile lavorare in
> parallelo con gli agenti. Gli agenti automatici non possono fare `gh auth
> switch` in sicurezza: è una **mutazione globale con corsa**, e non esiste
> nessun `--account` per singola invocazione.»

**Sul messaggio d'errore non esiste una convenzione, ed è la scoperta più utile.**
GitHub e GitLab collassano su 404 per non rivelare l'esistenza di una risorsa
(GitLab lo **impone per iscritto** ai propri sviluppatori). Google prescrive
normativamente il contrario in AIP-193: 403 **anche se la risorsa non esiste**, e
«il permesso va controllato prima dell'esistenza». Kubernetes non maschera
affatto. La RFC 9110 concede la facoltà senza raccomandarla; OWASP accetta
entrambi.

Ma ciò che accomuna tutte le scuole è l'unica cosa che conta:

> **Nessuna risposta d'errore nominerà mai l'identità sbagliata**, perché
> nominarla confermerebbe che la risorsa esiste.

**Verificato**: `curl` su un repo privato reale e su uno inventato restituisce
corpi **byte-identici**.

Da cui la conseguenza operativa: **poiché l'errore non può dire chi sei, il
sistema deve dirlo sempre, da sé, prima.** È la ragione per cui esistono
`kube-ps1`, il prompt rosso che GitLab ha adottato dopo aver perso 300 GB nel
2017, e la «differenziazione visiva delle interfacce» adottata da GOV.UK dopo
aver cancellato la produzione nel 2016.

**Gli incidenti dicono tutti la stessa cosa.** GOV.UK 2016 (interfacce identiche,
più schede aperte, tutte le app cancellate, gli utenti hanno dovuto ricaricare il
proprio codice sorgente). GitLab.com 2017 (l'host nella sessione attiva, ~300 GB,
cinque backup su cinque inefficaci). DigitalOcean 2017 (*«un processo di test
automatico era configurato male, con le credenziali di produzione»*). Spotify
2019 — e il secondo incidente è avvenuto **un mese dopo il primo, mentre
codificavano la prevenzione**: *«abbiamo modificato senza saperlo lo stato globale
durante build di revisione»*.

E quello che è il nostro caso esatto, **KeepTheScore**: l'host era `localhost`,
ma la variabile d'ambiente non era stata esportata, quindi la connessione si è
inizializzata con le credenziali di produzione. Oltre 300.000 tabelloni
cancellati, sette ore di dati perse per sempre.

> **Variabile non impostata → il default è la produzione.**

Due rimedi che quelle organizzazioni hanno davvero adottato, e che valgono qui:
**differenziare visivamente l'ambiente**, e **impedire ai processi automatici di
toccare lo stato globale**.

**Un rimedio che non va copiato**: `aws-vault` è dichiarato abbandonato dai suoi
autori, e il suo server di metadati porta scritto il proprio varco — «mentre
questo server gira, **qualunque** applicazione che voglia connettersi ad AWS
potrà farlo, col profilo con cui il server è partito». La sua variante corretta è
per sottoprocesso, con il token esposto solo al figlio via ambiente: quella è la
forma giusta.

## Le tre decisioni

Sono tre perché, aggiunte dopo, costringono a riscrivere ciò che nel frattempo è
stato costruito. Per due delle tre esiste già su questa macchina una misura di
quanto costi accorgersene tardi.

### D1 — La provenienza è un campo del dato, e si calcola dove l'ingresso si compone

Ogni valore che attraversa il grafo porta la propria origine: `fidato` se scritto
nel flusso o battuto da una persona, `non fidato` se uscito da un motore, da un
server MCP, da un file o dalla rete. L'etichetta si propaga per contatto e si
calcola **una volta sola in `step_input`**. Un insieme chiuso di campi —
`mandate`, `command`, `bin`, `tool`, `args`, `env`, `workdir`, `server.command` —
**non accetta mai** un valore di provenienza non fidata.

**Perché adesso, e non è un'opinione.** È letteralmente il guasto 28 un piano più
su, con lo stesso identico modo di fallire: *«la risoluzione dei rinvii non
appartiene alla singola azione: va fatta una volta sola dove l'ingresso si
compone, così ogni azione registrata — comprese quelle che nessuno ha ancora
scritto — la eredita. Finché sta dentro le azioni, ogni azione nuova nasce senza,
e nessun controllo lo dice.»*

Messa oggi è una funzione e una prova. Messa fra dieci azioni, sono dieci audit.

**Nasce con la sua prova**: una fixture in cui un `mandate` prende un rinvio a
un'uscita di motore, e il passo deve rompersi. Rossa il giorno che si scrive.

**Cosa costa.** Un flusso che oggi passa la risposta di un motore dentro un
comando smette di essere scrivibile. La via d'uscita onesta è un cancello
esplicito — una persona ha guardato questo valore e lo ha promosso — che è un
passo in più e un'attesa in più.

### D2 — L'ambiente del figlio si costruisce, non si eredita

`env_clear()` sui tre punti in cui Sailor genera un processo, e ricostruzione
dell'ambiente **solo** con ciò che il passo ha dichiarato e che la corsa è
autorizzata a concedere. La concessione è per (workspace × azione).

I segreti non passano mai dall'ingresso tipato: un passo non dichiara
`"env": {"TOKEN": "…"}` ma un **riferimento**, risolto sul confine del `Command`,
**dopo** che il record è stato costruito. È la stessa scelta strutturale già
presa nel deposito per tenere `input`/`output` fuori dai tipi dello storico, con
la stessa motivazione scritta: *«non c'è un campo da dimenticare di togliere»*.

**Perché adesso.** Sono poche righe oggi. Ogni giorno che passa sono le stesse
righe più l'elenco di ciò che si è rotto: i motori e i server MCP che oggi
funzionano stanno accumulando dipendenze mute dall'ambiente ereditato, e nessuno
le sta scrivendo.

**Cosa costa.** Ogni `npx` si rompe per primo. Elencare ciò che serve a ciascuno
è lavoro noioso, non difficile, e si fa una volta per descrittore — cioè in un
file di dati, coerentemente col vincolo permanente. Si perde la comodità di
«esporto una variabile nella shell e i flussi la vedono»: quella comodità **è** la
vulnerabilità.

**Corollario dello stesso gesto**: il descrittore di un server MCP dichiara la
versione invece di `-y`. Un'approvazione data una volta non deve vincolare
qualunque codice venga pubblicato domani.

### D3 — L'insieme delle opzioni è configurazione, e l'autorizzazione è una tripla

Un flusso nomina un **profilo di instradamento**; l'insieme dei profili ammessi
per un workspace vive dove né un flusso né un contenuto arrivano, e le azioni lo
leggono da sé. Un modello può **ordinare** le opzioni e motivare la scelta: non
può aggiungerne, e la scelta è validata per appartenenza a un insieme esatto
prima che qualunque cosa parta.

E l'autorizzazione non è solo *chi*: è la tripla **(identità, poteri,
destinazioni)**, valutata sulla corsa intera e verificabile **prima di lanciare**.

**Perché adesso, con la data.** `docs/decisioni.md` l'ha già scritto il 30/08, in
fondo alla voce sul multi-fornitore: *«non tutti i lavori possono andare
ovunque… Aggiungerla dopo vuol dire aver già mandato qualcosa nel posto
sbagliato.»* Quella frase è stata scritta pensando alle fasce gratuite che
addestrano sui dati ricevuti. **Vale identica, con conseguenze peggiori, per
Slack, Notion, Google e Linear.**

**Cosa costa.** Si perde «cambio motore modificando una riga del flusso»:
aggiungere un motore diventa due gesti invece di uno. Un flusso portato da
un'altra macchina va autorizzato prima di girare — che è scomodo esattamente
quanto è il punto.

## Perché la prima formulazione della regola non bastava

La regola proposta era:

> «Il routing sceglie fra opzioni già autorizzate, e l'insieme delle opzioni non
> lo decide un modello.»

**È giusta e non regge da sola**, per tre ragioni:

1. **Dice «un modello» e il pericolo è «un dato».** Chi vuole aggirarla resta
   dentro la lettera: non chiede a un modello di scegliere un account non
   dichiarato — aggiunge un `.flow.json` che *dichiara* quell'account, o riscrive
   un descrittore in `tools.d`. Il routing continua a scegliere fra opzioni «già
   autorizzate»; è cambiato **chi autorizza**.
2. **Difende l'identità e lascia scoperta la riga di comando.** `args`, `env`,
   `bin`, `workdir`, `stdin` decidono cosa viene eseguito quanto l'identità.
3. **La combinazione pericolosa non si compone nel routing.** «Usa l'account
   Slack dell'azienda A» può essere autorizzato e restare la mossa sbagliata, se
   in quella stessa corsa è già entrata una pagina scritta da un esterno. E il
   fronte di esecuzione parte in parallelo: due passi senza dipendenza fra loro
   non sono ordinati da nessuna regola sull'ordine.

## Le difese che già ci sono, e che non vanno smontate per distrazione

Elencate perché la tentazione, costruendo, è di toglierle senza accorgersene.

- **La finestra ha già `img-src 'self' data:; connect-src 'self' ipc:`.** È
  esattamente il canale da cui sono usciti i dati di ChatGPT, GitLab Duo, M365
  Copilot e Superhuman. Chi vorrà mostrare le anteprime di Notion nella finestra
  sta per riaprirlo: quella riga va difesa con una prova.
- **`$join` rifiuta di unire ciò che non è testo**, e **un rinvio dentro una
  risposta resta una frase**: due barriere reali contro l'iniezione strutturale,
  entrambe con prova.
- **Un valore risolto non si rilegge**: è ciò che impedisce di lavare un segreto
  attraverso due rinvii.
- **`input` e `output` sono fuori dai tipi** che alimentano lo storico, con
  canarino nella prova.
- **La negazione è il predefinito** su `accept` e su `same_holder_ok`, con la
  motivazione giusta: una lista di permessi dimenticata lascia passare tutto, una
  negazione dimenticata al massimo ferma un lavoro.
- **`require_preflight` rifiuta un `proves` vuoto**, perché ogni testo contiene
  la stringa vuota. È il tipo di ragionamento che serve applicato al resto.

## Cosa resta aperto, e non è stato misurato

- **L'elenco per piattaforma delle variabili che un server MCP stdio eredita non
  esiste.** La documentazione ufficiale dice che ne eredita «solo un
  sottoinsieme limitato, e l'insieme esatto dipende dalla piattaforma», e quale
  sia non è scritto da nessuna parte. Il sottoinsieme è contemporaneamente
  **troppo piccolo** per far partire i server (i noti fallimenti di `PATH` da
  interfaccia grafica) e **troppo grande** per stare tranquilli.
- **Cosa fanno `--ignore-user-config` di codex e `--extensions` di gemini** coi
  loro server MCP: provato solo Claude Code.
- **`agy` non è nella tabella dei profili**, pur essendo installato e usato dai
  flussi veri.
- **Ci sono due nozioni separate di «spazio di lavoro»**: `terminal::Workspace`
  (una cartella col suo nome) e `flow::workspace` (la risalita al marcatore). Non
  si conoscono. Vanno unite **prima** di appenderci i profili, o nasce il guasto
  delle due verità.
- **I permessi del file di deposito** non sono stati verificati: solo che il
  codice non ne imposta nessuno.
- **Il modello di minaccia non è stato eseguito**: è lettura del codice più due
  script in sola lettura sui flussi. Le tre analisi che hanno *eseguito* sono
  quella sui profili, quella su MCP e le quattro sonde sulla verginità.

## Il criterio, in una riga

Sailor ha già preso la decisione giusta stamattina, per un altro dato — la radice
del progetto:

> «Assente vuol dire assente. Nessun ripiego sulla cartella del processo: un
> flusso che lavora dove capita senza dirlo fa danno invece di fallire.»

**È lo stesso criterio, e va esteso alle credenziali.** L'assenza di un profilo
dichiarato non deve mai significare «usa quello globale»: deve significare
fermarsi e dirlo. È la sola differenza fra questo sistema e le sette ore di dati
che KeepTheScore ha perso perché una variabile non era stata esportata.

E ha già un precedente con un numero di CVE: la correzione di `n8n-mcp` non è
stata un controllo migliore sul ripiego, **è stata la rimozione del ripiego**.

## Una nota per chi costruirà, e non è una raccomandazione di prudenza

Tre cose che questo lavoro ha trovato e che conviene tenere insieme, perché sono
la stessa cosa vista da tre lati:

- **Una allowlist che memorizza un comando letterale memorizza il segreto dentro
  il comando.** È successo davvero, dentro `.claude/settings.json`, su questa
  stessa famiglia di strumenti.
- **`CLAUDE_CONFIG_DIR` sposta anche la voce del portachiavi di macOS**, non solo
  la cartella: una sessione con una cartella diversa legge una voce diversa. È la
  leva per processo che serve, e nessuno la tira per difetto.
- **Il posto dove si scrive «questo è consentito» è il posto dove il segreto si
  posa.** Vale per le allowlist, per i comandi approvati, per i descrittori e per
  i flussi versionati. Un segreto non deve mai poter entrare in un file che
  qualcuno vorrà versionare, condividere o mostrare — e l'unico modo di
  garantirlo è che quel file contenga **un riferimento**, mai un valore.
