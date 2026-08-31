# I guasti incontrati mentre si costruiva

Ogni riga qui è un guasto **vero**, capitato davvero, con la data. Nessuno è
ipotetico e nessuno è stato scritto per fare esempio.

**A cosa serve questo file, e perché non è un diario.** Un guasto riparato a mano
resta nella testa di chi l'ha riparato. La volta dopo ci ricasca un altro — è
successo il 29/08/2026 con lo stesso processo orfano, a due persone diverse,
nella stessa notte, e la seconda non sapeva della prima. Questo file è il
materiale grezzo di ciò che Sailor deve imparare a fare da solo: leggere com'è
andata e ricavarne un controllo. Finché quell'anello non è chiuso, la lista si
tiene a mano.

La forma di ogni voce viene da un lavoro che il flusso di ricerca ha trovato in
letteratura il 29/08/2026 — *Sillito & Kutomi, «Failures and Fixes: A Study of
Software System Incident Response»* — e dalla sua conclusione, che qui è la
regola: **il seguito di un guasto è un controllo, non un compito assegnato a
qualcuno.** Una voce senza la colonna «cosa lo impedirebbe» non è finita.

| # | data | cosa è successo | come si è visto | cosa lo impedirebbe | stato |
|---|---|---|---|---|---|
| 1 | 28/08 | Un motore esterno invocato con gli argomenti nell'ordine sbagliato: il prompt finiva ignorato | Solo eseguendolo. Le opzioni erano state lette dalla documentazione e mai provate | Una prova che esegue davvero ogni riga di comando prima che finisca in un flusso | **chiuso** — riga corretta e misurata |
| 2 | 28/08 | Un passo che falliva veniva registrato come riuscito, col codice d'errore sepolto nel risultato | Leggendo il deposito a mano | Un'uscita non-zero rompe il passo, con tolleranza da chiedere esplicitamente | **chiuso** — con mutante |
| 3 | 28/08 | Il controllo statico chiudeva senza errori su un flusso che nominava uno strumento inesistente | Provando a mano un flusso rotto | Il controllo distingue «non c'è qui» da «non esiste in nessun catalogo» | **chiuso** — con mutante |
| 4 | 29/08 | Un processo di sviluppo orfano occupava una porta e impediva l'avvio | Due volte, a due persone diverse, nella stessa notte | Sailor deve sapere quali processi ha avviato, per spegnerli e riprenderli | **aperto** |
| 5 | 28/08 | Prove che leggono un file di configurazione della macchina di chi le esegue | Diventate rosse dopo una pulizia, a codice invariato | Le prove non leggono lo stato della macchina; ciò che serve si versiona | **aperto** |
| 6 | 29/08 | Uno script ha stampato «rimossi 1975 file» senza rimuoverne nessuno | Guardando che il peso della cartella non era cambiato | Il messaggio di successo sta dentro il controllo dell'esito, mai accanto | **chiuso** — rifatto |
| 7 | 28/08 | Due passi dichiarati «in parallelo» giravano in fila | Cronometrando: due passi da 6 secondi ne impiegavano 12 | Una prova in cui ogni passo aspetta gli altri del fronte e fallisce se resta solo: non cronometra, quindi non mente su una macchina carica | **chiuso** il 30/08 — misurato: 2 passi da 6s in 6,07s (erano 12,07), 3 in 6,05s (erano 18,14) |
| 8 | 28/08 | Un descrittore con un campo che questa versione non conosce viene scartato intero | Scrivendone uno apposta | Un campo ignoto è un avviso sul campo, non il rifiuto del componente | **aperto** |
| 9 | 28/08 | Un elemento grafico invisibile: disegnato dello stesso colore dello sfondo | Solo da un'immagine dello schermo. Né i tipi né le prove lo vedevano | Lo schermo come oracolo: un'immagine confrontata, non solo i tipi | **chiuso** — quel caso |
| 10 | 28/08 | La stessa lista di componenti scritta in due punti del programma | Si sono disallineati in un'ora, nello stesso giorno di lavoro | Una sola fonte, e le altre la chiedono invece di ricopiarla | **aperto** |
| 11 | 29/08 | In modalità viva, un errore di compilazione in un crate qualunque **uccide la finestra** invece di lasciarla all'ultima versione buona | La finestra è sparita mentre un altro cantiere aveva un crate a metà | La finestra sopravvive a una compilazione fallita, e lo dice invece di chiudersi | **aperto** |
| 12 | 29/08 | `pgrep` dentro il perimetro **non vede i processi e risponde vuoto**, senza errore | Una sorveglianza ha detto «nessun flusso in esecuzione» mentre due giravano | Chiedere lo stato al deposito, non al sistema operativo: chi è vivo lo dice il dato, non un comando che il perimetro può zittire | **chiuso** — sorveglianza riscritta |
| 13 | 29/08 | `for x in $lista` in zsh **non spezza la variabile in parole**: l'intera lista entra in un giro solo | La sorveglianza riscritta ha rifatto lo stesso falso allarme del guasto 12, per una causa diversa | Leggere riga per riga (`while read`), mai iterare una variabile non quotata. **Era già scritto in una memoria e non è stato consultato** | **chiuso** — riscritta di nuovo |
| 14 | 29/08 | Un motore esaurito (limite settimanale) è stato registrato come guasto qualunque, e la corsa si è fermata | Leggendo il messaggio del motore: «hai raggiunto il limite settimanale, si azzera alle 7» | Distinguere «finito fino a un'ora nota» da «rotto»: il primo si aspetta o si instrada su un altro profilo, il secondo no. Serve prima che il consumo dichiarato dal motore entri nel deposito | **chiuso in parte** il 31/08 — con mutante. La distinzione c'è ovunque: classe `engine_exhausted` e specie `exhausted` nel deposito, **anche con un motore solo**, che era il caso in cui è capitato. **Resta l'attesa**: nessuno legge «si azzera alle 7» per riprovare a quell'ora, e non si legge apposta — estrarre quell'ora con un'espressione regolare da una frase vista una volta sola sarebbe un dato inventato che ha l'aria di una misura |
| 15 | 29/08 | Per cambiare l'innesco di un flusso è stato usato uno script Python che riscrive il JSON: Sailor non ha nessun comando per operare sui propri flussi | Theo lo ha visto nel terminale e ha chiesto perché la riga di comando non chiami Sailor. Verificato: `sailor flow` ha solo `list`, `due`, `check`, `run` | Ogni cosa che una persona deve fare su un flusso è un comando di Sailor. Finché si aggira con `python3`, il sistema non si usa da sé e nessuno se ne accorge dai suoi controlli | **aperto** |
| 16 | 29/08 | Il flusso di ricerca non è partito e il lavoro l'ha fatto una persona a mano. Tutti e sei i passi hanno `claude-code` scritto dentro, senza ripiego — mentre `agy` rispondeva | Provando i tre motori uno per uno: claude esaurito, codex 401, **agy vivo**. Nessuno li aveva provati | Un passo dichiara una **catena** di motori, non uno. E chi lancia vede su quale è finito | **aperto** |
| 17 | 29/08 | Due passi di quel flusso chiedono competenze di Claude Code (`advise-project-approach`, `neuroarxiv`) che stanno in `~/.claude/skills` di una sola persona: il flusso non è portabile e non lo dichiara | Cercando perché non bastasse cambiare motore. `flow check` lo dà per buono: controlla gli strumenti per identificativo, **le competenze no** | Ciò che un passo chiede va dichiarato tutto, e il controllo statico lo verifica — è il guasto 3 su un'altra dimensione. Chi non ha quella capacità deve funzionare peggio, non in silenzio | **aperto** |
| 18 | 29/08 | La casa di Sailor (`~/.config/sailor`) contiene **un solo file**, una firma. Ogni server MCP e ogni gancio che Sailor conosce li scopre leggendo la configurazione di Claude Code, Claude Desktop o Cursor: la sua dotazione è quella dei vicini | Cercando dove fossero installate le competenze del guasto 17. Non c'era nessun posto dove installarle | Una dotazione propria sotto la casa di Sailor, che viaggia col prodotto. Il rilevatore torna a dire cosa c'è sulla macchina — che è un'altra domanda | **aperto** |
| 19 | 30/08 | La casa di Sailor era **in due posti**: chi cercava deposito e listino risolveva `~/.config/sailor`, chi cercava i descrittori dell'utente `~/.sailor`, e la documentazione spedita mandava tutti nella seconda | Scrivendoci dentro il listino dei prezzi seguendo la documentazione, e vedendo che nessuno lo leggeva | Una funzione sola che dice dove sta la casa, e i chiamanti che la chiedono invece di riscriverla. Le due copie sbagliavano **insieme**, quindi si confermavano a vicenda | **chiuso** — con mutante |
| 20 | 30/08 | Un `with` che nomina un campo inesistente viene **ignorato in silenzio**: il passo parte, il motore riceve una riga di comando monca, e l'errore che arriva è suo | Scrivendo `prompt` invece di `stdin` in un flusso di prova. Il messaggio era «Input must be provided either through stdin» — di Claude, non di Sailor | Un campo sconosciuto in un `with` è un avviso su quel campo, non silenzio: è il guasto 8 visto dall'altro lato, e `flow check` è il posto dove dirlo prima di spendere | **aperto** |
| 21 | 30/08 | Le prove di `profiles` si rubavano la cartella temporanea a vicenda: **una esecuzione su venti falliva**, ogni volta su una prova diversa | Eseguendo la batteria molte volte di fila. Misurato: 1 su 20 prima, 0 su 50 dopo | Un contatore che cresce nel nome, invece del solo orologio: `cargo test` manda le prove sullo stesso processo e l'orologio di macOS non ha la risoluzione del nanosecondo | **chiuso** — misurato prima e dopo |
| 23 | 31/08 | **136 identificatori in italiano** — variabili, moduli, costanti, due file di prova, classi CSS — contro una regola scritta in `AGENTS.md` dal 28/08. Quasi tutti nati dopo di essa | Theo, leggendo il codice: «inaccettabile vedere ancora funzioni, variabili o codice, nomi di file scritti in italiano». Nessun controllo li aveva mai visti | La regola va dove si rilegge (`docs/decisioni.md`) **e** ha un controllo che la rende rossa. Serve anche che sia completa: quella vecchia elencava «funzione, tipi, campi, opzioni» e i nomi di file non c'erano — chi scriveva `smista_il_lavoro.rs` non trovava il proprio caso, e la direttiva di sessione «rispondi in italiano» vinceva | **chiuso** — il controllo è nato rosso su 64 e ora è verde |
| 22 | 31/08 | Il costo totale di una corsa era la **costante `0`**, scritta a mano in due punti, mentre le sue chiamate portavano il costo giusto una per una. La finestra mostrava lo zero accanto alla somma vera | Cercando su quale numero appoggiare un tetto di spesa. Era già registrato in `docs/da-fare.md` dal 29/08 — «costo totale sulle 16 corse: 0 su tutte» — e nessuno l'aveva letto come un guasto | Un totale non si dichiara: si chiede a chi tiene le righe. E le due copie di quella scrittura diventano una, nel crate che le due strade condividono già | **chiuso** — con mutante |

