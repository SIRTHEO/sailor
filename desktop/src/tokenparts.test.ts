import { describe, expect, test } from "vitest";
import { TOKEN_PARTS, seenTokens } from "./History";
import type { ModelCall } from "./engine";
import source from "./engine.ts?raw";

/**
 * **THE WINDOW REDOES A SUM THE ENGINE ALSO MAKES**, so a part missing on this
 * side makes a row disagree with the total printed above it. `cache_write_long`
 * was missing, which is the dearest entry of the lot.
 */

const call = (over: Partial<ModelCall>): ModelCall => ({
  call_id: "c", step_id: null, purpose: "p", cli: "e",
  requested_model: "", actual_model: "m",
  input_tokens: null, output_tokens: null, cached_tokens: null,
  cache_write_tokens: null, cache_write_long_tokens: null, total_tokens: null,
  turns: null, cost_micros: null, declared_cost_micros: null,
  error_type: null, started_at: 1, ended_at: 2,
  engine_identity: { kind: "not_a_known_engine" }, ...over,
});

describe("the tokens counted from one call", () => {
  test("EVERY TOKEN FIELD OF A CALL IS COUNTED, or the row contradicts the total", () => {
    const fields = [...source.matchAll(/^ {2}(\w+_tokens):/gm)].map((one) => one[1]);
    const declared = fields.filter((one) => one !== "total_tokens");
    // The control: a parse that found nothing would let the loop pass over an
    // empty list and guard no field at all.
    expect(declared.length, "no token field parsed out of the call").toBeGreaterThan(3);
    for (const field of declared) {
      expect(TOKEN_PARTS as readonly string[], `«${field}» crosses the wire uncounted`).toContain(field);
    }
  });

  test("a long cache write on its own is tokens, not silence", () => {
    expect(seenTokens(call({ cache_write_long_tokens: 900 }))).toBe(900);
  });

  test("AND NO NUMBER AT ALL IS STILL NOT ZERO", () => {
    expect(seenTokens(call({})), "a call that declared nothing did not spend nothing").toBe(-1);
    expect(seenTokens(call({ total_tokens: 7 })), "the engine's own total is the fallback").toBe(7);
  });
});
