# Tutto quello che Sailor sa fare, e cosa di questo si vede

**02/09/2026.** Censimento sistematico del motore, crate per crate, misurato
interrogando il codice e il binario. Nasce da una frase di Theo davanti a
un'anteprima: *«stai facendo forse l'1% del prodotto»*. Aveva ragione, e questo
documento è il conto.

**Come leggerlo.** Ogni voce ha uno stato:

| | |
|---|---|
| **CLI** | c'è un comando che lo fa |
| **finestra** | c'è una superficie che lo mostra o lo comanda |
| **nessuno** | il motore lo sa fare e nessuno lo può chiedere |

Dove un numero compare, viene da un comando che chiunque può rilanciare. Tre
strumenti usa-e-getta sono risultati ciechi durante questo censimento e i loro
numeri sono stati buttati; sono elencati in fondo.

---

## 1. I diciannove crate, e cosa ciascuno espone

| crate | righe | cosa sa fare | si vede? |
|---|---:|---|---|
| `actions` | 12 133 | le 23 azioni che un passo può eseguire | **parziale** — la finestra ne offre 7 famiglie |
| `sailor` | 18 760 | la riga di comando: 14 comandi, 49 verbi | **CLI** |
| `flow` | 8 021 | il grafo, le corse, la pianificazione, i tetti di spesa, i sotto-flussi | **parziale** |
| `toolbox` | 5 510 | quali strumenti ci sono, in che versione, se sei autenticato | **nessuno** |
| `ledger` | 5 591 | il deposito: corse, passi, eventi, chiamate, inventario | **parziale** — solo la storia |
| `terminal` | 3 557 | pseudo-terminali veri, con smistamento delle richieste | **finestra** |
| `models` | 3 005 | il listino, i prezzi, la quota residua, le modalità | **nessuno** |
| `ui` | 2 330 | ciò che la finestra legge: viste, raccolta, sorgenti | **finestra** |
| `inventory` | 1 771 | competenze, agenti, comandi, regole, ganci — e quali sono spenti | **CLI** |
| `sessions` | 1 635 | chi entra, cosa succede, cosa c'è sulla macchina | **CLI** |
| `supervisor` | 1 384 | ricostruire e rimettere in servizio a caldo | **nessuno** |
| `trigger` | 849 | gli inneschi: cosa fa partire una corsa | **parziale** |
| `registry` | 845 | il registro delle azioni e il collegamento al deposito | interno |
| `faults` | 794 | i guasti: cosa si è rotto e il controllo che manca | **CLI** |
| `profiles` | 697 | i profili di una riga di comando, con le loro case | **CLI + finestra** dal 02/09 |
| `relay` | 511 | la staffetta fra sessioni | **nessuno** |
| `catalogue` | 410 | la lingua di ciò che si legge | interno |
| `release` | 320 | mettere in servizio un binario costruito da HEAD | **nessuno** |
| `workspace` | 162 | gli alberi di lavoro di un repository | **finestra** |

**Sei crate su diciannove non hanno nessuna porta**: `toolbox`, `models`,
`supervisor`, `profiles`, `relay`, `release`. Sono 11 447 righe di motore che
nessuno può chiedere da nessuna parte se non leggendo il codice.

---

## 2. Le ventitré azioni che un passo può eseguire

Estratte dalle costanti `*_ACTION`. La cassetta della finestra offre **sette
famiglie**, ognuna con una sola azione predefinita: le altre sedici sono
raggiungibili solo scrivendo il JSON a mano.

