import { describe, expect, test } from "vitest";

/**
 * **A DOC BLOCK MUST BE FOLLOWED BY WHAT IT DOCUMENTS.** In JSDoc only the last
 * block before a declaration is attached to it: the first of a pair documents
 * nothing, leaves the tooling, and reads to a person as detached from the thing
 * it explains. So the cap of six lines has one way of being met — shortening —
 * and one way of being cheated: cutting one block into two five-line halves,
 * which keeps the counter green and makes the code worse. This is the check
 * that tells the two apart, and it reads this folder's own sources.
 *
 * Its scope is `desktop/src`, where the defect exists. In Rust the same split
 * costs no meaning — two `///` groups stay one rustdoc on the same item — and
 * the count over the whole tree is `comments_do_not_crowd_out_the_code`.
 */

const sources = import.meta.glob("./*.{ts,tsx}", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

interface DocBlock {
  start: number;
  end: number;
}

/**
 * The `/**`-opened blocks of a file, as line ranges. A block runs to the line
 * carrying its terminator: a blank line does not end it, and does not start a
 * second one either, which is the whole point of the measure below.
 */
export function docBlocksOf(lines: string[]): DocBlock[] {
  const blocks: DocBlock[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].trim().startsWith("/**")) continue;
    const start = i;
    while (i < lines.length && !lines[i].includes("*/")) i += 1;
    blocks.push({ start, end: Math.min(i, lines.length - 1) });
  }
  return blocks;
}

/**
 * Whether real code stands above a line. Imports do not count, nor line or
 * plain block comments: what matters is the first thing a doc block could have
 * been written to document. Only a file's first block comes back false — the
 * preamble, the one shape allowed to stand before another block.
 */
export function codeStandsAbove(lines: string[], before: number): boolean {
  let inImport = false;
  let inComment = false;
  for (let i = 0; i < before; i += 1) {
    const line = lines[i].trim();
    if (inComment) {
      if (line.includes("*/")) inComment = false;
      continue;
    }
    if (line === "" || line.startsWith("//")) continue;
    if (line.startsWith("/*") && !line.startsWith("/**")) {
      if (!line.includes("*/")) inComment = true;
      continue;
    }
    if (inImport) {
      if (line.includes("from ") || line.endsWith(";")) inImport = false;
      continue;
    }
    if (line.startsWith("import ")) {
      if (!line.includes("from ") && !line.endsWith(";")) inImport = true;
      continue;
    }
    return true;
  }
  return false;
}

/**
 * The blocks that document nothing: a doc block the next filled line opens
 * another doc block after. The file preamble is spared — it is the one block
 * JSDoc expects to document a file and not a declaration. Everything else that
 * explains a section rather than a declaration has a plain `/*` to be written
 * in, and saying so is the cure, not an exception.
 */
export function orphanDocBlocks(lines: string[]): DocBlock[] {
  const blocks = docBlocksOf(lines);
  const orphans: DocBlock[] = [];
  for (let i = 0; i < blocks.length - 1; i += 1) {
    const block = blocks[i];
    const next = blocks[i + 1];
    const between = lines.slice(block.end + 1, next.start);
    if (!between.every((line) => line.trim() === "")) continue;
    if (!codeStandsAbove(lines, block.start)) continue;
    orphans.push(block);
  }
  return orphans;
}

describe("a doc block is followed by what it documents", () => {
  test("NO DOC BLOCK IN `desktop/src` IS FOLLOWED BY ANOTHER INSTEAD OF BY CODE", () => {
    const found: string[] = [];
    for (const [path, text] of Object.entries(sources)) {
      const lines = text.split("\n");
      for (const block of orphanDocBlocks(lines)) {
        found.push(`${path}:${block.start + 1}-${block.end + 1}`);
      }
    }
    expect(
      found,
      "these blocks document nothing: another doc block follows them, and JSDoc " +
        "attaches only the last one to the declaration. Join the two — a long " +
        "block that is honest beats two short ones of which one is orphaned — or " +
        "shorten them into one. If the first explains a section and not a " +
        "declaration, write it as a plain block comment, which is what it is",
    ).toEqual([]);
  });

  test("the sources really are read, and they carry doc blocks", () => {
    // Without this the check above would be green on zero files, or on files
    // read as empty strings — the `?raw` glob has been silently empty before.
    expect(Object.keys(sources).length).toBeGreaterThan(20);
    const blocks = Object.values(sources).reduce(
      (total, text) => total + docBlocksOf(text.split("\n")).length,
      0,
    );
    expect(blocks).toBeGreaterThan(100);
  });
});

/**
 * **WHOEVER MEASURES MUST BE MEASURED.** If `docBlocksOf` stopped seeing, the
 * check above would be green forever. The fixtures are built by joining "/*"
 * to a star so that no line of this file itself opens a block: this file is
 * inside the glob it reads, and excluding it would be a hole.
 */
const OPEN = `/*${"*"}`;

describe("the check can still see what it counts", () => {
  test("two blocks in a row are found; a block above code is not", () => {
    // Both fixtures open with a line of code: at the very top of a file the
    // first block is the preamble, and the rule below is the one that says so.
    const split = ["const a = 1;", "", OPEN, " * the first half", " */", "", OPEN, " * the second", " */", "const b = 2;"];
    expect(orphanDocBlocks(split)).toHaveLength(1);

    const joined = ["const a = 1;", "", OPEN, " * one block", " * both halves", " */", "const b = 2;"];
    expect(orphanDocBlocks(joined)).toHaveLength(0);
  });

  test("the file preamble may precede another block; a section header may not", () => {
    const preamble = ['import { a } from "./a";', "", OPEN, " * the file", " */", "", OPEN, " * the constant", " */", "const a = 1;"];
    expect(orphanDocBlocks(preamble)).toHaveLength(0);

    const header = ["const a = 1;", "", OPEN, " * a section", " */", "", OPEN, " * the constant", " */", "const b = 2;"];
    expect(orphanDocBlocks(header)).toHaveLength(1);

    const plain = ["const a = 1;", "", "/* a section */", "", OPEN, " * the constant", " */", "const b = 2;"];
    expect(orphanDocBlocks(plain)).toHaveLength(0);
  });

  test("a blank line does not close a block, so half a pair is not miscounted", () => {
    // The trap the Rust ratchet fell into: counting the run of comment lines
    // rather than the block makes two five-line halves look like two short
    // blocks, which is exactly the reward this check removes.
    const paragraphs = [OPEN, " * one", " *", " * two", " */", "const a = 1;"];
    expect(docBlocksOf(paragraphs)).toHaveLength(1);
  });
});
