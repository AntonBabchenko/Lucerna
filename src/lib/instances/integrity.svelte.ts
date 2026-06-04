import { commands, events, type IntegrityStatus, type VerifyReport } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export type IntegrityState = 'idle' | 'verifying' | 'report' | 'repairing';

/**
 * Rune-composable state machine for the Integrity section.
 * `instanceId()` / `isRunning()` are getters; `initial` is the instance's
 * persisted status (shown immediately, no re-scan).
 */
export function createIntegrity(
  instanceId: () => string,
  isRunning: () => boolean,
  initial: IntegrityStatus | null = null,
  /** Called after a verify/repair persists a new status, so the caller can
   *  refresh the instance list (badge + Overview read the persisted field). */
  onPersisted: () => void = () => {},
) {
  let state = $state<IntegrityState>(initial ? 'report' : 'idle');
  let report = $state<VerifyReport | null>(null);
  const stored = $state<IntegrityStatus | null>(initial);
  let liveCheckedAt = $state<number | null>(null);
  let error = $state<string | null>(null);
  let filesDone = $state(0);
  let filesTotal = $state(0);

  let unlisten: (() => void) | null = null;
  let stopRoot: (() => void) | null = null;
  try {
    stopRoot = $effect.root(() => {
      events.verifyProgress
        .listen((e) => {
          if (e.payload.instance_id !== instanceId()) return;
          filesDone = e.payload.files_done;
          filesTotal = e.payload.files_total;
        })
        .then((u) => {
          unlisten = u;
        })
        .catch(() => {});
    });
  } catch {
    /* no Svelte runtime (vitest) — listener inert */
  }

  async function verify() {
    if (isRunning() || state === 'verifying' || state === 'repairing') return;
    const target = instanceId();
    error = null;
    state = 'verifying';
    filesDone = 0;
    filesTotal = 0;
    const res = await commands.verifyInstance(target);
    // The backend persisted the status regardless of UI navigation, so refresh
    // the instance list even if the user switched away mid-scan — otherwise the
    // badge/Overview wouldn't update until the next list refresh / app restart.
    if (res.status === 'ok') onPersisted();
    if (target !== instanceId()) return; // switched away: skip the local UI mutation only
    if (res.status === 'ok') {
      report = res.data;
      liveCheckedAt = Date.now();
      state = 'report';
    } else {
      error = formatError(res.error);
      state = stored ? 'report' : 'idle';
    }
  }

  async function repair() {
    if (isRunning() || state === 'repairing') return;
    const target = instanceId();
    error = null;
    state = 'repairing';
    filesDone = 0;
    filesTotal = 0;
    const res = await commands.repairInstance(target);
    if (res.status === 'ok') onPersisted(); // refresh list even if user switched away
    if (target !== instanceId()) return; // switched away: skip the local UI mutation only
    if (res.status === 'ok') {
      report = res.data;
      liveCheckedAt = Date.now();
    } else {
      error = formatError(res.error);
    }
    state = 'report';
  }

  function dispose() {
    unlisten?.();
    stopRoot?.();
  }

  return {
    get state() {
      return state;
    },
    get error() {
      return error;
    },
    get filesDone() {
      return filesDone;
    },
    get filesTotal() {
      return filesTotal;
    },
    get hasResult() {
      return report !== null || stored !== null;
    },
    get healthy(): boolean | null {
      return report ? report.healthy : stored ? stored.healthy : null;
    },
    get categories() {
      return report ? report.categories : (stored?.categories ?? []);
    },
    get problemCount() {
      return report ? report.problems.length : (stored?.problem_count ?? 0);
    },
    get checkedAt(): number | null {
      return report ? liveCheckedAt : (stored?.checked_unix_ms ?? null);
    },
    get report() {
      return report;
    },
    verify,
    repair,
    dispose,
  };
}