## Cosa dice questa tabella, letta tutta insieme

**Otto su tredici si sono visti solo eseguendo o guardando.** Non dal codice, non
dai tipi, non dalle prove: cronometrando, aprendo il deposito, facendo uno
screenshot, controllando il peso di una cartella. È la ragione per cui una prova
che non poteva venire diversa non è una prova.

**Undici sono ancora aperti** su ventidue, e tre di quelli (4, 10, 11) sono la stessa
cosa vista da tre lati: **il sistema non sa cosa sta facendo di sé** — quali
processi ha avviato, quante copie ha della stessa verità, se ciò che dichiara
corrisponde a ciò che fa.

**Il ventiduesimo era già scritto, e non contava come guasto.** Il numero «costo
totale sulle 16 corse: 0 su tutte» stava in `docs/da-fare.md` dal 29/08, in una
tabella di stato del deposito. Nessuno l'ha letto come un difetto finché non è
servito appoggiarci sopra una decisione: uno zero in una colonna di misure
sembra un sistema che non ha ancora speso, non un sistema che non sa contare. Un
dato mancante si traveste da dato tranquillo, e l'unico modo per smascherarlo è
provare a usarlo per decidere qualcosa.

**Nessuno è stato trovato da un controllo automatico.** Tutti da una persona che
guardava. Questo è il numero che deve cambiare — e il 23 è il primo che lascia
dietro di sé un controllo capace di ritrovarlo: `cargo test -p sailor --test
identifiers_are_in_english` è nato **rosso su 64 identificatori** ed è diventato
verde riparandoli. Non è una prova scritta dopo per certificare il lavoro: è la
misura che ha detto quanto lavoro c'era.

