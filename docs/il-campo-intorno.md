# Il campo intorno: chi altro costruisce questa cosa

**31/08/2026.** Nato da una domanda di Theo — «*non ci credo che là fuori non ci
siano progetti simili al mio*» — dopo che una ricerca di questo stesso giorno
aveva risposto «nessuno».

**Aveva ragione lui.** La ricerca aveva fatto una domanda strettissima — *chi fa
girare un flusso dentro una sessione già viva tenendo il registro passo per
passo?* — e su quella la risposta «nessuno» era vera. Ma è una domanda così
stretta che **la risposta era garantita in partenza**, e nessuno aveva chiesto
quella vera: *chi costruisce un prodotto dove si vede cosa può fare l'AI?*

È il secondo errore della stessa forma nella stessa giornata: l'altro è l'A/B
del passo consegnato, corso senza il braccio del terminale
(`2026-08-31-la-porta-non-paga.md`). **Un lavoro rigoroso su una domanda troppo
stretta produce una risposta che sembra solida e non lo è.**

## Stato di questo documento

Quello che segue viene da **una ricerca sul web, non da prodotti aperti e
provati**. È una mappa per sapere dove guardare, non un verdetto su nessuno.
Ogni voce che verrà verificata aprendola va marcata come tale.

## Il campo si divide in due, e Sailor sta a cavallo

### 1. Orchestratori di agenti a riga di comando

Pilotano `claude`, `codex`, `gemini`, `cursor` come motori. È il livello dei
motori di Sailor. **La categoria ha già un nome e due elenchi curati.**

| chi | cosa fa | perché ci riguarda |
|---|---|---|
| **Nimbalyst** | spazio di lavoro visuale con Claude Code e Codex **come motori di esecuzione**; kanban di sessioni parallele, revisione diff inline, isolamento in worktree | è la descrizione del guscio di Sailor, scritta da qualcun altro |
| **Agent Orchestrator** | app desktop **e** riga di comando; sorveglia Claude Code, Codex, Cursor, OpenCode e **20+ agenti** in parallelo, ognuno con worktree, ramo e pull request | il nostro «motore intercambiabile» a una scala che noi non abbiamo |
| **CloudCLI** | interfaccia web e mobile per più CLI, locale o remota | il caso d'uso che noi non copriamo |
| **Claude Code Desktop** | l'app ufficiale: orchestrare più agenti senza pannelli di terminale | il concorrente che non dobbiamo battere sul suo terreno |
| **Codex app** | thread paralleli, ciascuno isolato nel proprio worktree | idem |

Gli elenchi curati:
- https://github.com/bradagi/awesome-cli-coding-agents — «agenti a riga di comando e le impalcature che li orchestrano»
- https://github.com/andyrewlee/awesome-agent-orchestrators

### 2. Tele di nodi per flussi di AI

Il livello del grafo. **È affollatissimo e maturo.**

| chi | scala | nota |
|---|---|---|
| **Dify** | ~130.000 stelle | flussi visuali, RAG, 100+ fornitori, cruscotto di osservabilità; self-hosting gratuito |
| **Langflow** | ~100.000 stelle | tela visuale, e **il codice Python di ogni componente si modifica**, non solo in sandbox |
| **Rivet** | MIT | grafi di agenti con debug visuale, versionamento in YAML, collaborazione in tempo reale |
| **Flowise**, **n8n**, **Coze** | | la stessa famiglia |

Nota che pesa: un articolo già citato dalla ricerca di stamattina raccoglie
**guasti veri da Dify, Coze e n8n** — cioè quei prodotti sono abbastanza usati
da avere una letteratura sui loro difetti. Noi no.

## Cosa sembra ancora nostro — e con quale cautela

Due cose la ricerca non ha trovato da nessuna parte:

- **Il conto vero, chiamata per chiamata**, coi quattro tipi di token separati e
  i loro prezzi diversi. La ricerca del mattino aveva già misurato che **nessun
  catalogo pubblico porta quota residua né patto sui dati** (models.dev e
  OpenRouter, scaricati e contati).
- **Il flusso come grafo dichiarato che pilota le CLI.** Le tele di nodi
  pilotano le API; gli orchestratori desktop pilotano le CLI ma **senza grafo**.
  Sailor sta esattamente nel mezzo.