| azione | cosa fa | nella cassetta? |
|---|---|---|
| `shell_check` | esegue un comando e dice se è andato | **sì** (check) |
| `external_engine` | invoca un motore a riga di comando | **sì** (engine) |
| `handed_to_agent` | consegna il passo a una persona | **sì** (human) |
| `subflow` | esegue un altro flusso come passo | **sì** (subflow) |
| `store_write` | scrive nel deposito chiave-valore | **sì** (deposit) |
| `trigger` | il punto d'ingresso del flusso | **sì** (trigger) |
| `type_into_terminal` | scrive dentro un terminale che Sailor possiede | **sì** (gesture) |
| `store_read` | legge una chiave dal deposito | no |
| `store_list` | elenca le chiavi di un prefisso | no |
| `history_ask` | interroga lo storico delle corse | no |
| `mcp_ask` | chiama uno strumento di un server MCP | no |
| `mcp_ready` | verifica che un server MCP risponda | no |
| `detect_tools` | censisce cosa c'è sulla macchina | no |
| `tool_needs` | incrocia ciò che serve con ciò che c'è | no |
| `fault_record` | registra un guasto | no |
| `fault_list` | elenca i guasti | no |
| `work_claim` | prende in carico un lavoro condiviso | no |
| `work_release` | lo rilascia | no |
| `work_survey` | guarda chi sta facendo cosa | no |
| `take_mandate` | prende il mandato di una sessione | no |
| `ask_without_interaction` | chiede senza aspettare una persona | no |
| `empty_terminal` | verifica che un terminale sia fermo | no |
| `measure_terminal` | misura cosa c'è in un terminale | no |

**Le tre azioni di lavoro condiviso** — `work_claim`, `work_release`,
`work_survey` — sono il coordinamento fra agenti, e nella finestra non esiste
niente che le nomini.

---

## 3. I quattordici comandi, e i loro quarantanove verbi

| comando | verbi | nella finestra |
|---|---|---|
| `flow` | list · run · check · cost · cap · due · relocate · resume · schedule · tick | **2 su 10**: list, run |
| `session` | list · open · close · attach · detach · census · event · install | **0 su 8** |
| `faults` | list · add · check · import · render · status | **0 su 6** |
| `terminal` | list · run · press · reset · mandate | **4 su 5** — manca `mandate` |
| `profiles` | list · current · create · switch | **4 su 4** ✓ dal 02/09 — `current` è la riga «in force» |
| `models` | list · current · set | **0 su 3** |
| `worktree` | create · list · remove | **3 su 3** ✓ |
| `step` | open · close | **0 su 2** |
| `workspace` | init | **0 su 1** |
| `inventory` | — | mostrato in parte |
| `remaining` | — | **no** |
| `release` | — | **no** |
| `run` | — | **no** |
| `version` | — | **no** |

**Diciotto verbi su quarantanove hanno una porta.** Trentuno no.

---

## 4. I concetti di prodotto, uno per uno

### 4.1 Lo spazio di lavoro — il buco più grande

Un workspace si dichiara con **`sailor.json`** nella radice del progetto
(`crates/flow/src/workspace.rs`). Il file può essere `{}`: quello che conta è
**dove sta**, perché la sua posizione è la risposta alla domanda «qual è la
radice». Dentro può dichiarare:

| campo | cosa dice |
|---|---|
| `name` | come si chiama il progetto |
| `rules[]` | quali documenti valgono come regole (AGENTS.md, decisioni.md…) |
| `checks{}` | i controlli del progetto, per nome |
| `equipment` | quale dotazione pretende |
| *(campi ignoti)* | conservati, mai motivo di scarto — è il guasto 8 |

E l'origine di un flusso porta l'avvertimento con sé: `this project` se il
marcatore c'è, **`this project (no sailor.json: root guessed)`** se la radice è
stata indovinata risalendo fino a una cartella `flows/`.

**Cosa manca, ed è tutto:**

- non c'è **nessun elenco di spazi di lavoro**: Sailor ne conosce uno alla volta,
  quello in cui gira;
- non si può **passare da un progetto all'altro** dalla finestra;
- **i flussi di un progetto** si vedono mescolati a quelli di casa, distinti solo
  dall'etichetta d'origine;
- `sailor workspace init` esiste ed è **l'unico verbo**: non c'è `list`, non c'è
  `switch`, non c'è modo di vedere cosa un progetto ha dichiarato;
- il documento del 31/08 sulle credenziali pone la domanda vera e nessuno l'ha
  ancora risolta: *«non vorrei mai che un altro terminale aperto in altri
  workspace potrebbe avere altre credenziali»*.

### 4.2 I profili e le credenziali

`crates/profiles` sa: elencare, dire qual è in uso, crearne, scambiare le case
con un collegamento simbolico (`SymlinkSwap`), costruire l'ambiente di un
processo figlio (`build_environment`), riconoscere le CLI note (`KnownCli`).

