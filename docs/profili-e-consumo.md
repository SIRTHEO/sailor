# Profili, account, consumo — cosa fanno gli altri e cosa ne prendiamo

Ricerca del 29/08/2026, fatta perché una corsa si è fermata sul limite
settimanale di un motore e nessuno sapeva quanto restasse. Le fonti sono in
fondo. Qui sta solo ciò che cambia una decisione.

## Il problema che ci distingue: non sappiamo quale riga di comando avrà l'utente

È il vincolo che scarta metà delle soluzioni trovate. Un sistema che elenca le
CLI che conosce funziona finché qualcuno ne installa una nuova, e poi va
modificato. Sailor non può permetterselo: sarebbe un prodotto che si aggiorna a
ogni strumento nuovo del mondo.

**La risposta esiste già in letteratura, e non è un elenco: è un contratto.**
Git non sa quali gestori di credenziali esistono. Non ne tiene una lista:
definisce un **protocollo** — un programma che si chiama in un certo modo,
riceve righe `chiave=valore` sull'ingresso e ne restituisce altrettante — e
qualunque programma sul percorso che parli quella lingua funziona senza che Git
lo conosca. Si possono metterne diversi in fila: si chiedono in ordine e vince
il primo che risponde.

**Cosa ne discende per noi**: un profilo per una CLI ignota non è codice nostro,
è un **descrittore** che l'utente scrive — come già sono i descrittori degli
inneschi. Dentro ci va: quale variabile d'ambiente sposta la casa di quello
strumento, quale file tiene le credenziali, come si chiede allo strumento quanto
ha consumato. Chi ha una CLI che non conosciamo ne scrive uno e non tocca
Sailor. Chi non lo scrive resta sul ripiego che già abbiamo — il collegamento
simbolico — che funziona peggio ma funziona.

## I profili: tutti fanno la stessa cosa, e noi la facciamo già

AWS, Google Cloud e Kubernetes sono arrivati indipendentemente alla stessa
forma: **un file con sezioni nominate, una variabile d'ambiente che sceglie la
sezione, un'opzione sulla riga di comando che scavalca la variabile.** Tre
livelli, dal più permanente al più immediato.

`sailor profiles` fa il primo e il secondo. **Manca il terzo**: non si può dire
«questa corsa usa il profilo X» senza cambiare quello attivo per tutti. In un
sistema che lancia flussi mentre una persona lavora nello stesso terminale,
cambiare uno stato globale per una corsa è un guasto che aspetta.

Sulle credenziali, la pratica raccomandata da tutti è **non tenerle in chiaro in
un file**, ma chiederle al portachiavi del sistema al momento dell'uso. Noi
oggi spostiamo file e facciamo collegamenti simbolici: funziona, ma vuol dire
che il segreto sta scritto sul disco, e il ripiego col collegamento è già
marcato come il più fragile.

## Le quote: esiste un sistema che fa esattamente ciò che Theo ha descritto

LiteLLM è un intermediario davanti a un centinaio di fornitori. Ha **chiavi
virtuali con un tetto di spesa e un limite di frequenza ciascuna**, e traccia il
consumo per chiave, per persona, per squadra, per progetto. Ci sono tetti duri —
oltre il tetto le richieste si fermano — e soglie di avviso morbide.

È, parola per parola, «solo il 10% qui, il 40% riservato là, e poi bloccati».
Non va inventato: va guardato come funziona. **Il prezzo è dichiarato**: vuole
un database per le chiavi e la spesa, e uno per lo stato dell'instradamento.

**Ma c'è un'asimmetria che questa ricerca rende netta, ed è quella che ci ha
fermati stanotte.** Quel modello funziona su chiavi d'interfaccia, dove ogni
chiamata passa da un punto che possiamo contare. **Non funziona su una riga di
comando con un abbonamento**: quella parla col proprio fornitore, non da noi, e
il suo limite settimanale non è una nostra riga in un registro. Su quelle
l'unico segnale onesto è **ciò che lo strumento dichiara**: il consumo che
riporta a fine chiamata, e il rifiuto quando è finito.

