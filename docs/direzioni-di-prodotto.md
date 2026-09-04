# Direzioni di prodotto — segnate, non decise

> **Riletto il 04/09/2026: il punto 1 non descrive più la finestra.** Diceva
> «nessuna pagina per profili, modelli, fornitori»: `ProfileList.tsx` e
> `QuotaScreen.tsx` esistono, con le loro prove, e lo spazio di lavoro è
> diventato la spina dorsale invece di una scheda. Le altre tre direzioni non
> sono state rilette riga per riga: **prendile come segnate il 29/08, non come
> lo stato di oggi.** Per cosa manca davvero, la fonte è
> `docs/2026-09-02-the-termination-condition.md`.

Theo, 29/08/2026: «per ora segnamolo, poi prenderemo decisioni; concentriamoci
sul core e sulla risoluzione di avere la benzina».

Niente di qui dentro è deciso. Sta scritto perché una direzione dimenticata
diventa una scelta presa per omissione — che è come si sono accumulate le altre
cose scritte in questo repo.

## 1. La finestra: «più simile a n8n, ma con una UI/UX migliore»

Oggi non ci siamo per niente: c'è la tela dei flussi, l'editor di un passo, la
console di una corsa. Nessuna pagina per profili, modelli, fornitori. I nodi non
mostrano il modello che useranno né cosa è entrato dentro di loro.

Il metro resta quello già scritto fra i vincoli permanenti: **un'interfaccia che
nasconde cosa succede è il contrario del prodotto.** «Meglio di n8n» va misurato
lì, non sull'estetica.

## 2. Il processo di sviluppo come flusso

Non solo *cosa* costruiamo, ma **come**: un flusso che integri ciò che si
sviluppa. Da indagare seriamente, e le due domande sono queste:

- **il riutilizzo dei flussi** — un flusso scritto una volta dev'essere usabile
  da un altro senza copiarlo (è `subflow`, che non esiste);
- **come si progetta bene un flusso** — cosa è un nodo, quando si spezza, cosa
  si ripete. Legato alla tassonomia dei nodi: finché il tipo di un nodo non è un
  dato, «progettare bene» non ha un metro.

## 3. Integrare invece di costruire dentro

**Un nodo connesso, non codice nostro.** Vale per la parte di finestra e per
l'orchestrazione: se esiste un progetto vivo che fa quel pezzo, si collega.

*Domanda aperta, e resta aperta*: quale progetto vivo si può collegare per la
parte di finestra. Il 29/08/2026 era stato scritto qui che la risposta fosse un
prodotto preciso, ed era un fraintendimento: Theo l'aveva nominato come *esempio
di prodotto che integra un browser*, non come cosa da unire.

L'innesto però c'è già, qualunque sia il progetto: le azioni sono un registro, e
un progetto esterno si collega come **azione nuova** — la stessa forma con cui un
motore nuovo si aggiunge scrivendo un descrittore. Non serve inventare un
meccanismo, serve scegliere il progetto.

## 4. Un browser dentro, come strumento di verifica

Non è una comodità: è **l'oracolo del lavoro sull'interfaccia**. Quando si
disegna un prodotto in React, il browser dentro serve a *vedere* il risultato —
ed è l'unico modo di giudicarlo.

Il prodotto citato il 29/08 come esempio ne ha uno integrato, ed era **l'esempio
e nient'altro**.

Questa direzione ha già il suo vincolo permanente, e non è nuovo: **lo schermo è
il giudice** — «una regola di progetto che non si può verificare guardando
un'immagine è un'opinione». Quel vincolo nacque da due difetti che né i tipi né
le prove avevano visto, fra cui un elemento disegnato dello stesso colore dello
sfondo. Finché un flusso che tocca la finestra non può guardare cosa ha
prodotto, quel vincolo resta una frase: nessun passo può rispettarlo.

E si lega al punto 2: un flusso che sviluppa l'interfaccia **deve** poter
aprire ciò che ha appena scritto e confrontarlo. Senza, il flusso di sviluppo
sull'interfaccia è cieco esattamente dove il prodotto si gioca.

## Cosa viene prima di tutto questo

**La benzina.** Misurare il consumo, riconoscere una quota finita, e sfruttare
le fasce gratuite che i fornitori dichiarano. Senza, ogni corsa dipende da un
solo abbonamento e si ferma quando finisce — come il 29/08.
