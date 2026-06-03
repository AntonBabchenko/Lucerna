import { commands, events, type VerifyReport } from '$lib/ipc/bindings';

export type IntegrityState = 'idle' | 'verifying' | 'report' | 'repairing';

/**
 * Rune-composable state machine for the Integrity section.
 * `instanceId()` / `isRunning()` are getters so the section reacts to the
 * selected instance / running state without prop threading.
 */
export function createIntegrity(instanceId: () => string, isRunning: () => boolean) {
  let state = $state<IntegrityState>('idle');
  let report = $state<VerifyReport | null>(null);
  let error = $state<string | null>(null);
  let filesDone = $state(0);
  let filesTotal = $state(0);

  let unlisten: (() => void) | null = null;
  // Wrapped in $effect.root so the listener has an owner outside a component;
  // torn down via dispose(). Guarded so it stays inert under vitest (no Svelte
  // runtime) — see the installed/* composables for the same pattern.
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
    /* no Svelte runtime (vitest) — effect inert, which is fine for unit tests */
  }

  async function verify() {
    if (isRunning() || state === 'verifying' || state === 'repairing') return;
    error = null;
    state = 'verifying';
    filesDone = 0;
    filesTotal = 0;
    const res = await commands.verifyInstance(instanceId());
    if (res.status === 'ok') {
      report = res.data;
      state = 'report';
    } else {
      error = String(res.error);
      state = 'idle';
    }
  }

  async function repair() {
    if (isRunning() || state === 'repairing') return;
    error = null;
    state = 'repairing';
    filesDone = 0;
    filesTotal = 0;
    const res = await commands.repairInstance(instanceId());
    if (res.status === 'ok') {
      report = res.data;
    } else {
      error = String(res.error);
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
    get report() {
      return report;
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
    get problemCount() {
      return report?.problems.length ?? 0;
    },
    verify,
    repair,
    dispose,
  };
}
