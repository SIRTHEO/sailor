# Direzioni di prodotto — segnate, non decise

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

*Domanda aperta*: Theo ha nominato un progetto da unire per la finestra. Il nome
scritto qui sarebbe un'invenzione — **va chiesto a lui prima di cercarlo**, e
questa riga esiste perché nessuno lo dia per assodato.

Questa direzione ha già un innesto pronto: le azioni sono un registro, e un
progetto esterno si collega come **azione nuova** — è la stessa forma con cui un
motore nuovo si aggiunge scrivendo un descrittore.

## 4. Un browser dentro, come fa Orca

Quando ci saranno i terminali: chi guarda un flusso vede anche il browser che si
apre. È la stessa idea della tracciabilità — vedere cosa è successo, non che sia
successo — applicata a ciò che un flusso fa fuori dalla macchina.

## Cosa viene prima di tutto questo

**La benzina.** Misurare il consumo, riconoscere una quota finita, e sfruttare
le fasce gratuite che i fornitori dichiarano. Senza, ogni corsa dipende da un
solo abbonamento e si ferma quando finisce — come il 29/08.
