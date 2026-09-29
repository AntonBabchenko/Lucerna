/**
 * One reason of a batch report, with the names it stopped. It is shown on two lines — the names,
 * then the reason under them — never «Sodium, Indium: Не удалось…»: the reason is a sentence of
 * its own, and after a colon Russian wants a lower-case letter, which lower-casing would give by
 * breaking a name the reason may start with («CurseForge требует…») (plan §5d L4).
 */
export type ReasonLine = { readonly names: string; readonly reason: string };

/** A line of a report — a toast's or a status message's: plain text, or a reason with its names. */
export type ReportLine = string | ReasonLine;

/**
 * The lines a batch report gives for what it could not do: each reason once, with the names it
 * stopped. One grouping for every such report (the update run, a drop that left files behind, the
 * repair behind «Fix all» and «Fix and launch»), so a reason is never said twice and never said
 * without whom it concerns.
 *
 * In the order the reasons first came; the names in the order they came, as given — a caller whose
 * steps can name one mod twice for one reason drops the repeat itself.
 */
export function reasonLines(failures: readonly { name: string; reason: string }[]): ReasonLine[] {
  const byReason = new Map<string, string[]>();
  for (const { name, reason } of failures) {
    byReason.set(reason, [...(byReason.get(reason) ?? []), name]);
  }
  return [...byReason].map(([reason, names]) => ({ names: names.join(', '), reason }));
}
