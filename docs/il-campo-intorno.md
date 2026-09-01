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

Quello che Crewplane **non** dichiara: nessun conto dei token, nessun costo,
nessun tetto. È il pezzo che resta nostro.

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
- E in quell'elenco c'è anche **Orca**, classificato fra i «solo paralleli,
  nessun grafo dichiarato».

## Cosa resta da fare

Questo giro è stato fatto **leggendo pagine, non aprendo prodotti**. Restano
tre cose, in ordine di valore:

1. **Provare i canali di quota davvero**, su questa macchina: `codex
   app-server` e l'endpoint OAuth di Claude. Sono due comandi, costano zero, e
   chiudono un guasto aperto.
2. **Aprire Crewplane** e guardare come dichiara una fase e come riprende: è la
   cosa più vicina a noi, e sapere dove diverge vale più di qualunque recensione.
3. **Aprire Claudexor** sul punto della rotazione: come distingue un limite
   confermato da un errore qualunque. Il guasto 31 è chiuso dal 01/09/2026 —
   chi non dichiara come si esaurisce sta in fondo alla catena, e un controllo
   lo pretende — ma la misura che manca è ancora quella: le parole con cui `agy`
   dice di aver finito la quota. Il giorno in cui si vedono, `agy` torna a poter
   fare da ripiego.

Finché non sono fatte, ogni riga qui sopra è un indizio, non una misura.
