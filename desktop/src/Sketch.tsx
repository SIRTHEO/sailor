// The whiteboard. Blocks with a kind and words, arrows between them, and one
// button: the drawing goes to `draft-a-flow`, which writes a flow that stands
// where this person's flows live. The board draws nothing itself.

import { useEffect, useMemo, useRef, useState } from "react";
import { listenToRuns, startRun } from "./engine";
import { t } from "./i18n";
import { KIND_LABEL } from "./StepNode";
import { SKETCH_KINDS, loadSketch, newBlock, numberOf, saveSketch, sketchText, type Block } from "./whiteboard";

export const DRAFTING_FLOW = "draft-a-flow";

type Drafting = { state: "idle" } | { state: "running"; runId: string } | { state: "ended"; status: string } | { state: "failed"; why: string };

export function Sketch({
  native,
  onStarted,
  onDrafted,
}: {
  native: boolean;
  /** The run that writes the draft, so whoever holds the runs can show it. */
  onStarted: (runId: string) => void;
  /** The run ended: the flows on disk may have one more. */
  onDrafted: () => void;
}) {
  const storage = typeof window === "undefined" ? null : window.localStorage;
  const [blocks, setBlocks] = useState<Block[]>(() => loadSketch(storage));
  const [drafting, setDrafting] = useState<Drafting>({ state: "idle" });
  const next = useRef(blocks.length + 1);

  useEffect(() => saveSketch(storage, blocks), [blocks, storage]);

  useEffect(() => {
    if (!native || drafting.state !== "running") return;
    let stop: (() => void) | null = null;
    let dropped = false;
    void listenToRuns((event) => {
      if (event.run_id !== drafting.runId || event.kind !== "run_ended") return;
      const payload = event.payload as { status?: unknown } | null;
      setDrafting({ state: "ended", status: typeof payload?.status === "string" ? payload.status : "ended" });
      onDrafted();
    }).then((result) => {
      if ("why" in result) return;
      if (dropped) result.stop();
      else stop = result.stop;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [native, drafting, onDrafted]);

  const text = useMemo(() => sketchText(blocks), [blocks]);

  function add() {
    const id = `b${next.current}`;
    next.current += 1;
    setBlocks((prev) => [...prev, newBlock(id, prev.length === 0 ? "trigger" : "engine")]);
  }

  function update(id: string, change: Partial<Block>) {
    setBlocks((prev) => prev.map((block) => (block.id === id ? { ...block, ...change } : block)));
  }

  function remove(id: string) {
    setBlocks((prev) =>
      prev.filter((block) => block.id !== id).map((block) => ({ ...block, after: block.after.filter((from) => from !== id) })),
    );
  }

  function toggleArrow(id: string, from: string) {
    setBlocks((prev) =>
      prev.map((block) =>
        block.id === id
          ? { ...block, after: block.after.includes(from) ? block.after.filter((one) => one !== from) : [...block.after, from] }
          : block,
      ),
    );
  }

  async function draft() {
    try {
      const started = await startRun(DRAFTING_FLOW, text);
      setDrafting({ state: "running", runId: started.run_id });
      onStarted(started.run_id);
    } catch (error) {
      setDrafting({ state: "failed", why: String(error) });
    }
  }

  return (
    <div className="sketch">
      <div className="sketch__head">
        <h2 className="now__title">{t("window.sketch.title")}</h2>
        <p className="now__mute">{t("window.sketch.lead")}</p>
      </div>
      <div className="sketch__board">
        {blocks.map((block, index) => (
          <div className="sketch__block" key={block.id} data-kind={block.kind}>
            <div className="sketch__block-head">
              <span className="sketch__number">{index + 1}</span>
              <select
                className="sketch__kind"
                aria-label={t("window.sketch.kind")}
                value={block.kind}
                onChange={(event) => update(block.id, { kind: event.target.value as Block["kind"] })}
              >
                {SKETCH_KINDS.map((kind) => (
                  <option key={kind} value={kind}>
                    {KIND_LABEL[kind]}
                  </option>
                ))}
              </select>
              <button type="button" className="sketch__remove" onClick={() => remove(block.id)} title={t("window.sketch.remove")}>
                ×
              </button>
            </div>
            <textarea
              className="sketch__words"
              placeholder={t("window.sketch.words")}
              value={block.text}
              onChange={(event) => update(block.id, { text: event.target.value })}
            />
            {index > 0 && (
              <div className="sketch__after">
                <span className="now__mute">{t("window.sketch.after")}</span>
                {blocks
                  .filter((other) => other.id !== block.id)
                  .map((other) => (
                    <button
                      type="button"
                      key={other.id}
                      className="sketch__arrow"
                      data-on={block.after.includes(other.id) || undefined}
                      onClick={() => toggleArrow(block.id, other.id)}
                    >
                      {numberOf(blocks, other.id)}
                    </button>
                  ))}
              </div>
            )}
          </div>
        ))}
        <button type="button" className="sketch__add" onClick={add}>
          {t("window.sketch.add")}
        </button>
      </div>
      <div className="sketch__foot">
        <button
          type="button"
          className="sketch__draft"
          disabled={!native || blocks.length === 0 || drafting.state === "running"}
          onClick={() => void draft()}
        >
          {t("window.sketch.draft")}
        </button>
        {drafting.state === "running" && <span className="now__mute">{t("window.sketch.drafting")}</span>}
        {drafting.state === "ended" && (
          <span className="now__mute">{t("window.sketch.ended", { status: drafting.status })}</span>
        )}
        {drafting.state === "failed" && <span className="now__why">{drafting.why}</span>}
        {!native && <span className="now__mute">{t("window.sketch.outside")}</span>}
      </div>
      <details className="sketch__text">
        <summary className="now__mute">{t("window.sketch.as_text")}</summary>
        <pre>{text}</pre>
      </details>
    </div>
  );
}
