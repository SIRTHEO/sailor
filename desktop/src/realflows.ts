// **THE SHIPPED FLOWS, READ ONCE FOR EVERY TEST THAT NEEDS THEM.**
//
// Only the tests import this; nothing the window ships should, or the globs
// would drag every flow file into the bundle. It sits in `desktop/src` because
// `import.meta.glob` resolves its patterns relative to the file that writes
// them.

// One module and not a helper copied into each test, because the copies are
// what went wrong: `ports.test.tsx` and `StepEditor.test.tsx` each held their
// own reader and their own count of what it would find, and both went red at
// once for a premise neither of them owned.

import systemRs from "../../crates/flow/src/system.rs?raw";

/** The one line in `system.rs` that makes a flow travel inside the binary. */
const INCLUDED = /include_str!\("\.\.\/system\/([^"]+\.flow\.json)"\)/g;

/** Where the shipped flows live, as the glob below sees it. */
const SHIPPED_DIR = "../../crates/flow/system/";

/**
 * The flows shipped **inside the binary**, read from the source that owns
 * the list: the `include_str!` lines of `crates/flow/src/system.rs`, which the
 * compiler checks against the directory on every build. Not a copy typed here
 * — the first copy went stale the same night, when the three were renamed.
 */
export const SHIPPED_WITH_THE_BINARY = [...systemRs.matchAll(INCLUDED)].map((m) => m[1]).sort();

/**
 * The shipped flow files, as raw text. Only the directory that ships, not
 * `flows/`: the one file still there is a template the product hands out,
 * guarded by its own Rust test, and it fed no threshold here — while a file
 * half-written there killed both test files at collection time.
 */
export function readRealFlows(): Array<{ path: string; source: string }> {
  const files = import.meta.glob("../../crates/flow/system/*.flow.json", {
    eager: true,
    query: "?raw",
    import: "default",
  }) as Record<string, string>;
  return Object.keys(files)
    .sort()
    .map((path) => ({ path, source: files[path] }));
}

/** Decodes one flow file, and names it when it cannot. */
export function parseFlow<T>(path: string, source: string): T {
  try {
    return JSON.parse(source) as T;
  } catch (error) {
    throw new Error(`${path}: ${String(error)}`);
  }
}

/**
 * The shipped names missing from a set of paths — empty when all are there.
 * Whole paths, not suffixes: a file of the same name elsewhere is not the one
 * that ships. A test asserts on this rather than on a count so that its
 * failure says *which* file went missing.
 */
export function shippedFlowsMissingFrom(paths: string[]): string[] {
  return SHIPPED_WITH_THE_BINARY.filter((shipped) => !paths.includes(SHIPPED_DIR + shipped));
}
