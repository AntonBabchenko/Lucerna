export type StatusKind = 'running' | 'pick_version' | 'installing' | 'needs_install' | 'ready';
export type StatusTone = 'ok' | 'warn' | 'accent';

export interface StatusDescriptor {
  kind: StatusKind;
  tone: StatusTone;
}

/**
 * Read-only launch-state descriptor mirroring the sidebar's Play/Install
 * button logic. Precedence: running > pick_version > installing >
 * needs_install > ready.
 */
export function deriveStatus(
  instance: { mc_version: string; ready: boolean },
  running: boolean,
  installing: boolean,
): StatusDescriptor {
  if (running) return { kind: 'running', tone: 'ok' };
  if (instance.mc_version === '') return { kind: 'pick_version', tone: 'warn' };
  if (installing) return { kind: 'installing', tone: 'accent' };
  if (!instance.ready) return { kind: 'needs_install', tone: 'warn' };
  return { kind: 'ready', tone: 'ok' };
}
