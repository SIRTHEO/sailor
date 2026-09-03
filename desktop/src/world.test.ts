/**
 * Which tree a terminal belongs to, and what «outside» means. Trees nest, and
 * taking the first match instead of the deepest files the work under the
 * wrong project.
 */
import { describe, expect, test } from "vitest";
import { ago, outside, reopenIn, treeOf } from "./World";
import type { Project } from "./workspaces";
import type { TerminalSummary } from "./terminal";

function tree(name: string, root: string): Project {
  return { name, root, first_seen: 1, last_seen: 2, standing: "declared", current: false };
}

function terminal(id: string, workspaceRoot: string): TerminalSummary {
  return {
    id,
    workspaceRoot,
    workspaceName: "",
    alive: true,
    processId: 1,
    device: id,
    moved: 0,
    estimatedTokens: 0,
    program: "",
    profile: null,
  };
}

describe("which tree a terminal belongs to", () => {
  const trees = [tree("casa", "/t/casa"), tree("dentro", "/t/casa/dentro")];

  test("THE DEEPEST TREE WINS, because trees nest", () => {
    expect(treeOf("/t/casa/dentro/src", trees)?.root).toBe("/t/casa/dentro");
    expect(treeOf("/t/casa/altro", trees)?.root).toBe("/t/casa");
  });

  test("A TREE CONTAINS ITSELF", () => {
    expect(treeOf("/t/casa", trees)?.root).toBe("/t/casa");
  });

  /** A name that merely starts the same is not inside: `/t/casa-vecchia`. */
  test("A PREFIX OF THE NAME IS NOT A PLACE INSIDE IT", () => {
    expect(treeOf("/t/casa-vecchia", trees)).toBeNull();
  });

  /** Outside is a place: a column that drops it says the work is not there. */
  test("WHAT NO TREE CLAIMS IS OUT, AND STAYS ON THE LIST", () => {
    const all = [terminal("a", "/t/casa/dentro"), terminal("b", "/altrove"), terminal("c", "/t")];

    expect(outside(all, trees).map((one) => one.id)).toEqual(["b", "c"]);
  });
});

describe("how long ago", () => {
  test("the words a person uses when scanning a column", () => {
    expect(ago(100, 120)).toBe("now");
    expect(ago(0, 600)).toBe("10m");
    expect(ago(0, 3600 * 5)).toBe("5h");
    expect(ago(0, 3600 * 24 * 3)).toBe("3d");
  });
});

/**
 * Which tree the window reopens in. Read from the process's directory, a
 * window launched from the Finder starts in `/`: every project on the list
 * and none of them open.
 */
describe("where the window reopens", () => {
  function seen(current: boolean, last: number, root: string): Project {
    return { name: "p", root, first_seen: 1, last_seen: last, standing: "declared", current };
  }

  test("THE MOST RECENTLY WORKED IN, when none is open", () => {
    const list = [seen(false, 10, "/a"), seen(false, 99, "/b"), seen(false, 50, "/c")];

    expect(reopenIn(list)?.root).toBe("/b");
  });

  test("NOTHING TO DO when a tree is already open: it would drag you out of it", () => {
    const list = [seen(false, 99, "/a"), seen(true, 10, "/b")];

    expect(reopenIn(list)).toBeNull();
  });

  test("NOTHING TO DO with no project on record", () => {
    expect(reopenIn([])).toBeNull();
  });
});
