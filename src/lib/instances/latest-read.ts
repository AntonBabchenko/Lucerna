// A read whose calls can overlap, where only the newest may land: an older read
// that comes back last would put back the state from before the newer one (the
// instance list read beside an activation would put back the previous active
// instance).
//
// An overtaken call does not resolve early. It resolves once the newest read
// has landed, because its caller reads the state right after awaiting it.
export function latestRead<T>(
  read: () => Promise<T>,
  apply: (value: T) => void,
): () => Promise<void> {
  let seq = 0;
  let newest: Promise<void> = Promise.resolve();
  return () => {
    const mine = ++seq;
    const run = (async () => {
      const value = await read();
      // `newest` is a later call's run here, never this one: wait for it (and,
      // through it, for any call later still).
      if (mine !== seq) return newest;
      apply(value);
    })();
    newest = run;
    return run;
  };
}
