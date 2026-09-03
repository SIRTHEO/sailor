/**
 * A workspace has several trees, and the column has to say so: one project
 * checked out four times, listed flat, reads as four projects.
 */
import { describe, expect, test } from "vitest";
import { grouped, treeName, whereWeAre } from "./Workspace";
import type { Project } from "./workspaces";

function tree(name: string, root: string, last_seen: number, current = false): Project {
  return { name, root, first_seen: 1, last_seen, standing: "declared", current };
}

describe("the workspace and its trees", () => {
  test("FOUR CHECKOUTS OF ONE PROJECT ARE ONE ROW WITH FOUR TREES", () => {
    const seen = [
      tree("una-casa", "/t/una-casa", 40),
      tree("una-casa", "/t/rami/primo", 60),
      tree("un-altra", "/t/un-altra", 50),
      tree("una-casa", "/t/rami/secondo", 30),
    ];

    const projects = grouped(seen);

    expect(projects.map((one) => one.name)).toEqual(["una-casa", "un-altra"]);
    expect(projects[0].trees.map((one) => treeName(one.root))).toEqual([
      "primo",
      "una-casa",
      "secondo",
    ]);
  });

  test("THE MOST RECENTLY OPENED PROJECT COMES FIRST, by its most recent tree", () => {
    const seen = [tree("vecchia", "/t/vecchia", 10), tree("nuova", "/t/rami/x", 900)];

    expect(grouped(seen).map((one) => one.name)).toEqual(["nuova", "vecchia"]);
  });

  /** «outside every workspace» came back on screen as «outside every w…». */
  test("EACH STATE HAS ITS OWN TWO WORDS, and none of them is a sentence", () => {
    expect(whereWeAre({ state: "asking" }, null).name).toBe("…");
    expect(whereWeAre({ state: "mute", why: "no home to read" }, null)).toEqual({
      name: "unknown",
      tree: "cannot read",
    });
    expect(whereWeAre({ state: "asked", seen: [] }, null)).toEqual({
      name: "no workspace",
      tree: "outside",
    });
    const here = tree("una-casa", "/t/rami/primo", 10, true);
    expect(whereWeAre({ state: "asked", seen: [here] }, here)).toEqual({
      name: "una-casa",
      tree: "primo",
    });
    for (const state of ["asking", "mute", "asked"] as const) {
      const said = whereWeAre(
        state === "mute"
          ? { state, why: "why" }
          : state === "asked"
            ? { state, seen: [] }
            : { state },
        null,
      );
      expect(said.name.length, `«${said.name}» does not fit the column`).toBeLessThan(15);
    }
  });

  /** The tree is named by its folder: two checkouts differ by that word alone. */
  test("A TREE IS NAMED BY ITS LAST SEGMENT", () => {
    expect(treeName("/t/rami/primo")).toBe("primo");
    expect(treeName("/t/rami/primo/")).toBe("primo");
  });
});
