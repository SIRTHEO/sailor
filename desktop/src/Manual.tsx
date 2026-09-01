// I comandi di Sailor, dichiarati dal binario e non ricopiati qui.
//
// **QUESTO FILE NON CONTIENE IL NOME DI NESSUN COMANDO.** È la sua unica
// regola, e vale più di come è impaginato. Sailor ha dieci comandi e una
// trentina di forme: scriverli in TypeScript sarebbe stato mezz'ora di lavoro
// e una pagina che diverge dal binario alla prima opzione aggiunta. È il
// guasto 10 — la stessa verità in più posti, senza niente che li confronti —
// che in questo repo si è già ripresentato cinque volte, l'ultima il
// 01/09/2026 sul vocabolario delle azioni, dove la finestra offriva sei nomi
// che il motore non conosceva e ne rifiutava cinque che sapeva eseguire.
//
// Quindi `crates/sailor` è diventato lib+bin, `manual` traduce solo la forma,
// e qui si dispone. Se domani nasce un comando, questa pagina lo mostra senza
// che nessuno la apra; se ne sparisce uno, sparisce anche da qui.
//
// FUORI DAL GUSCIO NON SI INVENTA UN ELENCO. Nel browser il motore non
// risponde, e la pagina lo dice invece di mostrare un esempio plausibile: un
// manuale finto è peggio di un manuale assente, perché si legge uguale.

import { useMemo, useState } from "react";
import { useAsk } from "./ask";
import { manual, type CommandDoc } from "./engine";

/** Il manuale è compilato dentro il binario: si chiede una volta. */
const ONCE = null;

const EMPTY: CommandDoc[] = [];

/**
 * Le parole di una forma d'uso, separate in ciò che si digita e ciò che si
 * sostituisce. Serve a dare peso diverso alle due cose senza che nessuno
 * riscriva le righe a mano: `<nome>` e `[opzioni]` sono buchi da riempire,
 * tutto il resto è testo letterale.
 */
export function pieces(line: string): { text: string; hole: boolean }[] {
  return line
    .split(/(\s+)/)
    .filter((piece) => piece.trim() !== "")
    .map((word) => ({
      text: word,
      hole: word.startsWith("<") || word.startsWith("[") || word.includes("|"),
    }));
}

/** Quante forme in tutto: il numero che dice se il manuale è arrivato intero. */
export function shapeCount(commands: CommandDoc[]): number {
  return commands.reduce((total, command) => total + command.usage.length, 0);
}

export function Manual({ native }: { native: boolean }) {
  const { asked } = useAsk<CommandDoc[]>(
    native,
    manual,
    ONCE,
    "fuori dal guscio: i comandi li dichiara il binario",
  );
  const [open, setOpen] = useState<string | null>(null);

  const commands = asked.state === "answered" ? asked.value : EMPTY;
  const shapes = useMemo(() => shapeCount(commands), [commands]);

  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">Non posso elencare i comandi: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Chiedo al binario quali comandi ha…</p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Comandi</h2>
        <span className="now__count">{commands.length}</span>
        <span className="now__note">
          {shapes} forme, dichiarate dal binario che sta girando
        </span>
      </header>

      <ul className="manual">
        {commands.map((command) => {
          const here = open === command.name;
          return (
            <li className="manual__row" key={command.name}>
              <button
                type="button"
                className="manual__head"
                aria-expanded={here}
                onClick={() => setOpen(here ? null : command.name)}
              >
                <code className="manual__name">sailor {command.name}</code>
                <span className="manual__what">{command.description}</span>
                <span className="manual__shapes">
                  {command.usage.length}
                  <span className="manual__shapes-word">
                    {command.usage.length === 1 ? " forma" : " forme"}
                  </span>
                </span>
              </button>
              {here && (
                <ul className="manual__shapes-list">
                  {command.usage.map((line) => (
                    <li className="manual__shape" key={line}>
                      <code>
                        {pieces(line).map((piece, index) => (
                          <span
                            key={`${piece.text}-${index}`}
                            className={piece.hole ? "manual__hole" : "manual__word"}
                          >
                            {piece.text}{" "}
                          </span>
                        ))}
                      </code>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
