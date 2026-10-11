// Last-click-wins for an instance's Minecraft version and loader (spec D8 of
// the 2026-09-18 loader-picker work). Rapid loader clicks used to fire
// CONCURRENT commands; completions trickled back in arbitrary order after the
// user stopped clicking, every refresh re-rendered the picker to that command's
// result — «загрузчики сами переключаются» — and the final value on disk was
// whichever command happened to LAND last, not the last click.
//
// A toggle wants last-click-wins, not a replay of every intermediate click:
// each instance has one lane; `next` holds only the newest queued change (older
// queued ones are discarded — the cancellation the maintainer asked for), one
// command is in flight at a time, and a command that got overtaken by a newer
// click checks `cfgSuperseded` after its await and stays silent, because
// whatever it did is being overwritten right now. The backend command itself
// cannot be aborted mid-write; muting its side effects is the cancellable half.
//
// Module state, not component state: the Settings tab unmounts on every tab
// switch, and a remounted Game section must join the change already in flight,
// not start a second queue whose command could land last.
//
// NOTE the promise `enqueueCfgChange` returns says nothing about `run`'s
// outcome: for a change queued behind another it resolves at once. A caller
// that needs the outcome resolves its own promise from inside `run`, and passes
// `discard` to learn that its entry was replaced before it ever ran.

type Entry = { run: () => Promise<void>; discard: () => void };
type Lane = { inFlight: boolean; next: Entry | null };

const lanes = new Map<string, Lane>();
// Reactive mirror of "a change is in flight", read by the Game section to show
// itself busy after a remount.
const busy = $state<Record<string, true>>({});

export async function enqueueCfgChange(
  id: string,
  run: () => Promise<void>,
  discard: () => void = () => {},
): Promise<void> {
  let lane = lanes.get(id);
  if (!lane) {
    lane = { inFlight: false, next: null };
    lanes.set(id, lane);
  }
  lane.next?.discard();
  lane.next = { run, discard };
  if (lane.inFlight) return;
  lane.inFlight = true;
  busy[id] = true;
  try {
    while (lane.next) {
      const current: Entry = lane.next;
      lane.next = null;
      try {
        await current.run();
      } catch {
        // Each entry surfaces its own errors; swallowing here only keeps a
        // rejected entry from wedging the loop for the change behind it.
      }
    }
  } finally {
    lane.inFlight = false;
    delete busy[id];
    lanes.delete(id);
  }
}

/** A newer change for this instance is queued behind the running one. */
export function cfgSuperseded(id: string): boolean {
  return lanes.get(id)?.next != null;
}

/** A change for this instance is running or queued. */
export function cfgBusy(id: string): boolean {
  return busy[id] === true;
}

/** Tests only. */
export function __resetCfgQueueForTest(): void {
  lanes.clear();
  for (const k of Object.keys(busy)) delete busy[k];
}
