export type StatusKind =
  | 'running'
  | 'pick_version'
  | 'installing'
  | 'needs_install'
  | 'mods_unknown'
  | 'mods_blocking'
  | 'ready';
export type StatusTone = 'ok' | 'warn' | 'accent' | 'danger' | 'neutral';

export interface StatusDescriptor {
  kind: StatusKind;
  tone: StatusTone;
}

/**
 * Read-only launch-state descriptor mirroring the sidebar's Play/Install
 * button logic, then — for an instance that is set up — the page pre-flight,
 * the Play gate's own verdict. Precedence: running > pick_version >
 * installing > needs_install > mods_unknown > mods_blocking > ready.
 *
 * `blockingMods` is how many mods stop the game (the page pre-flight's
 * blocking rows, `hasBlocking` applied), or null while that pre-flight has not
 * answered — then nothing is claimed either way: «ready» is only ever the
 * answer of a check that ran (fallback discipline: honesty).
 */
export function deriveStatus(
  instance: { mc_version: string; ready: boolean },
  running: boolean,
  installing: boolean,
  blockingMods: number | null,
): StatusDescriptor {
  if (running) return { kind: 'running', tone: 'ok' };
  if (instance.mc_version === '') return { kind: 'pick_version', tone: 'warn' };
  if (installing) return { kind: 'installing', tone: 'accent' };
  if (!instance.ready) return { kind: 'needs_install', tone: 'warn' };
  if (blockingMods === null) return { kind: 'mods_unknown', tone: 'neutral' };
  if (blockingMods > 0) return { kind: 'mods_blocking', tone: 'danger' };
  return { kind: 'ready', tone: 'ok' };
}
