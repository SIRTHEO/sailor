// **THE REAL FLOWS, READ ONCE FOR EVERY TEST THAT NEEDS THEM.**
//
// Only the tests import this; nothing the window ships should, or the globs
// would drag every flow file into the bundle. It sits in `desktop/src` because
// `import.meta.glob` resolves its patterns relative to the file that writes
// them.

// One module and not a helper copied into each test, because the copies are
// what went wrong: `ports.test.tsx` and `StepEditor.test.tsx` each held their
// own reader and their own count of what it would find, and both went red at
// once on 01/09 for a premise neither of them owned.

/**
 * The flows shipped **inside the binary**, read with `include_str!`. They exist
 * on a machine that just installed Sailor and has no `flows/` at all, so no test
 * can be green for having read nothing while these are named. Whatever else a
 * glob finds is a bonus, not a premise.
 */
export const SHIPPED_WITH_THE_BINARY = [
  "migrate-to-sailor.flow.json",
  "dispatch-the-work.flow.json",
  "what-this-machine-has.flow.json",
];

/**
 * Every flow file readable from here, from **both places they live**. Two globs
 * and not a `..`: the whole root would pull in `target/`.
 */
export function readRealFlows(): Array<{ path: string; source: string }> {
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
  };
  return Object.keys(files)
    .sort()
    .map((path) => ({ path, source: files[path] }));
}

/**
 * The shipped names missing from a set of paths — empty when all are there. A
 * test asserts on this rather than a count so its failure says *which* file
 * went missing.
 */
export function shippedFlowsMissingFrom(paths: string[]): string[] {
  return SHIPPED_WITH_THE_BINARY.filter(
    (shipped) => !paths.some((path) => path.endsWith(shipped)),
  );
}
