# Coordinamento fra agenti senza un centro

**31/08/2026.** Il problema che `docs/piano-consumo-e-profili.md` lascia scoperto
sotto «PROBLEMA 4» — *«attribuzione fra più persone o più macchine: nessuna
delle undici pratiche lo tocca. Tutte contano su un unico contatore centrale»* —
non è solo un problema di contabilità. È il caso generale di cui oggi si sono
visti tre esemplari sulla stessa macchina, nello stesso giorno.

## I tre casi, che sono la specifica

1. **Sette agenti su sette alberi di lavoro della stessa repo, e nessuno sa
   degli altri.** L'unico che lo sa è chi li ha lanciati, e lo sa perché li ha
   lanciati lui.
2. **Una sessione ha committato su `sorgenti` cancellando il lavoro non
   committato di un'altra.** Una riparazione già provata, sparita. Nessuna delle
   due se n'è accorta; il lavoro è stato rifatto a mano.
3. **La stessa voce è stata numerata due volte.** Due sessioni hanno appeso in
   modo indipendente una voce a `docs/guasti-incontrati.md` leggendo lo stesso
   stato: due `27` e due `28`, cioè quattro guasti diversi su due numeri.

Il vincolo che scarta metà delle soluzioni prima di valutarle: **Sailor non ha
un centro e non deve averne uno.** Niente server sempre acceso, niente demone,
niente servizio esterno.

## Il punto d'incontro che esisteva già

`ledger::default_directory()` risponde **lo stesso percorso da qualunque albero
di lavoro** — su questa macchina `~/.claude/state/flussi` — ed è SQLite in
modalità WAL con un tempo di attesa dichiarato. Più processi lo aprono
direttamente: nessuno di loro è «il server», e chi non c'è non tiene niente. Il
punto d'incontro senza centro non andava costruito, andava riconosciuto.

Nel linguaggio della fonte più vecchia di questo rapporto, il deposito è un
**tuple space povero**, e il povero qui basta.

---

# 1. La ricerca

Distinguo ciò che ho aperto da ciò che non si è aperto. Le fonti sotto sono
state scaricate e lette; dove una pagina ha risposto 403, 404 o ha reso un testo
troncato, lo dico e indico per quale via ho recuperato il contenuto.

## 1.1 Presenza e annuncio con scadenza

### Il lease, e perché il termine è il vero parametro di progetto

**Gray & Cheriton, *Leases* (SOSP 1989)** — aperto, PDF scaricato ed estratto da
`https://web.stanford.edu/class/cs240/readings/leases.pdf`. (`.../89-leases.pdf`
dà 404.)

> «A lease is a contract that gives its holder specified rights over property for
> a limited period of time.»

E la riga che fissa il criterio di scelta del termine:

> «Short lease terms have several advantages. One is that they minimize the delay
> resulting from client and server failures… If some host holding a lease for
> this file is unreachable, the delay continues until the lease expires.»

**Il TTL è esattamente il tempo massimo in cui l'annuncio di un morto resta
appeso.** Non è un parametro di prestazione: è la misura del difetto principale
di questa famiglia di sistemi.

**Kubernetes `coordination.k8s.io/Lease`** — aperto,
`https://kubernetes.io/docs/concepts/architecture/leases/`:

> «Under the hood, every kubelet heartbeat is an **update** request to this
> `Lease` object, updating the `spec.renewTime` field… The Kubernetes control
> plane uses the time stamp of this field to determine the availability of this
> `Node`.»

Il *concetto* regge senza centro (è un timestamp più una durata). L'*attuazione*
K8s no: vuole un apiserver, cioè un orologio autorevole e scritture
compare-and-swap. **Preso il concetto, scartata l'attuazione.**

### La scadenza che il kernel garantisce: `flock(2)`

**`man 2 flock` su questa macchina** (macOS 25.6), letto in locale:

> «Locks are on files, not file descriptors… If a process holding a lock on a
> file forks and the child explicitly unlocks the file, the parent will lose its
> lock.»

E `https://man7.org/linux/man-pages/man2/flock.2.html`, aperta:

> «the lock is released either by an explicit LOCK_UN operation on any of these
> duplicate file descriptors, or when all such file descriptors have been
> closed.»

