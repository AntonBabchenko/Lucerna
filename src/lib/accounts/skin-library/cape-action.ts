// Decides the cape step of "apply saved skin". The library is account-agnostic
// but capes are owned per account, so a saved cape may not exist for the
// applying account — leave the cape untouched in that case.
export type CapeAction = { kind: 'set'; id: string } | { kind: 'clear' } | { kind: 'skip' };

export function resolveCapeAction(
  capeId: string | null,
  ownedCapeIds: readonly string[],
): CapeAction {
  if (capeId === null) return { kind: 'clear' };
  return ownedCapeIds.includes(capeId) ? { kind: 'set', id: capeId } : { kind: 'skip' };
}
