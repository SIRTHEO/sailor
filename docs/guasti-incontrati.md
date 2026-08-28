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
| 7 | 28/08 | Due passi dichiarati «in parallelo» giravano in fila | Cronometrando: due passi da 6 secondi ne impiegavano 12 | Un controllo che confronta ciò che la descrizione dichiara con ciò che il motore fa | **aperto** — documentato, non riparato |
| 8 | 28/08 | Un descrittore con un campo che questa versione non conosce viene scartato intero | Scrivendone uno apposta | Un campo ignoto è un avviso sul campo, non il rifiuto del componente | **aperto** |
| 9 | 28/08 | Un elemento grafico invisibile: disegnato dello stesso colore dello sfondo | Solo da un'immagine dello schermo. Né i tipi né le prove lo vedevano | Lo schermo come oracolo: un'immagine confrontata, non solo i tipi | **chiuso** — quel caso |
| 10 | 28/08 | La stessa lista di componenti scritta in due punti del programma | Si sono disallineati in un'ora, nello stesso giorno di lavoro | Una sola fonte, e le altre la chiedono invece di ricopiarla | **aperto** |
| 11 | 29/08 | In modalità viva, un errore di compilazione in un crate qualunque **uccide la finestra** invece di lasciarla all'ultima versione buona | La finestra è sparita mentre un altro cantiere aveva un crate a metà | La finestra sopravvive a una compilazione fallita, e lo dice invece di chiudersi | **aperto** |

## Cosa dice questa tabella, letta tutta insieme

**Sei su undici si sono visti solo eseguendo o guardando.** Non dal codice, non
dai tipi, non dalle prove: cronometrando, aprendo il deposito, facendo uno
screenshot, controllando il peso di una cartella. È la ragione per cui una prova
che non poteva venire diversa non è una prova.

**Cinque sono ancora aperti**, e tre di quei cinque (4, 10, 11) sono la stessa
cosa vista da tre lati: **il sistema non sa cosa sta facendo di sé** — quali
processi ha avviato, quante copie ha della stessa verità, se ciò che dichiara
corrisponde a ciò che fa.

**Nessuno è stato trovato da un controllo automatico.** Tutti da una persona che
guardava. Questo è il numero che deve cambiare.
