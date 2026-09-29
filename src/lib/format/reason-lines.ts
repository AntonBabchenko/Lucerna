/**
 * The lines a batch report gives for what it could not do: each reason once, after the names it
 * stopped — «Sodium, Indium: Не удалось связаться с сервером…». One grouping for every such
 * report (the update run, a drop that left files behind, the repair behind «Fix all» and «Fix and
 * launch»), so a reason is never said twice and never said without whom it concerns.
 *
 * In the order the reasons first came; the names in the order they came, as given — a caller whose
 * steps can name one mod twice for one reason drops the repeat itself.
 */
export function reasonLines(failures: readonly { name: string; reason: string }[]): string[] {
  const byReason = new Map<string, string[]>();
  for (const { name, reason } of failures) {
    byReason.set(reason, [...(byReason.get(reason) ?? []), name]);
  }
  return [...byReason].map(([reason, names]) => `${names.join(', ')}: ${reason}`);
}
