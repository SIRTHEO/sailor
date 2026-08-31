# Sailor non possiede niente: guarda dalla finestra dei vicini

Nato dalla domanda di Theo del 29/08/2026: «quelle competenze, e in generale
tutto ciò che gira attorno — competenze, server MCP — non dovrebbero essere
installate **in** Sailor? Perché è successo?»

## Cosa è successo, misurato

**Sailor ha una casa, ed è vuota.** `~/.config/sailor` contiene un file solo, una
firma, dal 10/08/2026. Niente competenze, niente server, niente dotazione.

**Il rilevatore conosce 36 strumenti, e ogni cosa che non sia un eseguibile la
scopre leggendo la configurazione di un altro programma.** I cinque descrittori
di server MCP puntano tutti là: `~/.claude.json`, `~/.claude/settings.json`,
`~/.mcp.json`, la configurazione di Claude Desktop, quella di Cursor. Stessa cosa
per i ganci: i file di Claude Code e quelli di VS Code.

**Le competenze non esistono nemmeno come categoria.** Le famiglie censite sono
sei — `ai_cli`, `mcp_server`, `tool`, e tre di automazioni. Nessuna `skill`.

## Perché è successo, e non è distrazione

**1. Il rilevatore è nato per censire la macchina, non per equipaggiare Sailor.**
Quello lo fa bene: 391 voci d'inventario, e sono vere. Ma un inventario risponde
alla domanda «cosa c'è su questa macchina», che è un'altra domanda da «cosa può
usare un flusso». Le due sono state trattate come una sola, e da lì in poi la
dotazione di Sailor **è** la dotazione dei vicini, vista dalla finestra.

**2. Chi ha scritto i flussi girava dentro Claude Code.** Un motore che scrive un
flusso ha sotto mano le proprie competenze, e le nomina con la stessa naturalezza
con cui userebbe un comando. Non è una svista che si può correggere facendo più
attenzione: **chi scrive non percepisce il confine del proprio ambiente.** L'unica
difesa è un controllo fuori da chi scrive.

**3. Non essendoci una casa, non c'era nemmeno un posto dove sbagliare.** Non è
che qualcuno abbia scelto `~/.claude/skills` invece della cartella giusta: la
cartella giusta non esisteva.

## Cosa ne discende

**Sailor deve avere una dotazione sua**, sotto la sua casa: le competenze che i
flussi usano, i server che i flussi interrogano, i descrittori degli strumenti.
Ciò che sta lì viaggia col prodotto, si versiona, si importa insieme a un flusso,
e vale su qualunque macchina.

**Il rilevatore resta**, ma torna a fare il proprio mestiere: dire cosa c'è su
questa macchina. È utile — è così che Sailor sa che esiste `agy` — ma è
un'informazione sull'ambiente, non una dotazione.

**E fra le due nasce un terzo caso, che oggi manca.** Il controllo statico
distingue già due situazioni: uno strumento dichiarato che qui non c'è (avviso), e
un nome che nessun catalogo dichiara (errore). Ne serve un terzo:

> **c'è, ma solo perché ce l'ha un altro programma.**

Non è un errore — funziona, su questa macchina. Non è nemmeno normale — non
funzionerà per nessun altro. È **preso in prestito**, e va detto: un flusso che lo
usa non è portabile, e chi lo pubblica deve saperlo prima, non dopo.

**Le competenze diventano una famiglia come le altre.** Finché `skill` non è una
categoria che il rilevatore conosce, un passo può nominarne una e nessun controllo
può accorgersene — non perché il controllo sia debole, ma perché non c'è niente
contro cui confrontare.

## Il filo che lega questo agli altri guasti di stasera

È lo stesso di sempre, alla terza forma. Sailor non ha comandi per operare su di
sé (guasto 15), non registra ciò che spende pur avendo le tabelle pronte, e non
possiede ciò che usa. **Un sistema che vive di roba altrui non può sapere cosa
sta facendo di sé**, e infatti **nessuno** dei guasti incontrati è stato
trovato da un controllo automatico. (Un numero preciso stava qui e invecchiava:
lo si conta in `docs/guasti-incontrati.md`, che dal 31/08/2026 ha una prova che
lo tiene onesto.)