**Misurato su questa macchina, non citato a memoria.** Un detentore prende
`LOCK_EX|LOCK_NB`, scrive il proprio pid nel file, viene ucciso con `-9`:

```
--- lock provato mentre il detentore e' vivo:
BUSY: [Errno 35] Resource temporarily unavailable -> pid=62936
--- dopo kill -9 (SIGKILL, nessun handler possibile):
FREE: lock acquired -> pid=62936
```

**Il risultato che conta: dopo `SIGKILL` il lock è libero, mentre il pid scritto
nel file è rimasto lì, falso.** Il kernel garantisce la scadenza; il byte sul
disco no. È l'unica primitiva in cui la scadenza non dipende da codice nostro —
e con ~70 sonni al giorno, qualunque cosa dipenda da un `atexit` o da un signal
handler è già rotta in partenza.

**Scartato `fcntl` / record lock POSIX**, con le parole del man page di macOS:

> «This interface follows the completely stupid semantics of System V and IEEE
> Std 1003.1-1988 ("POSIX.1") that require that all locks associated with a file
> for a given process are removed when *any* file descriptor for that file is
> closed by that process.»
> «Flock(2) is recommended for applications that want to ensure the integrity of
> their locks»

Una qualunque libreria che apre e chiude lo stesso file farebbe evaporare il
lock.

### `O_EXCL` alla git: il precedente che mostra il difetto invece della cura

**`https://raw.githubusercontent.com/git/git/master/lockfile.h`**, aperto:

> «We create the `<filename>.lock` file with `O_CREAT|O_EXCL` so that we can
> notice and fail if somebody else has already locked the file…»
> «**Automatic cruft removal.** If the program exits after we lock a file but
> before the changes have been committed, we want to make sure that we remove
> the lockfile. This is done by… setting up an `atexit(3)` handler and a signal
> handler that clean up the lockfiles.»

**`SIGKILL` non è intercettabile**, quindi `index.lock` resta appeso per sempre:
è il difetto del mandato, in casa di git. E git lo sa — `core.lockfilePid`,
aperto su `Documentation/config/core.adoc`, si limita a **diagnosticare**:

> «Git can provide additional diagnostic information about the process holding
> the lock, including whether it is still running.»

con il messaggio, letto in `lockfile.c`: *«the lock file may be stale (PIDs can
be reused)»*.

**Il caso opposto, dentro lo stesso repo**, è la cosa più vicina a ciò che serve
— `lock_repo_for_gc` in `builtin/gc.c`, aperto:

```c
time(NULL) - st.st_mtime <= 12 * 3600 &&
fscanf(fp, scan_fmt, &pid, locking_host) == 2 &&
(strcmp(locking_host, my_host) || !kill(pid, 0) || errno == EPERM);
```

**Tre condizioni insieme: TTL sull'`mtime`, hostname, e `kill(pid,0)`.** Non
l'una o l'altra.

### Gli editor, che hanno pagato questo prezzo per trent'anni

**Emacs**, `https://www.gnu.org/software/emacs/manual/html_node/elisp/File-Locks.html`
e il sorgente `src/filelock.c`, entrambi aperti. Il lock è un symlink `.#FN` che
punta a `user@host.pid:boot`, e il commento nel sorgente dice perché:

> «This avoids a single mount (== failure) point for lock files.»

**«Nessun centro», detto trentacinque anni fa.** La decisione di staleness,
righe 486-495:

```c
if (VALID_PROCESS_ID (pid)
    && ! (kill (pid, 0) < 0 && errno != EPERM)
    && (boot_time == 0 || within_one_second (boot_time, get_boot_sec ())))
  return ANOTHER_OWNS_IT;
```

Il `boot_time` serve contro il pid riciclato — ma è grossolano: dopo un riavvio
*tutti* i lock diventano stale in blocco. **E su questa macchina non è nemmeno
disponibile**: misurato, `kern.boottime` risponde `EPERM` dentro il perimetro,
mentre `sysctl(KERN_PROC_PID)` passa e dà `p_starttime` al microsecondo. Quello è
strettamente migliore: distingue due processi con lo stesso pid nello stesso
avvio.

