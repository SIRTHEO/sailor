// The three entries of the terminals section, apart from the section itself.
//
// The rail names them before the section has arrived, and the section carries
// the emulator: importing them from there is what would fetch three hundred
// kilobytes to draw three words.

export type TerminalsTab = "live" | "projects" | "worktrees";

export const TERMINALS_TABS: { id: TerminalsTab; name: string; about: string }[] = [
  { id: "live", name: "Live", about: "the terminals open now" },
  { id: "projects", name: "Projects", about: "the ones sailor has been opened in" },
  { id: "worktrees", name: "Worktrees", about: "copies of a repository, side by side" },
];