Quindi due meccanismi, non uno:

| | motori a chiave | riga di comando con abbonamento |
|---|---|---|
| quanto è costato | contato da noi sulla chiamata | **solo ciò che lo strumento dichiara** |
| quanto resta | tetto che decidiamo noi | **solo quando rifiuta** |
| distribuire fra profili | sì, in anticipo | solo a posteriori, e a occhio |
| se il fornitore non dichiara niente | non capita | resta cieco, e va detto |

La riga in fondo è il vincolo di indipendenza dal modello applicato qui: chi non
dichiara il consumo deve continuare a funzionare, misurando peggio.

## Prevedere prima di spendere: metà è risolto

**L'ingresso si può contare esattamente prima di spendere.** Anthropic ha un
punto d'accesso che conta i token di una richiesta senza eseguirla, gratis,
accettando gli stessi dati della chiamata vera — sistema, strumenti, immagini,
documenti. Il conteggio è dichiarato come stima che può scostarsi di poco.

**L'uscita no.** Nessuno sa in anticipo quanto scriverà un modello. Si può solo
metterle un tetto e stimarla da com'è andata prima. Questa è la ragione per cui
la previsione viene **dopo** la misura, non prima: senza uno storico di corse
misurate non c'è niente su cui stimare.

Sul contare quando c'è una cache, DeepSeek separa nella risposta i token
d'ingresso che hanno colpito la cache da quelli che non l'hanno colpita, e i due
si sommano al totale. È la forma giusta anche per noi: un solo numero di
«ingresso» nasconde una differenza di prezzo di un ordine di grandezza, e
renderebbe la nostra misura del costo falsa proprio dove conta.

## Cosa ne discende, in ordine di dipendenza

1. **Leggere il consumo che il motore dichiara e scriverlo nel deposito**, con i
   token d'ingresso separati fra colpiti e non colpiti dalla cache. Senza questo
   nessuno degli altri esiste.
2. **Un profilo per corsa**, non solo globale: il terzo livello che AWS ha e noi
   no.
3. **Il descrittore di una CLI ignota**, sul modello del protocollo di Git:
   quale variabile, quale file, come si chiede il consumo.
4. **I tetti per profilo**, con la soglia d'avviso separata dal blocco.
5. **La previsione**, solo dell'ingresso e solo dichiarata come stima, quando
   c'è uno storico su cui poggiarla.

E un guasto da registrare subito, perché è già capitato: **un motore esaurito
non è un motore rotto.** Va distinto, perché il primo si aspetta o si instrada
altrove e il secondo no.

## Fonti

- Git, protocollo dei gestori di credenziali — <https://git-scm.com/docs/gitcredentials>
- AWS, profili nominati — <https://dev.to/andreasbergstrom/juggling-multiple-aws-cli-profiles-like-a-pro-2h88>
- LiteLLM, chiavi virtuali, tetti e limiti — <https://docs.litellm.ai/docs/proxy/users>
- Anthropic, conteggio dei token — <https://docs.anthropic.com/en/docs/build-with-claude/token-counting>
- DeepSeek, cache di contesto e campi del consumo — <https://api-docs.deepseek.com/guides/kv_cache/>

---

# Il seguito del 29/08 sera: l'asimmetria si scioglie

Ieri avevo concluso che una riga di comando con abbonamento «parla col proprio
fornitore, non da noi», e che perciò il consumo lì non si può contare. **È vero
solo finché non la si fa passare da noi.**

## Quello che fanno i progetti seri: un punto locale in mezzo

Ne esistono diversi, vivi e usati. Il più maturo — Claude Code Router — è un
**punto d'accesso locale** verso cui si fa puntare la CLI: da lì dentro si
decidono fornitori, modelli, account e regole d'instradamento, con **riserve di
credenziali, rotazione delle chiavi e modelli di ripiego in ordine**. Altri
(OmniRoute, 9router) fanno la stessa cosa con più fornitori e ripiego che tiene
conto della quota residua.

