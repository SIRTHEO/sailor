// Il segno grafico di uno strumento.
//
// IL SEGNO È UN DATO, NON UN RAMO DI CODICE. Qui sotto c'è una mappa da
// identificativo a disegno, e il componente sa disegnare *una forma*, mai una
// forma in particolare. Non esiste nessun `if (tool === "claude")`: aggiungere
// un segno è aggiungere una voce alla mappa, e non aggiungerlo non toglie
// niente a nessuno.
//
// IL RIPIEGO È IL CASO NORMALE. Chi installa Sailor ha sul disco strumenti che
// io non ho mai visto: la mappa qui sotto copre una manciata di identificativi,
// e tutto il resto — la maggioranza, su qualunque macchina vera — prende un
// monogramma su una tinta calcolata dall'identificativo stesso. Deve quindi
// essere dignitoso, non una scatola vuota: è quello che si vedrà quasi sempre.
//
// I DISEGNI SONO INLINE E FATTI QUI. Nessuna richiesta di rete: il guscio non
// ne fa, e una tela che aspetta un logo da un CDN è una tela che sul portatile
// di qualcun altro resta bianca. Sono forme geometriche che distinguono a colpo
// d'occhio, non riproduzioni dei marchi: servono a far riconoscere una riga in
// un elenco, e per quello bastano.

import type { CSSProperties } from "react";

export interface ToolMarkShape {
  /** I tratti del disegno, su una griglia 24×24. */
  paths: Array<{ d: string; fill?: boolean }>;
  /** Il colore del segno. */
  tint: string;
}

/**
 * I segni che conosco, per identificativo dello strumento come lo dichiara il
 * motore (`id` in `discover_tools`). Una voce in più qui non richiede di
 * toccare nient'altro.
 */
const MARKS: Record<string, ToolMarkShape> = {
  "claude-code": {
    tint: "#d97757",
    // Un asterisco di raggi che partono dal centro.
    paths: [
      { d: "M12 3v7M12 14v7M4.2 7.5l6.1 3.5M13.7 13l6.1 3.5M4.2 16.5l6.1-3.5M13.7 11l6.1-3.5" },
    ],
  },
  codex: {
    tint: "#10a37f",
    // Un anello spezzato con un nodo al centro: un ciclo che passa da un punto.
    paths: [
      { d: "M19 12a7 7 0 1 1-3.5-6.1" },
      { d: "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z", fill: true },
    ],
  },
  "gemini-cli": {
    tint: "#4285f4",
    // Una stella a quattro punte, tutta d'un tratto.
    paths: [{ d: "M12 2c0 5.5 4.5 10 10 10-5.5 0-10 4.5-10 10 0-5.5-4.5-10-10-10 5.5 0 10-4.5 10-10z", fill: true }],
  },
  ollama: {
    tint: "#7c3aed",
    // Due archi come orecchie sopra un corpo tondo.
    paths: [
      { d: "M7.5 9c-.8-1.6-.9-3.4-.4-5 1.4.7 2.5 2 3 3.6M16.5 9c.8-1.6.9-3.4.4-5-1.4.7-2.5 2-3 3.6" },
      { d: "M12 21c-3.6 0-6-2.4-6-6s2.4-7 6-7 6 3.4 6 7-2.4 6-6 6z" },
    ],
  },
  git: {
    tint: "#f05033",
    // Tre nodi e i rami che li uniscono.
    paths: [
      { d: "M6 4v10M6 14a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM17 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM17 9c0 4-5 3-11 5" },
    ],
  },
  gh: {
    tint: "#8b949e",
    // Un tondo con la coda: la sagoma che tutti riconoscono, ridotta all'osso.
    paths: [
      { d: "M12 2.5a9.5 9.5 0 0 0-3 18.5v-3.2c-2.4.5-3-1.2-3-1.2-.4-1-1-1.3-1-1.3-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.3 1.1 2.9.8.1-.6.3-1.1.6-1.3-2-.2-4-1-4-4.4 0-1 .3-1.8.9-2.4-.1-.2-.4-1.1.1-2.3 0 0 .7-.2 2.4.9a8.3 8.3 0 0 1 4.4 0c1.7-1.1 2.4-.9 2.4-.9.5 1.2.2 2.1.1 2.3.6.6.9 1.4.9 2.4 0 3.4-2 4.2-4 4.4.3.3.6.9.6 1.8V21A9.5 9.5 0 0 0 12 2.5z", fill: true },
    ],
  },
  docker: {
    tint: "#2496ed",
    // Container impilati sopra la linea dell'acqua.
    paths: [
      { d: "M4 12h4v4H4zM9 12h4v4H9zM14 12h4v4h-4zM9 7h4v4H9z", fill: true },
      { d: "M2 17c3 2 7 2.5 11 1.5 3-.7 5.4-2.4 6.5-4.5" },
    ],
  },
  node: {
    tint: "#5fa04e",
    // L'esagono, senza altro dentro.
    paths: [{ d: "M12 2.5l8.2 4.75v9.5L12 21.5l-8.2-4.75v-9.5z" }],
  },
  npm: {
    tint: "#cb3837",
    // Il blocco pieno con la tacca.
    paths: [
      { d: "M2 6h20v12h-10v-9h-4v9H2z", fill: true },
    ],
  },
  cargo: {
    tint: "#c96a3f",
    // Un ingranaggio: un anello e i suoi denti.
    paths: [
      { d: "M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z" },
      { d: "M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2.1 2.1M16.9 16.9L19 19M19 5l-2.1 2.1M7.1 16.9L5 19" },
    ],
  },
  kubectl: {
    tint: "#326ce5",
    // Il timone: un cerchio e i suoi raggi.
    paths: [
      { d: "M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9z" },
      { d: "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z", fill: true },
      { d: "M12 3v6.5M19.8 7.5l-5.6 3.3M19.8 16.5l-5.6-3.3M12 21v-6.5M4.2 16.5l5.6-3.3M4.2 7.5l5.6 3.3" },
    ],
  },
  curl: {
    tint: "#0b7285",
    // Un'onda che entra e una che esce.
    paths: [{ d: "M2 9c3-3 5 3 8 0s5 3 8 0M2 16c3-3 5 3 8 0s5 3 8 0" }],
  },
  python: {
    tint: "#3776ab",
    // Due corpi che si incastrano.
    paths: [
      { d: "M12 2.5c-3 0-4.5 1.2-4.5 3.2V9h4.5v1H6.2C4 10 3 11.6 3 14.5S4 19 6.2 19H8v-3.2C8 13.6 9.4 12 11.5 12h4" },
      { d: "M12 21.5c3 0 4.5-1.2 4.5-3.2V15H12v-1h5.8c2.2 0 3.2-1.6 3.2-4.5S20 5 17.8 5H16v3.2c0 2.2-1.4 3.8-3.5 3.8h-4" },
    ],
  },
};

