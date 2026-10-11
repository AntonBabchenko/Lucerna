import { untrack } from 'svelte';
import { onWindowFocus } from './window-focus';

/**
 * Re-read a list when the user comes back to the launcher window — from the file manager the
 * Add-ons folder button opened, with files dropped in by hand. A return while the list cannot be
 * read right now (it is hidden under Browse, or one of its own writes runs) is remembered and read
 * as soon as it can be. Call during component initialisation.
 */
export function rereadOnReturn(list: {
  /** There is something to read: a profile is selected and the client panel shows. */
  active: () => boolean;
  /** This list is the one on screen. */
  visible: () => boolean;
  /** No write of this list runs — one that does re-reads the list itself when done. */
  idle: () => boolean;
  reread: () => void;
}): void {
  // A return the list could not answer yet.
  let missed = $state(false);
  $effect(() => {
    if (!list.active()) return;
    return onWindowFocus(() => {
      if (list.visible() && list.idle()) list.reread();
      else missed = true;
    });
  });
  $effect(() => {
    if (!missed || !list.active() || !list.visible() || !list.idle()) return;
    missed = false;
    // What the read itself touches must not become this effect's dependencies.
    untrack(list.reread);
  });
}