**Ma non è una misura: è ciò che una ricerca sul web non ha mostrato.** Dopo
oggi, un'assenza non si vende come un fatto.

## Il rischio vero, ed è cambiato

Non è che qualcuno ci copi. È che **stiamo rifacendo a mano cose che esistono
già e funzionano meglio** — e che nessuno ce lo dica, perché non abbiamo mai
guardato.

## Come lo gestiscono loro — primo giro, 31/08

### Crewplane è il nucleo di Sailor, già costruito

«*Control plane a riga di comando per flussi di agenti disegnati da una
persona, in Markdown; esegue fasi sequenziali o parallele attraverso Claude
Code, Codex, Gemini CLI, Copilot CLI **o qualunque comando configurato**,
riprende dopo un fallimento, e tiene ingressi, uscite e registri su disco.*»

Punto per punto contro di noi: il flusso dichiarato ✓, la catena di motori per
identificativo ✓, la ripresa dopo un guasto ✓ (l'abbiamo costruita **oggi**),
e — la frase che pesa di più — **«passaggi di artefatto espliciti che tengono
l'esecuzione ispezionabile su disco»** e **«nodi completati validati che
permettono a un flusso fallito di riprendere»**.

Quel «passaggio di artefatto esplicito» è **parola per parola** la decisione che
abbiamo preso noi — *fra i passi passa un artefatto, non una conversazione* — e
i «nodi completati validati» sono `reconcile`.

~~Quello che Crewplane **non** dichiara: nessun conto dei token, nessun costo,
nessun tetto. È il pezzo che resta nostro.~~

**Falso, e corretto il 04/09/2026 clonando il repository invece di leggerne la
descrizione.** Crewplane ha cinque classi di gettoni — `input`, `cached_input`,
`cache_write`, `output`, `reasoning` — un prezzo per milione **per classe**, e
un livello di **confidenza** del costo (`exact`, ripiego stimato a quattro
caratteri per gettone, `none`). Cioè il nostro stesso principio — *ciò che non è
misurato non diventa un numero* — scritto da qualcun altro come tipo, non come
commento. Ha anche i worktree git, che questa pagina non registrava.

Resta nostro qualcosa di più stretto, e va detto così invece che in grande:
**la sesta classe.** Nessuno tiene separata la scrittura di cache a lunga
durata — Crewplane si ferma a cinque, Claudexor collassa lettura e scrittura in
un campo solo. Sul nostro descrittore quella classe è il **96% del costo** di
una chiamata misurata: collassarla sbaglia il conto di un ordine di grandezza.

Quello che Crewplane davvero non fa è leggere la quota dal fornitore: cerca
sottostringhe nell'uscita (`quota_reached_on_contains`) e poi aspetta. È il
contrario del criterio di `unusable_when`.

### Claudexor risolve la quota, e col nostro stesso criterio

«*Piano di controllo locale che tiene un solo filo di lavoro attraverso Claude
Code, Codex, Cursor e OpenCode. Collega più account dello stesso harness,
**traccia la quota di ciascuno**, e a richiesta ruota automaticamente quando uno
raggiunge il limite.*»

E la riga che conta: **«sposta il lavoro solo dopo un limite confermato — mai
per un normale errore di rete».** È esattamente il principio di
`unusable_when`, e la distinzione del guasto 14 fra «finito fino a un'ora nota»
e «rotto», scritta da qualcun altro.

Anche **teamclaude** fa rotazione multi-account su quota.

### La quota residua si legge — e noi avevamo scritto che non si poteva

La ricerca del mattino aveva concluso: *per codex e le CLI a forfait non esiste
nessun canale documentato; sapere che sono esaurite prima di chiamarle non si
può.* **Falso**, per due motori su tre:

| motore | da dove | unità |
|---|---|---|
| **Codex** | `codex app-server`, metodo JSON-RPC **`account/rateLimits/read`**; in alternativa lo SQLite `~/.codex/state_5.sqlite` | percentuale usata **+ istante di azzeramento**, finestre 5 ore e 7 giorni |
| **Claude Code** | endpoint OAuth non documentato `api/oauth/usage` con intestazione `anthropic-beta: oauth-2025-04-20`; credenziali in `~/.claude/.credentials.json` | percentuale 0–1, finestre 5 ore e 7 giorni (azzeramento settimanale giovedì) |
| **Gemini CLI** | **nessun canale ufficiale**: si ricostruisce dai file di sessione `~/.gemini/tmp/_/chats/session-*.json` sommando `tokens.total` | conteggio sessioni, che **non corrisponde 1:1 alle richieste** |

Due avvertenze da tenere se lo adottiamo: l'endpoint di Claude è **beta e
versionato**, quindi può rompersi in silenzio; e quello di Codex è **interno**,
non un REST pubblico.

**Questo chiude il pezzo mancante del guasto 14** (l'attesa: nessuno legge
«si azzera alle 7») e dà alla voce 7 della ricerca il canale che le mancava per
codex — senza inventare niente, che era la condizione.

### Gli altri, in una riga

- **MartinLoop** — «*token and cost budgets, iteration caps, stop conditions*»:
  il nostro tetto di spesa, più il tetto sui giri che noi non abbiamo.
- **Omnigent** — agenti definiti in YAML, e «*applica politiche di
  approvazione, di spesa e sugli strumenti*»: i nostri cancelli.
- **cc-router** — slot virtuali opus/sonnet/haiku con ripiego e bilanciamento:
  il routing, fatto sul modello invece che sul passo.
- **Vibestrate** — «*flusso YAML di fasi*».
- **DeerFlow**, **fractal** — deleghe gerarchiche a sotto-agenti.

## Le tre cose da fare — fatte il 04/09/2026

Le prime due erano «aprire Crewplane» e «aprire Claudexor», la terza «provare i
canali di quota». Fatte, e **la prima ha smentito subito una riga di questa
pagina**: è la ragione per cui il primo giro non contava come misura.

**Il canale della quota**: `sailor remaining` risponde per Claude Code, letto da
`api/oauth/usage`. Codex e Gemini non sono ancora letti da nessuno.

**Claudexor, sul punto della rotazione.** Non guarda mai la prosa, e il suo
documento di architettura lo scrive: *«the classifier reads only typed event
fields — never prose»*. L'adattatore di ogni motore traduce il frame nativo in
**due campi distinti e disgiunti** sullo stesso evento — `rate_limit` e
`transient {network | service_unavailable}` — e un predicato solo, ottantacinque
righe, decide. Il transitorio si ritenta **sullo stesso profilo**, e non entra
mai nella rotazione.

E porta la condizione che a noi non era venuta in mente:

> non si ruota mai su un tentativo che ha già toccato il disco.

Solo se il risultato è vuoto **e** nessuna modifica è stata osservata. È la metà
del guasto 31 che non avevamo nominato, ed è MIT.

## Cosa resta da fare adesso

1. **Aprire herdr** — Rust, Apache-2.0, 35.248 stelle, spinto il 04/09: un
   server di sfondo che **possiede i pseudo-terminali**, con i riquadri marcati
   `working | blocked | idle` e un'interfaccia su socket. È il nostro
   `sailor terminal host`, con tre anni di vantaggio. Non conta gettoni e non
   legge quota. Mezza giornata ad aprirlo prima di scrivere un'altra riga lì.
2. **Le parole con cui `agy` dice di aver finito la quota**, che è la misura
   ancora mancante del guasto 31. Il giorno in cui si vedono, `agy` torna a
   poter fare da ripiego.
3. **La quota di Codex dal canale giusto**: il metodo app-server
   `account/rateLimits/read` (da `rust-v0.142.0`) — **non** i file di rollout,
   dove `rate_limits` è nullo in modo exec e, per un difetto aperto, sempre
   nullo dal rilascio di gpt-5.4. Claudexor legge ancora i rollout: qui siamo
   avanti se ci arriviamo per primi.

**Due divieti, e vengono da chi ha già sbattuto.** Non rinnovare mai un gettone
al posto della riga di comando: su Claude un rinnovo fuori banda invalida la
copia della CLI, su Codex il gettone di rinnovo è a uso singolo e rotante.
*Leggere sì, rinnovare no.* E il canale che usiamo per Claude non è documentato
— quello documentato è la statusline, che porta le stesse due finestre e non
costa una chiamata: vanno tenuti in coppia, non uno al posto dell'altro.

Le righe di questa pagina che non dicono «clonato e letto» restano indizi, non
misure. Al 04/09 sono clonati e letti Crewplane e Claudexor, e nient'altro.
