// What an integrity result means for every surface that shows it (spec 2026-10-18; #519's spec
// §5.5 revisited on the maintainer's delegation). Forge files an older Lucerna built differently,
// and Forge outputs that could not be checked because the installer is not saved, are notes, not
// damage: the game launches, and Repair is optional. A result with only notes is shown neutrally
// everywhere — the sidebar draws no glyph, the Overview raises nothing, Verify's toast is a
// success — and the Integrity section is the one place that explains them. Any damage counts as
// it always has, notes included in the count.
import type { CategoryReport, ForgeInstallerGap } from '$lib/ipc/bindings';

export type IntegrityState = 'ok' | 'notes' | 'damaged';

export type IntegritySummary = {
  state: IntegrityState;
  /** Built-differently files, plus one for an installer that kept the outputs unchecked. */
  notes: number;
  /** What every surface counts when there is damage: the result's whole problem count. */
  problemCount: number;
};

type Status = {
  healthy: boolean;
  problem_count: number;
  categories: CategoryReport[];
  forge_installer?: ForgeInstallerGap | null;
};

export function summarizeIntegrity(status: Status): IntegritySummary {
  const builtDifferently = status.categories.reduce((n, c) => n + (c.built_differently ?? 0), 0);
  const notes = builtDifferently + (status.forge_installer ? 1 : 0);
  if (status.healthy) return { state: 'ok', notes: 0, problemCount: 0 };
  // An unhealthy result with nothing counted (an unreadable manifest) is damage, not a note.
  const damaged = status.problem_count - notes;
  const state = notes > 0 && damaged <= 0 ? 'notes' : 'damaged';
  return { state, notes, problemCount: status.problem_count };
}
