// Whether a task's progress stream can honestly carry a speed/ETA readout,
// plus a thin adapter onto the machinery that computes one — so callers of
// the task registry do not need to reach into `$lib/servers/` for a generic
// concern. See `$lib/servers/upload-progress-format` for the actual EWMA +
// throttle implementation; this module does not reimplement it.
import {
  advanceProgressDisplay,
  emptyProgressDisplay,
  type ProgressDisplay,
} from '$lib/servers/upload-progress-format';
import type { Task, TaskProgress } from './types';

export { advanceProgressDisplay, emptyProgressDisplay, type ProgressDisplay };

/** Whether a rate readout is honest for this progress shape.
 *
 *  Game install deliberately fails this test: its backend event has no
 *  bytes_total at all, and its bytes_done only advances at whole-file
 *  completion by the file's DECLARED size — a rate derived from it is a
 *  staircase of file sizes, not a transfer rate.
 *
 *  The `total > 0` guard also absorbs specta's `number | null` export, which
 *  applies even to fields Rust declares non-optional. */
export function canShowRate(progress: TaskProgress | null): boolean {
  return progress !== null && progress.unit === 'bytes' && progress.total > 0;
}

/** The task registry's rate shape (`Task['rate']` minus null). */
export type TaskRate = NonNullable<Task['rate']>;

/** Adapts a throttled `ProgressDisplay` snapshot — produced by
 *  `advanceProgressDisplay`, the same EWMA-over-cumulative-bytes machinery
 *  the SFTP upload UI drives — onto the task registry's `Task['rate']`
 *  shape. Pure field rename: `speedBytesPerSec` → `bytesPerSec`,
 *  `etaSecondsValue` → `etaSeconds`. Callers own the throttled state
 *  (`ProgressDisplay`) themselves via `advanceProgressDisplay`/
 *  `emptyProgressDisplay`, re-exported above, and call this once per tick to
 *  get the value `upsertProgress` expects for `rate`. */
export function toTaskRate(display: ProgressDisplay): TaskRate {
  return { bytesPerSec: display.speedBytesPerSec, etaSeconds: display.etaSecondsValue };
}