**Vim**, `https://vimhelp.org/usr_11.txt.html`, aperto — il messaggio `E325:
ATTENTION` porta `process ID: 12559 (still running)`, calcolato al momento della
lettura, e offre `[O]pen Read-Only / [E]dit anyway / [R]ecover / [Q]uit`.
**Vim non decide: chiede.** È il modello giusto quando il verdetto è incerto, ed
è la lezione di progetto più utile delle due.

### `utmp`: il precedente esatto, fantasmi compresi

`https://man7.org/linux/man-pages/man5/utmp.5.html`, aperta:

> «The utmp file allows one to discover information about who is currently using
> the system.»

`who(1)` ha l'opzione che tradisce il problema: `-d, --dead print dead
processes`. La causa dei fantasmi, dal thread di patch di coreutils
(`https://bug-coreutils.gnu.narkive.com/YCY45Lg8/patch-who-and-stale-utmp-entries`,
aperto): *«what happened was that I reset the machine during a root login
session»*. **La voce di uscita la scrive chi esce; se non esce, non la scrive
nessuno.** Il file registra un'intenzione, non un fatto.

E la cura, che è la riga più utile di tutta la ricerca:

> «All the patch does is claim that **a utmp entry referencing a process no
> longer running is stale and should not be used.**»

### Scartato: gossip e mDNS

**SWIM** (Das, Gupta, Motivala — `https://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf`,
scaricato ed estratto) e **Serf** (`https://github.com/hashicorp/serf/blob/master/docs/internals/gossip.html.markdown`,
aperto; `serf.io` fa 302). SWIM regge il vincolo «nessun centro» — ma risolve un
problema che **qui non esiste**, e lo dice da solo:

> «The asynchrony and unreliability of the underlying network can cause messages
> to be lost, leading to false detection of process failures, since a process
> that is losing messages is indistinguishable from one that has failed»

Sette processi sulla stessa macchina non hanno rete. Il kernel sa già con
certezza chi è vivo: pagare un protocollo probabilistico, con un socket UDP e un
thread di sfondo in ciascuno dei sette, per riscoprirlo, è **reinventare il TTL a
caro prezzo**. **mDNS** (`https://www.rfc-editor.org/rfc/rfc6762.html`, aperta) è
peggio: su macOS richiede `mDNSResponder`, cioè un demone — il centro che il
vincolo esclude, anche se lo fornisce il sistema.

## 1.2 Rilevare una scrittura concorrente, e numerare senza coordinatore

### Perché il `27` non poteva che raddoppiarsi

**RFC 9562 (UUID), §2 e §6.4** — l'HTML si tronca, letto il `.txt` normativo su
`https://www.rfc-editor.org/rfc/rfc9562.txt`:

> «"auto-increment" schemes that are often used by databases do not work well:
> the effort required to coordinate sequential numeric identifiers across a
> network can easily become a burden.»
> «**Centralized Registry**: With this method, all nodes tasked with creating
> UUIDs consult a central registry… Shared knowledge schemes with central/global
> registries are outside the scope of this specification and are **NOT
> RECOMMENDED**.»

**ULID** (`https://github.com/ulid/spec`, aperta): 48 bit di timestamp più 80 di
casualità, *«Lexicographically sortable!»*, 26 caratteri. Attenzione a un
dettaglio che si legge male: la monotonia dichiarata — *«if the same millisecond
is detected, the `random` component is incremented by 1 bit»* — è di **una
factory in un solo processo**, non fra processi.

