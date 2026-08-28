// La tela senza flussi.
//
// È lo stato in cui il prodotto si apre la prima volta, ed è quello che si
// sbaglia più spesso: una finestra vuota, con la minimappa vuota e i comandi
// senza niente da comandare, si legge come un guasto — e il gesto che serve,
// creare il primo flusso, non è da nessuna parte. Qui la tela bianca dice cosa
// è un flusso e offre il gesto.
//
// TRE STATI, TRE FRASI DIVERSE. «Sto chiedendo», «il motore non risponde» e
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

export function BlankCanvas({ state, failure, brokenCount, onCreate }: BlankCanvasProps) {
  return (
    <div className="blank">
      <div className="blank__card">
        {state === "failed" ? (
          <>
            <h2>Il motore non risponde.</h2>
            <p>
              La tela è vuota perché nessuno ha potuto dire cosa c'è sul disco, non perché il disco
              sia vuoto. Finché il motore tace, un flusso creato adesso non si potrebbe salvare.
            </p>
            {failure && <p className="blank__why">{failure}</p>}
          </>
        ) : state === "loading" ? (
          <p>Chiedo i flussi al motore…</p>
        ) : (
          <>
            <h2>Qui non c'è ancora niente. Cominciamo.</h2>
            <p>
              Un flusso è una sequenza di passi: ognuno fa una cosa sola e dice da quali altri
              dipende. Creane uno, aggiungi i passi dalla cassetta a sinistra e collegali
              trascinando da un nodo all'altro.
            </p>
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
          </>
        )}
      </div>
    </div>
  );
}