**Il 30/08 ne sono arrivati tre in un giorno solo, e tutti e tre eseguendo.** Il
19 si è visto scrivendo un file dove la documentazione diceva e guardando che
nessuno lo leggeva; il 20 lanciando un flusso con un campo sbagliato; il 21
rieseguendo la batteria venti volte di fila invece di una. Nessuno dei tre
sarebbe uscito leggendo il codice: due erano d'accordo con se stessi, e il terzo
si vedeva solo ripetendo.

**Il quindicesimo dice perché.** Sailor non ha comandi per operare su se stesso,
quindi chi ci lavora lo aggira con script esterni — e uno strumento aggirato non
registra niente di ciò che gli succede intorno. È lo stesso difetto del quarto e
del decimo, alla radice: **il sistema non sa cosa sta facendo di sé perché non
passa da sé.**

**Il tredicesimo era già scritto, e non è servito.** La trappola di zsh stava in
una memoria da giorni: chi ha riscritto la sorveglianza — cioè chi aveva appena
finito di riparare il guasto 12 — non l'ha consultata e ci è ricaduto dentro
venti minuti dopo. È la prova che una lezione scritta non vale niente se non c'è
il momento in cui qualcuno la va a leggere. Per i flussi quel momento adesso
esiste, ed è `docs/decisioni.md`, letto in tre punti del flusso di sviluppo. Per
chi scrive a mano no.
