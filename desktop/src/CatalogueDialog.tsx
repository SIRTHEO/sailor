// Starting a flow from the catalogue: pick an entry, read what it is and what
// it runs, fill its inputs, say where it goes. What the shell refuses is said.

import { useEffect, useState } from "react";
import { t } from "./i18n";
import {
  firstDestination,
  inputsStillMissing,
  makeFromCatalogue,
  readCatalogue,
  type CatalogueEntry,
  type CatalogueReading,
  type Destination,
  type MadeFlow,
} from "./startfrom";

type Ask = { state: "reading" } | { state: "unreadable"; why: string } | { state: "ready"; reading: CatalogueReading };

const KIND_WORDS = {
  template: "window.catalogue.kind_template",
  example: "window.catalogue.kind_example",
};

const HEADING = "text-[length:var(--text-small)] font-semibold tracking-widest text-[color:var(--text-2)] uppercase";
const NOTE = "text-[length:var(--text-small)] text-[color:var(--text-2)]";
const FAILED = "text-[length:var(--text-small)] text-[color:var(--failed)]";
const FIELD = "rounded-[var(--radius)] border border-solid border-[var(--border)] bg-[var(--surface-0)] px-[var(--space-2)] py-px";

export interface CatalogueDialogProps {
  onClose: () => void;
  onMade: (made: MadeFlow) => void;
}