/**
 * La tinta del ripiego, calcolata dall'identificativo.
 *
 * Deve essere *stabile* e *distinta*: lo stesso strumento ha sempre lo stesso
 * colore su ogni macchina e a ogni avvio — altrimenti il colore non aiuta a
 * riconoscere niente — e due strumenti vicini nell'elenco tendono a cadere
 * lontani sulla ruota, perché a differenziare siano le cifre alte dell'hash.
 */
function fallbackTint(id: string): string {
  let hash = 0;
  for (let index = 0; index < id.length; index += 1) {
    hash = (hash * 31 + id.charCodeAt(index)) >>> 0;
  }
  const hue = hash % 360;
  // Saturazione e luminosità restano in una fascia stretta: il segno deve
  // leggersi sul chiaro e sullo scuro senza che nessuno lo scelga a mano.
  return `hsl(${hue} 52% 48%)`;
}

/**
 * Il monogramma: la prima lettera di ogni pezzo dell'identificativo, al più
 * due. `docker-compose` dà «DC», `socraticode` dà «S». Non è un'abbreviazione
 * ufficiale di niente — è un appiglio per l'occhio, e il nome per intero sta
 * comunque scritto accanto.
 */
export function monogram(id: string): string {
  const parts = id.split(/[-_. ]+/).filter((part) => part !== "");
  if (parts.length === 0) return "?";
  const letters = parts.slice(0, 2).map((part) => part[0]!.toUpperCase());
  return letters.join("");
}

/** Vero se conosco un disegno per questo identificativo. */
export function hasMark(id: string): boolean {
  return id in MARKS;
}

export interface ToolMarkProps {
  /** L'identificativo dello strumento come lo dichiara il motore. */
  id: string;
  size?: number;
  /**
   * Uno strumento che non c'è si mostra spento — visibile e in grigio — non
   * nascosto: chi guarda un nodo che non può girare deve capirlo dalla tela.
   */
  off?: boolean;
  title?: string;
}

/**
 * Il segno di uno strumento: il suo disegno se lo conosco, il monogramma se no.
 *
 * Non conosce nessuno strumento per nome — chiede alla mappa, e la mappa può
 * essere vuota senza che questo componente cambi di una riga.
 */
export function ToolMark({ id, size = 18, off = false, title }: ToolMarkProps) {
  const shape = MARKS[id];
  const tint = off ? "#94a3b8" : (shape?.tint ?? fallbackTint(id));

  return (
    <span
      className="tool-mark"
      data-off={off || undefined}
      // La tinta viaggia anche come proprietà propria: il monogramma la usa per
      // il proprio sfondo, e non puo' leggerla da `currentColor` perche' su di
      // se' dichiara il bianco del testo.
      style={{ width: size, height: size, color: tint, "--mark-tint": tint } as CSSProperties}
      title={title ?? id}
      aria-hidden={title === undefined || undefined}
    >
      {shape ? (
        <svg viewBox="0 0 24 24" width={size} height={size} role="presentation">
          {shape.paths.map((path, index) => (
            <path
              key={index}
              d={path.d}
              fill={path.fill ? "currentColor" : "none"}
              stroke={path.fill ? "none" : "currentColor"}
              strokeWidth={1.8}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          ))}
        </svg>
      ) : (
        // Il ripiego non imita un logo che non ho: dice le iniziali su una
        // pastiglia, e si vede che è un ripiego invece di sembrare un segno
        // ufficiale sbagliato.
        <span className="tool-mark__monogram" style={{ fontSize: Math.round(size * 0.44) }}>
          {monogram(id)}
        </span>
      )}
    </span>
  );
}
