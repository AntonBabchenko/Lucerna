// "May Lucerna restart — or close — right now?" The shared core of the gate the Storage panel
// (data-folder move) and the Updates panel (an update closes the app) both put in front of their
// action. Extracted so the two cannot drift: the rules are the same, only the sentences and the
// moments to ask differ, and those stay with each panel.
//
// Rules (StoragePanel's, unchanged):
// - `checking` until the first answer, and while an answer is pending: nothing is allowed on an
//   unknown state.
// - Only an EXACT 'none' opens the gate. `restart_blocked` is not wrapped in typedError, so a
//   failure is a rejection; a rejection, a null, a malformed token — anything that is not one of
//   the backend's answers — is 'unknown': "could not ask" is "could not tell".
// - A slower, older query never overwrites a newer answer.
//
// No `$effect` here: the panels decide WHEN to ask (on mount, when an update is offered, after a
// refused action, on Check again), so this is a plain factory over `$state` — the
// `createMcVersions` shape minus the effects, and nothing to dispose.

import { commands, type RestartBlock } from '$lib/ipc/bindings';

export type RestartGate = RestartBlock | 'checking';

export function createRestartGate() {
  let block = $state<RestartGate>('checking');
  let seq = 0;

  async function recheck(): Promise<void> {
    const mine = ++seq;
    block = 'checking';
    let next: RestartBlock = 'unknown';
    try {
      const answer: unknown = await commands.restartBlocked();
      if (answer === 'none' || answer === 'running' || answer === 'busy') next = answer;
    } catch {
      // "Could not ask" is "could not tell": stay blocked.
      next = 'unknown';
    }
    if (mine === seq) block = next;
  }

  return {
    get block(): RestartGate {
      return block;
    },
    recheck,
  };
}