Misurato su questa macchina: **due profili, entrambi `NOT AUTHENTICATED`**. Lo
si scopre quando una corsa fallisce. Nella finestra: niente.

> **Aperto il 02/09/2026.** La finestra ha ora la schermata «Profili»:
> le righe di comando note con **la nota che dice come lo sappiamo**, i profili
> di ciascuna con lo stato d'accesso chiesto al motore dentro la casa di *quel*
> profilo, e i due gesti — passare a un profilo, crearne uno. La risposta è
> quella di `sailor profiles list`, non una seconda: `profiles_cmd::overview()`
> è la copia unica. Restano fuori le credenziali di workspace e l'`env_clear()`
> del paragrafo qui sotto.

Il piano scritto il 31/08 va oltre e non è costruito: credenziali **globali** e
credenziali **di un workspace che da lì non escono**; l'ambiente del figlio che
**si costruisce invece di ereditarsi** (`env_clear()`); l'autorizzazione come
tripla (workspace × azione × profilo).

### 4.3 Gli strumenti e le capacità — `toolbox`, 5 510 righe, zero porte

Sa dire, per ogni strumento: se c'è, dove, in che versione, se sei autenticato
(`LoginStatus`), quali capacità offre (`Capability`, `CapabilityForm`,
`CapabilityState`), quali ganci e abilità ha una sessione (`SessionHooks`,
`SessionAbilities`), e cosa manca a un flusso per girare (`ToolNeedsAction`).

Il flusso `what-this-machine-has` fa esattamente questo giro e stampa
«questo flusso non gira perché manca questo, e si installa così». **Nella
finestra non esiste niente di tutto ciò.**

### 4.4 Gli inneschi — due forme, e una è dichiarata e muta

`trigger::Kind` ha **due varianti**: `Manual` e `Terminal`. I descrittori sono
dati (`~/.config/sailor/triggers.d/`), la forma è codice.

`Terminal` **è dichiarato e non ascolta**. Sta scritto nel repo perché è una
scelta: *«un ascolto simulato sarebbe peggio di un ascolto assente, perché un
flusso verde direbbe che qualcuno ha parlato»*.

Manca del tutto **il tempo**: `flow` ha già `Schedule`, `Recurrence`, `is_due`,
`tick` — e il documento `2026-09-01-il-tempo-e-l-ultima-scelta.md` elenca le
cinque decisioni da prendere prima di scriverne il nodo.

### 4.5 Il deposito — otto tabelle, e non esiste ancora

`runs` · `steps` · `events` · `model_calls` · `inventory_items` · `processes` ·
`snapshots` · `store`.

Su questa macchina **`~/.config/sailor/ledger` non è mai stato creato**. La
finestra mostra solo `execution_history` e `run_snapshot`: la storia delle corse.
Non si vedono gli eventi, le chiamate ai modelli, l'inventario nel tempo, i
processi, il deposito chiave-valore.

Il mandato di agosto lo diceva già: *«Sailor registra tutto quello che succede e
non torna mai a leggerlo»*.

### 4.6 I modelli, i prezzi, la quota

`models` sa: il listino (48 voci misurate), quale è in uso, cambiarlo, i prezzi
per milione di token, le modalità accettate (testo/immagine/video), il contesto,
e **quanta quota resta** letta dal motore (`Remaining`, `from_claude_oauth_usage`).

Nella finestra: **niente di tutto questo**. Una corsa si ferma sul limite e
nessuno sa quanto restava — che è il fatto da cui è nata la ricerca del 29/08.

### 4.7 La staffetta, la supervisione, il rilascio

- **`relay`** — passare il testimone fra sessioni. Il progetto sta in
  `docs/2026-08-28-il-flusso-che-accompagna.md`. Nessuna porta.
- **`supervisor`** — `cargo_build`, `rebuild_then_swap`, `LiveStatus`,
  `close_the_ones_that_stopped_breathing`: ricostruire e rimettere in servizio a
  caldo. Nessuna porta. Il comando `live_status` è esposto dal guscio **e la
  finestra non lo chiama**.
- **`release`** — `Readiness`, `Service`, `Target`, `read_stamp`: mettere in
  servizio un binario costruito da HEAD, mai dall'albero di lavoro. Nessuna porta.

### 4.8 Le sessioni

