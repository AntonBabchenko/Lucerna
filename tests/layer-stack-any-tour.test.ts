import { beforeEach, describe, expect, it } from 'vitest';
import {
  __resetLayers,
  anyTourRunning,
  insertTour,
  newLayerId,
  pushLayer,
} from '../src/lib/ui/layer-stack.svelte';

// A page-level host that mounts its tour while another tour is in the stack is refused by
// `insertTour` and stays inert until re-mounted; `anyTourRunning` is what such a host waits on
// (the deps tour, spec 2026-10-08 §3.1).
describe('anyTourRunning', () => {
  beforeEach(() => __resetLayers());

  it('is false with nothing open, and with only a modal or a popover open', () => {
    expect(anyTourRunning()).toBe(false);
    pushLayer(newLayerId('modal'), 'modal', () => {});
    pushLayer(newLayerId('popover'), 'popover', () => {});
    expect(anyTourRunning()).toBe(false);
  });

  it('is true while a tour is in the stack, on top or stepped aside, and false once it is released', () => {
    const release = insertTour(newLayerId('tour'), null, () => {});
    expect(release).not.toBeNull();
    expect(anyTourRunning()).toBe(true);
    // Stepped aside under a popover opened over it: still running.
    pushLayer(newLayerId('popover'), 'popover', () => {});
    expect(anyTourRunning()).toBe(true);
    release?.();
    expect(anyTourRunning()).toBe(false);
  });
});
