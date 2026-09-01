// La tela senza flussi.
//
// È lo stato in cui il prodotto si apre la prima volta, ed è quello che si
// sbaglia più spesso: una finestra vuota, con la minimappa vuota e i comandi
// senza niente da comandare, si legge come un guasto — e il gesto che serve,
// creare il primo flusso, non è da nessuna parte. Qui la tela bianca dice cosa
// è un flusso e offre il gesto.
//
// TRE STATI, TRE FORME DIVERSE. «Sto chiedendo», «il motore non risponde» e
// «non c'è ancora niente» portano a cose diverse: nel secondo un flusso creato
// adesso non si potrebbe salvare, e offrire il gesto sarebbe una promessa che
// nessuno può mantenere.

export type CanvasState = "loading" | "failed" | "empty";

interface BlankCanvasProps {
  state: CanvasState;
  /** Perché il motore non risponde, parola per parola: si mostra, non si riassume. */
  failure?: string | null;
  brokenCount: number;
  onCreate: () => void;
}

/**
 * Le corsie dello scheletro, e quanti passi ognuna finge di avere.
 *
 * Due corsie di lunghezza diversa, non due uguali: uno scheletro simmetrico si
 * legge come un motivo decorativo, uno sbilenco come del contenuto che sta
 * arrivando. I numeri non promettono niente — nessuno sa ancora cosa c'è sul
 * disco — e infatti la forma non porta nessuna parola.
 */
const SKELETON_LANES = [3, 4];

export function BlankCanvas({ state, failure, brokenCount, onCreate }: BlankCanvasProps) {
  /**
   * **LA LETTURA IN CORSO È UNA FORMA, NON UNA ROTELLA.**
   *
   * Una rotella dice «aspetta»; uno scheletro dice **cosa sta arrivando** — le
   * corsie e i loro passi, nel posto dove compariranno — e chi guarda comincia
   * a leggere lo schermo prima che i dati arrivino.
   *
   * Non si muove. Il foglio dichiara due durate e una curva, e le riserva a un
   * movimento che conferma un gesto: qui non c'è nessun gesto da confermare, e
   * un battito continuo sarebbe la sola cosa che si muove in tutta la finestra.
   * A dire che la lettura è in corso è la parola sotto la forma, come il
   * divieto 5 pretende accanto a ogni segno.
   */
  if (state === "loading") {
    return (
      <div className="blank" data-state="loading">
        <div className="blank__skeleton" aria-hidden="true">
          {SKELETON_LANES.map((steps, lane) => (
            <div className="blank__lane" key={lane}>
              <div className="blank__head">
                <div className="blank__plate blank__plate--name" />
                <div className="blank__plate blank__plate--desc" />
              </div>
              <div className="blank__row">
                {Array.from({ length: steps }, (_, step) => (
                  <div className="blank__plate blank__plate--step" key={step} />
                ))}
              </div>
            </div>
          ))}
        </div>
        <p className="blank__waiting">Chiedo i flussi al motore…</p>
      </div>
    );
  }

  if (state === "failed") {
    return (
      <div className="blank" data-state="failed">
        <div className="blank__card">
          <h2>Il motore non risponde.</h2>
          <p>
            La tela è vuota perché nessuno ha potuto dire cosa c'è sul disco, non perché il disco
            sia vuoto. Finché il motore tace, un flusso creato adesso non si potrebbe salvare.
          </p>
          {failure && <p className="blank__why">{failure}</p>}
        </div>
      </div>
    );
  }

  /**
   * **LO STATO VUOTO INSEGNA IL PRIMO GESTO**, e per insegnarlo deve nominare
   * posti che esistono. Qui c'era scritto «aggiungi i passi dalla cassetta a
   * sinistra»: la cassetta è diventata una barra in fondo alla tela, e con zero
   * flussi non c'è affatto. Un'istruzione falsa costa più del silenzio, perché
   * chi la segue conclude che il prodotto è rotto.
   */
  return (
    <div className="blank" data-state="empty">
      <div className="blank__card">
        <h2>Qui non c'è ancora niente. Cominciamo.</h2>
        <p>
          Un flusso è una catena di passi: ognuno fa una cosa sola e dice da quali altri dipende.
        </p>
        {/* `role="list"` non è un di più: il foglio toglie i pallini con
            `list-style: none`, e con quella riga alcuni lettori di schermo
            smettono di annunciare «elenco di tre». I numeri qui li disegna il
            foglio, quindi senza il ruolo l'ordine dei gesti si perderebbe
            proprio per chi non lo vede. */}
        <ol className="blank__gestures" role="list">
          <li>Crea il flusso: nasce vuoto, e resta qui finché non lo salvi.</li>
          <li>Aggiungi i passi dalla barra che compare in fondo alla tela.</li>
          <li>Collegali trascinando da una porta all'altra: il filo è la dipendenza.</li>
        </ol>
        {brokenCount > 0 && (
          <p className="blank__why">
            {brokenCount === 1
              ? "Un flusso sul disco non si carica: è in fondo alla colonna, col motivo."
              : `${brokenCount} flussi sul disco non si caricano: sono in fondo alla colonna, col motivo.`}
          </p>
        )}
        <button type="button" className="is-primary" onClick={onCreate}>
          Crea il primo flusso
        </button>
      </div>
    </div>
  );
}