`sessions` sa: chi entra e chi esce (`Arrival`, `Inhabitant`), il censimento
(`Census`), gli eventi di un terminale, l'ancoraggio per tty
(`tty_of_nearest_ancestor`), e la misura di quanto è piena una sessione
(`Fullness`). Otto verbi da riga di comando, **nessuno nella finestra**.

### 4.9 I guasti

`faults` sa registrarli, elencarli, verificarli, importarli, renderizzarli, e
dire lo stato di ciascuno (`Standing`). Misurati **66** in
`docs/guasti-incontrati.md`. Nella finestra: niente.

---

## 5. Quello che la finestra chiama davvero

Ventiquattro comandi su ventisei esposti dal guscio:

`day_summary` · `delete_flow` · `discover_tools` · `execution_history` ·
`flow_trigger` · `flows` · `known_runs` · `machine_inventory` · `manual` ·
`open_runs` · `run_snapshot` · `run_usage` · `save_flow` · `start_run` ·
`step_history` · `terminal_*` (6) · `worktree_*` (3)

Mai chiamati: **`flow_places`** (dove stanno i flussi sul disco) e
**`live_status`** (lo stato vivo, che la finestra sonda invece di ascoltare).

---

## 6. La lista di ciò che è da connettere, in ordine

L'ordine è per **quanto motore sblocca per riga di finestra scritta**.

### Primo — quello che serve ogni giorno e non c'è

1. ~~**Gli spazi di lavoro**~~ — **fatto il 02/09**: il registro
   (`workspaces.json`), `sailor workspace list` col suo `gone`, la schermata
   «Progetti» e i flussi raggruppati per origine. Resta il **passaggio** da un
   progetto all'altro, che muove radice, terminali e credenziali insieme.
2. ~~**I profili**~~ — **fatto il 02/09**: quale è in uso, lo stato d'accesso
   chiesto al motore dentro la casa di ciascuno, passare da uno all'altro e
   crearne. Resta ciò che il piano del 31/08 chiama credenziali **di un
   workspace**, e l'`env_clear()`.
3. **La quota e i prezzi**: quanto resta, quanto costa, quale modello.
   Tre comandi già scritti.
4. **Gli strumenti**: cosa c'è sulla macchina, in che versione, se autenticato,
   e cosa manca a un flusso per girare. 5 510 righe già scritte.

### Secondo — il deposito che si interroga

5. **Le otto tabelle**, non solo la storia delle corse: eventi, chiamate ai
   modelli, inventario nel tempo, processi, il deposito chiave-valore.
6. **I guasti** dentro la finestra, con il controllo che manca a ciascuno.

### Terzo — le azioni che nessuno può comporre

7. **Le sedici azioni fuori dalla cassetta**, a partire da quelle che il motore
   esegue già bene: `store_read`, `store_list`, `history_ask`, `mcp_ask`,
   `detect_tools`, `tool_needs`.
8. **Il lavoro condiviso**: `work_claim`, `work_release`, `work_survey`.

### Quarto — quello che va deciso prima di costruirlo

9. **Il tempo**: cinque decisioni aperte, scritte il 01/09.
10. **L'innesco da terminale**: dichiarato e muto, per scelta.
11. **Le credenziali per spazio di lavoro**: il piano del 31/08, mai costruito.
12. **La staffetta** e **la ricostruzione a caldo**: due motori interi senza porta.

---

## Appendice: i tre strumenti risultati ciechi

1. **Il grep sulle registrazioni delle azioni** ha risposto `alfa`, `zeta`,
   `spia` — nomi di prove. Le azioni vere stanno nelle costanti `*_ACTION`, e
   sono 23.
2. **Il grep sulle varianti degli enum** ha dato zero su `StepSpecies`,
   `trigger::Kind` e `CapabilityForm`, che esistono tutte e tre. La forma giusta
   è cercare gli usi (`StepSpecies::`), non la dichiarazione.
3. **Il grep sui campi di una struttura** ha dato zero su `Declaration`, che ha
   cinque campi. `grep -A30` non basta quando la documentazione sta fra un campo
   e l'altro: si legge l'indice delle righe.

Vale la regola già scritta: quando un elenco esce vuoto, prima di concludere
«non c'è» si dà allo strumento un caso che **deve** risultare positivo.