export function CatalogueDialog({ onClose, onMade }: CatalogueDialogProps) {
  const [ask, setAsk] = useState<Ask>({ state: "reading" });
  const [chosen, setChosen] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [name, setName] = useState("");
  const [place, setPlace] = useState<Destination>("home");
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);

  useEffect(() => {
    let still = true;
    readCatalogue().then(
      (reading) => {
        if (!still) return;
        setAsk({ state: "ready", reading });
        setPlace(firstDestination(reading));
        setChosen(reading.entries[0]?.name ?? null);
      },
      (error) => still && setAsk({ state: "unreadable", why: String(error) }),
    );
    return () => {
      still = false;
    };
  }, []);

  const reading = ask.state === "ready" ? ask.reading : null;
  const entry = reading?.entries.find((one) => one.name === chosen) ?? null;
  const missing = entry ? inputsStillMissing(entry, values) : [];
  const blocked = busy || !entry || entry.refused !== undefined || name.trim() === "" || missing.length > 0;

  function choose(next: CatalogueEntry) {
    setChosen(next.name);
    setValues({});
    setRefusal(null);
  }

  function create() {
    if (!entry || blocked) return;
    const given = Object.fromEntries(Object.entries(values).filter(([, value]) => value.trim() !== ""));
    setBusy(true);
    setRefusal(null);
    makeFromCatalogue(entry.name, name.trim(), given, place).then(
      (made) => {
        setBusy(false);
        onMade(made);
      },
      (error) => {
        setBusy(false);
        setRefusal(String(error));
      },
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--surface-0)]/80 p-[var(--space-4)]">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("window.catalogue.title")}
        className="flex max-h-full w-full max-w-3xl flex-col gap-[var(--space-3)] overflow-auto rounded-[var(--radius)] border border-solid border-[var(--border)] bg-[var(--surface-1)] p-[var(--space-4)]"
      >
        <div className="flex items-center justify-between gap-[var(--space-2)]">
          <h2 className="text-[length:var(--text-mid)] font-semibold">{t("window.catalogue.title")}</h2>
          <button type="button" onClick={onClose}>
            {t("window.catalogue.close")}
          </button>
        </div>

        {ask.state === "reading" && <p className={NOTE}>{t("window.catalogue.reading")}</p>}
        {ask.state === "unreadable" && (
          <p role="alert" className={FAILED}>
            {t("window.catalogue.unreadable", { why: ask.why })}
          </p>
        )}
        {reading && reading.entries.length === 0 && <p className={NOTE}>{t("window.catalogue.empty")}</p>}

        {reading && reading.entries.length > 0 && (
          <div className="flex flex-wrap gap-[var(--space-4)]">
            <ul role="list" aria-label={t("window.catalogue.title")} className="flex w-56 flex-col gap-[var(--space-1)]">
              {reading.entries.map((one) => (
                <li key={one.name}>
                  <button
                    type="button"
                    aria-pressed={one.name === chosen}
                    className="flex w-full flex-col items-start text-left"
                    onClick={() => choose(one)}
                  >
                    <span className="font-mono">{one.name}</span>
                    {one.kind && <span className={NOTE}>{t(KIND_WORDS[one.kind])}</span>}
                  </button>
                </li>
              ))}
            </ul>

            {entry && (
              <section className="flex min-w-0 flex-1 flex-col gap-[var(--space-2)]" aria-label={entry.name}>
                {entry.refused !== undefined ? (
                  <p role="alert" className={FAILED}>
                    {t("window.catalogue.refused_entry", { why: entry.refused })}
                  </p>
                ) : (
                  <p>{entry.purpose}</p>
                )}
                {entry.teaches && (
                  <>
                    <h3 className={HEADING}>{t("window.catalogue.teaches")}</h3>
                    <p>{entry.teaches.capability}</p>
                    <h3 className={HEADING}>{t("window.catalogue.you_will_see")}</h3>
                    <p>{entry.teaches.result}</p>
                  </>
                )}

                <h3 className={HEADING}>{t("window.catalogue.steps")}</h3>
                <ol role="list" className="flex flex-col gap-px">
                  {entry.steps.map((step) => (
                    <li key={step.id} className={NOTE}>
                      <span className="font-mono text-[color:var(--text-1)]">{step.id}</span> · {step.action} ·{" "}
                      {step.deps.length === 0
                        ? t("window.catalogue.first")
                        : t("window.catalogue.after", { deps: step.deps.join(", ") })}
                    </li>
                  ))}
                </ol>

                <h3 className={HEADING}>{t("window.catalogue.inputs")}</h3>
                {entry.inputs.length === 0 && <p className={NOTE}>{t("window.catalogue.no_inputs")}</p>}
                {entry.inputs.map((input) => (
                  <label key={input.name} className="flex flex-col gap-px">
                    <span>
                      <span className="font-mono">{input.name}</span>
                      {input.required && <span className={NOTE}> · {t("window.catalogue.required")}</span>}
                    </span>
                    <input
                      className={FIELD}
                      value={values[input.name] ?? ""}
                      onChange={(event) => setValues((prev) => ({ ...prev, [input.name]: event.target.value }))}
                    />
                    <span className={NOTE}>{input.means}</span>
                  </label>
                ))}

                <label className="flex flex-col gap-px">
                  <span>{t("window.catalogue.name")}</span>
                  <input className={FIELD} value={name} onChange={(event) => setName(event.target.value)} />
                </label>

                <fieldset className="flex flex-col gap-px">
                  <legend>{t("window.catalogue.where")}</legend>
                  <label className="flex items-center gap-[var(--space-1)]">
                    <input
                      type="radio"
                      name="catalogue-place"
                      checked={place === "workspace"}
                      disabled={reading.workspace === null}
                      onChange={() => setPlace("workspace")}
                    />
                    <span>{t("window.catalogue.here")}</span>
                    <span className={`${NOTE} font-mono`}>
                      {reading.workspace ?? t("window.catalogue.no_workspace_here")}
                    </span>
                  </label>
                  <label className="flex items-center gap-[var(--space-1)]">
                    <input
                      type="radio"
                      name="catalogue-place"
                      checked={place === "home"}
                      disabled={reading.home === null}
                      onChange={() => setPlace("home")}
                    />
                    <span>{t("window.catalogue.yours")}</span>
                    <span className={`${NOTE} font-mono`}>{reading.home ?? ""}</span>
                  </label>
                </fieldset>

                <div>
                  <button type="button" className="is-primary" disabled={blocked} onClick={create}>
                    {busy ? t("window.catalogue.creating") : t("window.catalogue.create")}
                  </button>
                </div>
                {refusal !== null && (
                  <p role="alert" className={FAILED}>
                    {t("window.catalogue.refused", { why: refusal })}
                  </p>
                )}
              </section>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
