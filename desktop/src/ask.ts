// Chiedere una cosa al motore, e dire onestamente com'è andata.
//
// **TRE ESITI, NON DUE.** «Ho la risposta» e «non ce l'ho» non bastano: fra i
// due c'è «non ho potuto chiedere», ed è quello che va detto per esteso. Una
// schermata vuota perché non è girato niente e una schermata vuota perché il
// motore non risponde si assomigliano troppo, e la seconda è quella in cui si
// continua a lavorare credendo che non stia succedendo nulla.
//
// PERCHÉ UN GANCIO E NON TRE COPIE. «Adesso», la storia e l'inventario fanno
// tutti e tre la stessa cosa: chiedono al guscio, ripetono ogni tanto, e devono
// saper dire perché non hanno risposta. Scritto tre volte, il terzo lo scrive
// senza il ramo del silenzio — che è il ramo che conta.

import { useCallback, useEffect, useState } from "react";

/** Com'è andata la domanda, dal punto di vista di chi guarda. */
export type Asked<T> =
  | { state: "asking" }
  | { state: "answered"; value: T }
  | { state: "mute"; why: string };

/**
 * Chiede al motore, e ripete finché la schermata è aperta.
 *
 * `every` in millisecondi, oppure `null` per chiedere una volta sola: un
 * censimento del disco non va rifatto ogni quattro secondi, una corsa viva sì.
 */
export function useAsk<T>(
  native: boolean,
  question: () => Promise<T>,
  every: number | null,
  outside: string,
): { asked: Asked<T>; again: () => void } {
  const [asked, setAsked] = useState<Asked<T>>(() =>
    native ? { state: "asking" } : { state: "mute", why: outside },
  );

  const again = useCallback(() => {
    if (!native) return;
    question()
      .then((value) => setAsked({ state: "answered", value }))
      .catch((error: unknown) => setAsked({ state: "mute", why: String(error) }));
    // `question` sta fuori dalle dipendenze di proposito: chi chiama la scrive
    // in linea, quindi cambia a ogni render, e metterla qui rifarebbe la
    // domanda a ogni disegno invece che a ogni battito.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [native]);

  useEffect(() => {
    if (!native) return;
    again();
    if (every === null) return;
    const tick = window.setInterval(again, every);
    return () => window.clearInterval(tick);
  }, [native, every, again]);

  return { asked, again };
}

/**
 * Un orologio che batte, per far invecchiare le durate a schermo.
 *
 * **SENZA, «ferma da 2 min» RESTA SCRITTO PER UN'ORA.** La riga sembrerebbe
 * viva e sarebbe congelata: è lo stesso difetto per cui il 28/08/2026 una vista
 * mostrava «in corso da 00:30» su una corsa finita da un pezzo.
 */
export function useClock(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(tick);
  }, []);
  return now;
}
