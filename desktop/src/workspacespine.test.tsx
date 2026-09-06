/**
 * A workspace has several trees, and the column has to say so: one project
 * checked out four times, listed flat, reads as four projects.
 */
import { describe, expect, test } from "vitest";
import { grouped, treeName } from "./workspacetrees";
import type { Project } from "./workspaces";

function tree(name: string, root: string, last_seen: number, current = false): Project {
  return { name, root, first_seen: 1, last_seen, standing: "declared", current };
}

describe("the workspace and its trees", () => {
  test("FOUR CHECKOUTS OF ONE PROJECT ARE ONE ROW WITH FOUR TREES", () => {
    const seen = [
      tree("a-home", "/t/a-home", 40),
      tree("a-home", "/t/branches/first", 60),
      tree("another", "/t/another", 50),
      tree("a-home", "/t/branches/second", 30),
    ];

    const projects = grouped(seen);

    expect(projects.map((one) => one.name)).toEqual(["a-home", "another"]);
    expect(projects[0].trees.map((one) => treeName(one.root))).toEqual([
      "first",
      "a-home",
      "second",
    ]);
  });

  test("THE MOST RECENTLY OPENED PROJECT COMES FIRST, by its most recent tree", () => {
    const seen = [tree("older", "/t/older", 10), tree("newer", "/t/branches/x", 900)];

    expect(grouped(seen).map((one) => one.name)).toEqual(["newer", "older"]);
  });

  /** The tree is named by its folder: two checkouts differ by that word alone. */
  test("A TREE IS NAMED BY ITS LAST SEGMENT", () => {
    expect(treeName("/t/branches/first")).toBe("first");
    expect(treeName("/t/branches/first/")).toBe("first");
  });
});
