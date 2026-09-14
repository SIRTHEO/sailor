// The home lab, drawn like any row of `EnginesScreen`: presence, what it
// carries, how long it took to say so. Per the mandate: it is an engine, not
// a special case, so it earns no screen of its own.

import { residentWords, responseWords, type LabStatus } from "./lab";

export type LabAsk = { state: "asking" } | { state: "asked"; value: LabStatus } | { state: "mute"; why: string };

export function LabEngineRow({ ask }: { ask: LabAsk }) {
  if (ask.state === "asking") {
    return (
      <section className="panel__block" data-presence="undetermined">
        <div className="panel__title">Lab</div>
        <p className="now__mute">Reading the lab…</p>
      </section>
    );
  }
  if (ask.state === "mute") {
    return (
      <section className="panel__block" data-presence="undetermined">
        <div className="panel__title">Lab</div>
        <p className="now__mute" data-bad>I cannot say: {ask.why}</p>
      </section>
    );
  }
  const status = ask.value;
  return (
    <section className="panel__block" data-presence={status.reachable ? "present" : "absent"}>
      <div className="panel__title">Lab</div>
      <dl className="now__kv">
        <dt>on this machine</dt>
        <dd>{status.reachable ? "answers" : "does not answer"}</dd>
        {status.reachable && (
          <>
            <dt>resident</dt>
            <dd>{residentWords(status.resident_models)}</dd>
            <dt>last reading</dt>
            <dd>{responseWords(status.response_time_secs)}</dd>
          </>
        )}
        {!status.reachable && status.error !== null && (
          <>
            <dt>why</dt>
            <dd>{status.error}</dd>
          </>
        )}
      </dl>
    </section>
  );
}