La forma è sempre la stessa, ed è banale: **la CLI non sa di essere dirottata.**
Si cambia una variabile d'ambiente che dice dove sta il servizio, e tutto il
resto — il suo prompt, i suoi strumenti, il suo modo di lavorare — resta uguale.

**Perché per noi cambia tutto.** Se le chiamate passano da un punto nostro:

- il consumo si conta **esatto**, non «quello che lo strumento si degna di
  dichiarare»;
- le quote per profilo diventano possibili davvero — il 10% qui, il 40%
  riservato là — perché c'è un posto che le può far rispettare;
- «il modello giusto per il compito» diventa una regola in un file, non un nome
  scritto dentro un flusso;
- un fornitore esaurito si aggira passando al successivo, invece di far fallire
  la corsa.

È lo stesso identico meccanismo che i profili già usano: **spostare una
variabile d'ambiente invece di modificare lo strumento.** Sailor lo fa già per
la casa di una CLI; farlo per il suo punto d'accesso è la stessa idea applicata
un livello più in là.

## I due vincoli che questa strada porta con sé, da dire prima

**1. Non tutto ciò che questi progetti offrono è lecito.** Alcuni si presentano
come «Claude illimitato e gratis» e «non raggiungerai mai i limiti»: quello è
aggirare la quota di un fornitore, e vale il vincolo A della ricerca —
si scarta. Ciò che invece è pienamente legittimo, e abbondante, sono le **quote
gratuite dichiarate dai fornitori stessi**: al 2026 sono un elenco lungo — un
grosso quantitativo di token al giorno da alcuni, migliaia di richieste
giornaliere da altri, un miliardo di token al mese dalla fascia sperimentale di
un altro ancora. Il patto è quello che dichiarano loro, e usarlo per quello che
è non ha niente di storto.

**2. Gratis quasi sempre vuol dire che i tuoi dati addestrano il modello.** Su
alcune fasce è scritto esplicitamente, e su una l'accesso alla quota grossa è
*condizionato* all'averlo accettato. Per uno strumento che gira dentro il codice
di qualcuno **questo non può stare nascosto in una configurazione**: è il
vincolo di chiarezza. Un fornitore, nel catalogo, deve portare scritto cosa fa
dei dati che gli arrivano, e chi manda un lavoro deve poterlo vedere prima.

Ne discende una cosa concreta sull'instradamento automatico: **non tutti i
lavori possono andare ovunque.** Un flusso che legge codice privato e uno che
riassume un documento pubblico non hanno lo stesso insieme di destinazioni
ammesse. La regola d'instradamento ha bisogno di questa dimensione fin
dall'inizio, perché aggiungerla dopo vuol dire aver già mandato qualcosa nel
posto sbagliato.

## Cosa ne discende, e dove si innesta

Il punto 1 della lista di ieri — «leggere il consumo che il motore dichiara» —
resta valido per gli strumenti che restano fuori, ma **smette di essere il primo
passo**. Il primo passo diventa il punto locale in mezzo, perché è quello che
rende esatti tutti i numeri a valle invece di lasciarli alla buona volontà di
chi risponde.

E il catalogo dei modelli che già esiste (`crates/models`) è il posto dove
questa roba vive: oggi tiene i modelli con i soli gratuiti accesi. Deve tenere
anche **da quale fornitore**, **con quale quota**, **a che patto sui dati**, e
**quanto ne resta**.

## Fonti del seguito

- Claude Code Router — <https://github.com/musistudio/claude-code-router>
- OmniRoute — <https://github.com/diegosouzapw/OmniRoute>
- Le fasce gratuite permanenti, confronto 2026 — <https://openrouter.ai/blog/tutorials/free-llm-apis-compared/>
- Elenco mantenuto delle interfacce gratuite — <https://github.com/cheahjs/free-llm-api-resources>
