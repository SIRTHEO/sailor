// @vitest-environment jsdom
//
// **IL PANNELLO NON PUÒ CANCELLARE UN CAMPO CHE NON SA LEGGERE.**
//
// Il principio, che vale oltre questo campo: *ciò che non si sa leggere non si
// può riscrivere*. Chi non sa un campo lo lascia dov'era invece di ometterlo —
// perché omettere è una scrittura, e una scrittura silenziosa su un file altrui
// è una perdita che nessuno vede finché non riapre il flusso.
//
// LA PROVA GIRA SUI FILE VERI E RICONTROLLA IL FILE, non lo stato in memoria.
// Una prova che confrontasse `choice` con quello che il pannello mostra
// resterebbe verde: `choice` è già la copia mutilata. Qui si prende un flusso
// di `flows/` così com'è sul disco, lo si fa passare dal pannello con un gesto
// che chi lo usa fa di continuo — scrivere nel campo «Modello» — e si
// ricostruisce il passo come finirebbe sul disco. Quello si confronta.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { StepEditor } from "./StepEditor";
import { joinToolParams, splitToolParams } from "./tools";
import type { FlowFile, Step } from "./flow";

afterEach(cleanup);

/**
 * I dieci flussi veri, letti col bundler come fa `ports.test.tsx` — e **dai due
 * posti in cui vivono**: nove in `flows/`, di questo progetto, e
 * `smista-il-lavoro` dentro il binario (`crates/flow/system/`), perché le
 * regole di instradamento spedite col prodotto lo nominano. Il pannello li
 * riscrive tutti allo stesso modo, quindi la prova che verifica cosa
 * sopravvive al salvataggio deve vederli tutti: guardando un posto solo, i
 * passi con una catena scendevano da 20 a 18 — e i due che sparivano,
 * `dispatch` e `verify`, sono proprio quelli del flusso spedito.
 */
function realFlows(): Array<{ path: string; flow: FlowFile }> {
  const files = {
    ...(import.meta.glob("../../flows/*.flow.json", {
      eager: true,
      query: "?raw",
      import: "default",
    }) as Record<string, string>),
    ...(import.meta.glob("../../crates/flow/system/*.flow.json", {
      eager: true,
      query: "?raw",
      import: "default",
    }) as Record<string, string>),
  } as Record<string, string>;
  return Object.keys(files)
    .sort()
    .map((path) => ({ path, flow: JSON.parse(files[path]) as FlowFile }));
}

/** I passi dei flussi veri che dichiarano una catena di motori, col loro file. */
function stepsWithAChain(): Array<{ path: string; step: Step }> {
  const found: Array<{ path: string; step: Step }> = [];
  for (const { path, flow } of realFlows()) {
    for (const step of flow.graph.steps) {
      if (Array.isArray(step.with?.tool)) found.push({ path, step });
    }
  }
  return found;
}

/**
 * Fa passare un passo dal pannello vero e restituisce il `with` come finirebbe
 * sul disco dopo aver toccato il campo «Modello».
 *
 * Non simula il pannello: lo monta. La perdita nasce dall'incastro fra
 * `splitToolParams`, `joinToolParams` e i tasti che il pannello preme, e una
 * simulazione di quell'incastro sarebbe la seconda copia che sbaglia insieme
 * alla prima.
 */
function throughThePanel(step: Step, newModel: string): Record<string, unknown> | null {
  let written: Record<string, unknown> | null | undefined;
  render(
    <StepEditor
      flowName="prova"
      color="#000"
      step={step}
      siblingIds={[]}
      tools={[]}
      discovery={{ state: "ready", tools: [] }}
      usedModels={[]}
      onRename={() => {}}
      onField={(patch) => {
        written = patch.with;
      }}
      onToggleDep={() => {}}
      onDelete={() => {}}
    />,
  );
  const field = screen
    .getAllByText("Modello")
    .map((label) => label.parentElement?.querySelector("input"))
    .find((input): input is HTMLInputElement => input != null);
  expect(field, "il pannello non mostra il campo «Modello»").toBeDefined();
  fireEvent.change(field as HTMLInputElement, { target: { value: newModel } });
  expect(written, "il pannello non ha scritto niente").not.toBeUndefined();
  return written as Record<string, unknown> | null;
}

describe("il pannello riscrive un passo senza perdere quello che non sa leggere", () => {
  const chained = stepsWithAChain();

  test("i flussi veri con una catena si leggono davvero, e sono 20", () => {
    // Senza questa, tutto il resto passerebbe su zero passi — il modo più
    // silenzioso di essere verdi per non aver guardato niente. Sui dieci file
    // di `flows/` i passi `external_engine` sono 25: 20 con una catena, 5 con
    // una stringa sola.
    expect(chained.length).toBe(20);
  });

  test("UN GIRO DAL PANNELLO NON CANCELLA LA CATENA DAL FILE", () => {
    const lost: string[] = [];
    for (const { path, step } of chained) {
      const before = step.with as Record<string, unknown>;
      const after = throughThePanel(step, "opus") ?? {};
      cleanup();
      for (const key of Object.keys(before)) {
        if (!(key in after)) {
          lost.push(
            `${path} · passo «${step.id}»: sparisce «${key}» — c'era ${JSON.stringify(
              before[key],
            )}, e dopo il salvataggio il passo è ${JSON.stringify(after)}`,
          );
        }
      }
    }
    expect(lost.join("\n"), `il pannello ha cancellato dei campi:\n${lost.join("\n")}`).toBe("");
  });

  test("e la catena torna sul disco IDENTICA, non solo presente", () => {
    // «Presente» non basta: un elenco riscritto in un altro ordine è un altro
    // ordine di preferenza, cioè un altro motore in testa.
    for (const { path, step } of chained) {
      const after = throughThePanel(step, "opus") ?? {};
      cleanup();
      expect(after.tool, `${path} · passo «${step.id}»`).toEqual(step.with?.tool);
    }
  });

  test("il modello scritto nel pannello arriva comunque nel file", () => {
    // La difesa non deve diventare «il pannello non scrive più niente»: il
    // gesto che si stava facendo deve continuare a funzionare.
    const { step } = chained[0];
    const after = throughThePanel(step, "opus") ?? {};
    expect(after.model).toBe("opus");
  });

  test("e il pannello DICE che la catena c'è, invece di mostrare «nessuno»", () => {
    // Lasciare il campo dov'era non basta se poi il selettore dice
    // «— nessuno —» sopra un passo che nomina tre motori: sarebbe la bugia del
    // nodo spostata di una finestra, con la sola differenza che questa non
    // cancella niente.
    const { step } = chained[0];
    throughThePanel(step, "opus");
    const chain = (step.with?.tool as string[]).join(" › ");
    expect(screen.getByText(new RegExp(chain))).toBeDefined();
  });

  test("scegliere uno strumento sostituisce la catena, perché è un gesto esplicito", () => {
    // L'altra faccia: lasciare un campo dov'era vale per chi NON lo tocca. Chi
    // sceglie «codex» nel pannello ha scelto, e la catena cede il posto —
    // altrimenti il selettore sarebbe finto.
    const { step } = chained[0];
    const { rest, choice } = splitToolParams(step.with);
    const after = joinToolParams(rest, { ...choice, tool: "codex" });
    expect(after?.tool).toBe("codex");
  });
});