**Cosa si perde passando da `27` a un ULID**: la leggibilità e la citabilità a
voce, e la cardinalità visibile («siamo al 27» non si legge più dall'ID). **Non**
si perde l'ordinamento: è il punto di ULID e UUIDv7.

### Rilevare invece di prevenire

**RFC 9110 §13.1.1 (`If-Match`)** — HTML troncato, letto il `.txt`. Descrive
letteralmente il caso 3:

> «multiple user agents writing to a common resource as a semaphore (e.g., a
> nonatomic increment) are likely to collide and potentially lose important
> state transitions.»
> «If-Match is most often used with state-changing methods… to prevent accidental
> overwrites when multiple user agents might be acting in parallel on the same
> resource (i.e., to prevent the "lost update" problem).»

Senza centro il `412` diventa: leggi il file, tieni l'hash, riscrivi con
`rename(2)` atomico **solo se** l'hash su disco è ancora quello. Non serve un
server, serve il kernel.

**CouchDB**, `https://docs.couchdb.org/en/stable/replication/conflicts.html`,
aperta — e qui la lezione è nel *secondo* comportamento, non nel primo:

> «CouchDB picks one arbitrary revision as the "winner", using a deterministic
> algorithm… If you do `GET /db/test?conflicts=true`… you will get the winner
> plus a `_conflicts` member containing an array of the revs of the other,
> conflicting revision(s).»
> «It could look as if the changes she made there have been lost - but of course
> they have not, they have just been hidden away as a conflicting revision.»

Il vincitore deterministico **nasconde**; il valore sta nel conflitto reso
**oggetto interrogabile**.

**Syncthing**, `https://docs.syncthing.net/users/syncing.html`, aperta — la
stessa scelta, portata all'estremo e con la ragione scritta:

> «one of the files will be renamed to
> `<filename>.sync-conflict-<date>-<time>-<modifiedBy>.<ext>`»
> «we don't know which of the conflicting files is the "best" from the user point
> of view.»

**Non fondono. Producono un oggetto in più, brutto da vedere, che nessuno può
non vedere.** È la forma più economica di visibilità: nessun database, un file
accanto all'originale.

**Dynamo (SOSP 2007)** — il PDF non si è estratto via fetch; letto come immagini
delle pagine 209-210 di
`https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf`. §4.4:

> «If the counters on the first object's clock are less-than-or-equal to all of
> the nodes in the second clock, then the first is an ancestor of the second and
> can be forgotten. Otherwise, the two changes are considered to be in conflict
> and require reconciliation.»

È il passo successivo, se un giorno serve distinguere *«discende da»* da *«è
divergente»* invece di limitarsi a *«è cambiato»*. Non serve oggi.

### Scartato: i CRDT

**Automerge** (`https://automerge.org/docs/reference/documents/`) e **Yjs**
(`https://github.com/yjs/yjs`), aperti. Yjs: *«changes are automatically
distributed to other peers and merged without merge conflicts»*. Due ragioni per
scartarli, e la seconda è dirimente:

1. **Il costo.** Il dato deve vivere dentro la struttura CRDT e ogni modifica
   deve passare dalle sue API (`Automerge.splice`). Un `docs/guasti-incontrati.md`
   scritto a mano — o modificato da un agente con `Edit` — non è un documento
   Automerge. Il file su disco diventerebbe un blob binario: si perde
   `git diff`, cioè la revisione.
2. **Risolvono il problema sbagliato.** Un CRDT **fonde senza conflitto**, cioè
   fa sparire il segnale. Qui non vogliamo la fusione automatica: vogliamo il
   rilevamento e la visibilità. E nessun CRDT assegna etichette sequenziali
   dense e distinte senza coordinamento — per la stessa ragione della RFC 9562.

## 1.3 Protocolli fra agenti

### A2A: scartato, tranne la forma

`https://a2a-protocol.org/latest/specification/` e
`.../topics/agent-discovery/`, aperte. La Agent Card è *«A JSON metadata document
published by an A2A Server, describing its identity, capabilities, skills,
service endpoint…»*. Le tre strategie di discovery: **Well-Known URI** («*the
client agent knows or programmatically discovers the domain*» — cioè presuppone
già di sapere chi cercare), **Curated Registries** («*An intermediary service
(the registry)*» — un centro esplicito), **Direct Configuration** (una lista a
mano che nessuno aggiorna quando un agente muore).

Tutti e tre i binding sono di rete e l'Agent Card porta un `service endpoint`:
**in pratica ogni agente deve stare in ascolto su una porta.** Sette listener
HTTP effimeri per parlare fra vicini di casa è il costo massimo per il beneficio
minimo. **Tenuta solo la forma del payload**: identità, capacità, cosa sto
facendo, come JSON.

### Linda: la fonte che descrive quello che il deposito già è

**Gelernter, *Generative communication in Linda*, ACM TOPLAS 7(1), 1985.** Il PDF
ACM (`https://dl.acm.org/doi/pdf/10.1145/2363.2433`) risponde **403**; il paper
integrale è stato letto da
`https://www.cs.unc.edu/~stotts/COMP590-059-f21/slides/lindaGenerative.pdf`
(piè di pagina TOPLAS verificati).

> «A tuple in TS is equally accessible to all processes within TS, but is bound
> to none.»
> «**p3. Distributed sharing.** Linda allows j address-space-disjoint processes
> to share some variable v by depositing it in TS… **It is not necessary (as in
> other languages) that a shared variable be implemented by a process or
> module.**»
> «**p2. Time uncoupling.** A tuple added to TS by `out( )` remains in TS until
> it is removed by `in( )`… process A… may run to completion before process B…
> is loaded.»

Il costo, dichiarato dall'autore: *«The maintenance of a central directory might
lead to congestion and vulnerability; it is furthermore inappropriate as a kernel
function»*, e la scelta fra le due attuazioni dipende dal rapporto
letture/scritture. **Su una macchina sola il caso è degenere: lettura e scrittura
costano una riga di SQLite.**

**JavaSpaces** (`https://river.apache.org/release-doc/current/specs/html/js-spec.html`,
aperta; il PDF Sun non si è decodificato) porta il pezzo che a Linda manca:

> «Entries written into a JavaSpaces service are governed by a lease» … che serve
> a «keep the space free of debris left behind due to system crashes and network
> failures.»

**`lease`, non `write` e basta.** L'attuazione JavaSpaces è un servizio Jini e va
scartata; il modello no.

### MCP: giusto come superficie, non come trasporto fra agenti

`https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio`,
aperta:

> «In the **stdio** transport, the client launches the MCP server as a
> subprocess.»
> «Servers **SHOULD** exit promptly when their standard input is closed…»

**Nessun demone**: il server nasce e muore col client. Ma la topologia è a
stella: un server MCP notifica *il proprio client*, non gli altri sei agenti. È
la superficie giusta per esporre l'interrogazione, non il canale fra pari.

### Quello che c'era già, e che non va rifatto

**Claude Code ha già questa architettura, ed è viva su questa macchina.**
`https://code.claude.com/docs/en/cross-session-messaging`, aperta:

> «**Each session registers itself in files on disk.** When Claude lists or
> messages your local sessions, Claude Code reads those files to find the
> sessions, so two sessions can reach each other only when they can see the same
> files.»
> «On this machine → Over a per-session socket on macOS and Linux… **never
> through Anthropic servers**.»

Registro su file più un socket per processo, zero demoni: è un `utmp` per agenti
con la buca delle lettere. **Copre però solo gli agenti Claude Code.** Codex e
Gemini restano fuori, e sono il motivo per cui Sailor deve fare la stessa cosa un
piano più in basso, dove i tre motori sono uguali.

**Agent teams** (`https://code.claude.com/docs/en/agent-teams`) fa lo stesso a
mano — mailbox JSON sotto `~/.claude/teams/…`, *«Task claiming uses file locking
to prevent race conditions»* — ma con un limite che qui è fatale: *«One team per
session»*, *«Lead is fixed»*. È una stella con un capo; i sette sono pari e nati
indipendenti.

**Aider non si vede, ed è un bug aperto**: `https://github.com/Aider-AI/aider/issues/302`,
*«Support multiple instances of aider, for parallel workflow»*, **aperto**,
nessuna risposta del manutentore. Le tre cure che l'autore propone sono le
nostre, e nessuna è stata attuata.

**Claude Code, sui worktree, dichiara l'isolamento come una virtù e si ferma
lì** — `https://code.claude.com/docs/en/worktrees`:

> «Running each Claude Code session in its own worktree means edits in one
> session never touch files in another…»

Vero per i file. **Non vero per `.git`**, che è condiviso, ed è da lì che è
passato il caso 2.

### `.git` è già condiviso, e questo produce una scoperta

`https://git-scm.com/docs/git-worktree`, aperta:

> «The new worktree is linked to the current repository, **sharing everything
> except per-worktree files such as `HEAD`, `index`, etc.**»
> «refs are shared across all worktrees, except `refs/bisect`, `refs/worktree`
> and `refs/rewritten`.»

E `https://git-scm.com/docs/git-update-ref` dà il compare-and-swap atomico che a
un file su disco manca:

> «stores the `<new-oid>` in the `<ref>`… after verifying that the current value
> of the `<ref>` matches `<old-oid>`.»
> «If all `<ref>`s can be locked with matching `<old-oid>`s simultaneously, all
> modifications are performed. Otherwise, no modifications are performed.»

**La scoperta, che non era nel mandato e vale da sola. E non è una citazione: è
misurata su questa macchina.** `refs/stash` **non** è fra le eccezioni
per-worktree, quindi la pila degli stash è **una sola per tutti e sette gli
alberi di lavoro**. Chiesto a git, da dentro il mio albero:

```
$ git -C .../sailor-worktrees/coordination rev-parse --git-path HEAD
/Users/theo/personal/sailor/.git/worktrees/coordination/HEAD    <- mio

$ git -C .../sailor-worktrees/coordination rev-parse --git-path refs/stash
/Users/theo/personal/sailor/.git/refs/stash                     <- di tutti
```

`git stash` di una sessione mette il proprio lavoro in una pila da cui un'altra
può fare `pop`. È il modo più veloce che ha una sessione per far sparire il
lavoro di un'altra senza che nessuna delle due se ne accorga — **una spiegazione
candidata del caso 2**, che finora non aveva causa. La memoria di casa dice già
che `lint-staged` fa `git stash` globale: qui si vede perché quello è pericoloso
in un albero condiviso, e non solo scomodo.

**Cosa git *ha* e non guarda mai da solo.** Un solo avviso fra worktree, e sul
ramo, non sui file:

> «By default, `add` refuses to create a new worktree when `<commit-ish>` is a
> branch name and is already checked out by another worktree»

Nient'altro: `git status` non sa nulla degli altri alberi; `git worktree lock`
non è un lock di concorrenza (*«If a worktree is on a portable device or network
share which is not always mounted, lock it to prevent its administrative files
from being pruned»*); `core.checkStat` dice solo «questo file è cambiato rispetto
al mio index», senza **chi** né **rispetto a te**. **`git worktree list` è
l'unico censimento, e va interrogato: non parla da solo.**

**Uso documentato di ref custom `refs/sailor/…` per metadati fra worktree: non
ne ho trovato uno**, e lo dichiaro. La meccanica è documentata; la convenzione
sarebbe nostra, e andrebbe difesa con una prova, non con una citazione.

## 1.4 Cosa la ricerca non ha prodotto — che è un risultato

- **Nessuno risolve il problema 4 di `piano-consumo-e-profili.md` senza un
  centro.** La ricerca conferma il buco invece di chiuderlo: `litellm` sta su
  Redis, A2A su un registro o sul DNS, K8s su un apiserver. Ciò che si trova
  senza centro (lease, flock, tuple space, version vector) risolve **presenza e
  conflitto**, non **somma di consumi fra macchine**. Quella resta aperta.
- **Nessuna fonte affronta il congelamento da sonno di sistema.** Tutte
  presuppongono che un processo o risponda o sia morto. Un processo congelato
  otto ore e poi vivo non compare in nessuna delle undici fonti aperte, ed è il
  caso normale su questa macchina.
- **Nessuna fonte dà un identificativo sequenziale denso senza coordinamento.**
  La RFC 9562 lo dichiara impossibile e lo marca `NOT RECOMMENDED`. Quindi il
  «guasto 27» non si salva: o si accetta un identificativo lungo, o si accetta
  che il numero venga assegnato a valle da chi rigenera l'indice.

---

# 2. Il progetto, con le domande decise

## 2.1 Cosa annuncia un agente, e quando

**Annuncia una rivendicazione, e la rinnova.** Chi (nome, più il proprio pid),
dove (repo, albero di lavoro, ramo, elenco di percorsi), cosa sta facendo, e
**fino a quando**.

**Perché un lease e non un annuncio più un rilascio.** Un rilascio è una promessa
che il morto non può mantenere, e su questa macchina i processi muoiono male. Il
rinnovo è **l'unica promessa che un processo morto non può fingere di
mantenere** — è la ragione per cui `utmp` lascia fantasmi e JavaSpaces no.

**Il rilascio esiste comunque, e resta distinto dalla scadenza.** Chi finisce
alle 10:00 non deve trattenere fino alle 10:15. E `released` («qualcuno ha
guardato e ha finito») non si confonde con `expired` («nessuno si è più fatto
vivo»): è la stessa distinzione che `docs/decisioni.md` impone alle capacità di
uno strumento, dove *«scrivere `false` non è la stessa cosa che tacere»*. Nel
primo caso il lavoro è finito, nel secondo è a metà e nessuno lo sa.

**Il termine predefinito è 15 minuti**, e chi vuole altro lo dichiara. Il criterio
è quello di Gray & Cheriton: il termine *è* il ritardo massimo con cui l'annuncio
di un morto sparisce, e va scelto contro il costo del rinnovo — non contro
nient'altro.

## 2.2 Come si accorge di una collisione, e cosa fa

**Si accorge rileggendo, subito dopo aver scritto.** L'ordine non è indifferente:
guardando prima di scrivere, due agenti che partono insieme leggerebbero
entrambi un deposito che non contiene ancora l'altro e concluderebbero «sono
solo». Scrivendo prima, **chi arriva secondo vede sempre il primo**, e nel caso
peggiore si vedono tutti e due — che è l'errore dalla parte giusta.

**Le collisioni hanno tre specie, perché i casi vissuti hanno due gravità.**

| specie | cosa vuol dire | il caso vissuto |
|---|---|---|
| `same_paths` | stesso albero, percorsi che si toccano — o uno dei due li ha presi tutti | il caso 2 |
| `same_workdir` | stesso albero, percorsi dichiarati e disgiunti | il caso 2 sotto un'altra forma: `git commit` non guarda i percorsi che qualcuno ha dichiarato |
| `same_repository` | altro albero, stessa repo | il caso 1 |

**Perché tre e non una.** Sette agenti condividono *sempre* la repo: se
`same_repository` valesse quanto il resto, ogni annuncio sarebbe una collisione,
e un allarme che suona sempre è un allarme che qualcuno spegne il primo giorno.

**Cosa fa: avvisa sempre, si ferma solo dove il flusso l'ha chiesto.**
L'annuncio riesce e porta con sé l'elenco di chi c'è; il flusso ha un ramo, come
ce l'ha su `found: false` di `store_read`. Con `refuse_when_shared` dichiarato,
il passo fallisce con classe `work_is_shared` nominando chi teneva — **ma mai su
`same_repository`**. È la forma che `docs/decisioni.md` già impone al modello
Bazel: *«Il controllo entra come avviso e diventa barriera solo con un cambio di
configurazione, dopo averlo visto funzionare»*.

**E c'è una ragione tecnica che vieta di andare oltre.** Il deposito non offre un
compare-and-swap, e non posso aggiungerne uno: **una serratura che ogni tanto
lascia passare tutti e due è peggio di nessuna serratura, perché qualcuno ci
crede.** Quello che questo nodo garantisce — che nessun agente cancelli mai la
riga di un altro — lo garantisce *per costruzione*, non per esclusione mutua.

## 2.3 Chi può leggere

**Un agente, non solo la finestra.** L'interrogazione è un'azione registrata
(`work_survey`): qualunque flusso può scriverla, quindi qualunque agente che
esegua un flusso può farla. Il dato vive in una collezione ordinaria del
deposito, quindi lo legge anche `store_list`, e la finestra dopo di loro.

È il primo pezzo della voce di `docs/da-fare.md` che dice *«Ricerca su tutto ciò
che il sistema conserva… dalla finestra e da qualunque agente. Come nodo di
sistema, non come funzione della finestra»* — mai iniziata finora.

## 2.4 Cosa succede quando l'agente muore male

**La scadenza, e nient'altro.** Il pid viene registrato perché una persona possa
controllare, **e non è l'oracolo**: il guasto 12 dice che dentro il perimetro
`pgrep` non vede i processi e risponde vuoto **senza errore**, e la cura scritta
accanto è «chiedere lo stato al deposito, non al sistema operativo». Un secondo
oracolo che risponde «non ho potuto guardare» travestito da «non c'è» rifarebbe
quel guasto.

**Il limite di questa scelta, dichiarato.** Durante il sonno di sistema un
processo è **congelato ma vivo**, e l'orologio a muro avanza: al risveglio la sua
scadenza è passata mentre lui non è morto. Per questo il censimento non dice mai
«morto»: dice `expired`, e chi legge ha `renewed_at` accanto per distinguere
trenta secondi di silenzio da otto ore.

**La misura autorevole esiste, ed è `flock(2)`** — misurata sopra: il kernel
rilascia il lock anche dopo `SIGKILL`. **Non è stata costruita**, e la ragione è
dichiarata: vuole o una dipendenza nuova nel workspace, che il `Cargo.toml` di
casa tiene al minimo per scelta scritta, o del codice `unsafe` in un crate che
non ne ha. Va aggiunta come **secondo strato autorevole sopra il lease, non al
suo posto**: `flock` sa se un processo esiste, non se sta lavorando — e un agente
appeso su un prompt tiene il lock all'infinito.

---

# 3. Quello che è costruito

`crates/actions/src/presence.rs`, tre nodi registrati nel registro condiviso:

- **`work_claim`** — annuncia o rinnova, e restituisce chi altro c'è.
- **`work_release`** — smette di trattenere subito, restando distinguibile da una
  scadenza.
- **`work_survey`** — chi lavora e chi non c'è più, con il perché.

**Perché questo pezzo e non un altro.** È il pezzo che rende visibile il caso 2 —
quello che è costato lavoro vero — ed è **esattamente** il caso 1: sette agenti
che non si vedono smettono di non vedersi quando c'è un posto dove guardare. Il
caso 3 non è chiuso, ma la sua lezione è dentro il meccanismo, ed è la scelta
centrale del progetto:

> **La chiave porta il processo.** Un annuncio sta sotto `<agente>#<pid>`, quindi
> **nessun agente scrive mai la riga di un altro**. Due che scrivono nello stesso
> posto si sovrascrivono; due che scrivono ciascuno nel proprio si sommano. È il
> doppio `27` letto al contrario, e senza il `#<pid>` due agenti che scegliessero
> lo stesso nome ricadrebbero nel difetto contro cui il modulo è scritto.

## Come si usa, da un flusso

```json
{
  "id": "annuncia",
  "deps": [],
  "action": "work_claim",
  "max_attempts": 1,
  "when": null,
  "with": {
    "agent": "chi-sono",
    "repository": "/Users/theo/personal/sailor/.git",
    "workdir": "/Users/theo/personal/sailor",
    "branch": "sorgenti",
    "paths": ["crates/actions"],
    "doing": "cosa sto facendo",
    "lease_seconds": 900,
    "refuse_when_shared": false
  },
  "input_schema": { "type": "any" },
  "output_schema": { "type": "any" }
}
```

## Cosa resta scoperto

1. **Il secondo strato `flock`**, con la ragione dichiarata sopra. Senza, un
   agente congelato dal sonno appare `expired` pur essendo vivo.
2. **`refs/stash` condiviso fra i sette alberi.** Misurato qui, **non riparato** e
   non verificato *sul* caso vissuto: so che il canale è aperto, non che il
   lavoro sia passato di lì. Chi lo vuole chiudere ha due strade — vietare
   `git stash` agli agenti, o dare a ciascuno un riferimento suo.
3. **Nessuno chiama ancora questi nodi.** Esistono, sono registrati e si vedono
   funzionare da un flusso; ma finché il flusso di sviluppo non annuncia da sé,
   restano una capacità e non una difesa. È lo stesso stato dichiarato in
   `docs/decisioni.md` per `needs_capabilities`: il vocabolario esiste, l'uso no.
4. **Il caso 3 non è chiuso.** Il rimedio che la ricerca indica — una voce, un
   file, con nome ULID, e l'indice generato — riguarda `docs/`, non questo crate.
5. **Il punto di contatto con il guasto 4** (il registro dei processi che Sailor
   avvia, in costruzione altrove): quello sa *quali processi ho lanciato*, questo
   sa *chi dichiara di lavorare su cosa*, **compresi gli agenti che Sailor non ha
   lanciato**. Si incontrano sul pid: un annuncio il cui pid compare in quel
   registro è verificabile; uno il cui pid non c'è è un agente esterno, e resta
   affidato al solo lease. Non sono stati uniti.
6. **`piano-consumo-e-profili.md` PROBLEMA 4 resta aperto** per la parte di
   contabilità: nessuna fonte somma consumi fra macchine senza un centro.
