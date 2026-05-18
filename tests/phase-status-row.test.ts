import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';

// Capture listener callbacks at module-eval time via vi.hoisted so the
// vi.mock factory (which is hoisted itself) can access them.
const listeners = vi.hoisted(() => ({
  installProgress: null as ((payload: { payload: unknown }) => void) | null,
  processSpawned: null as ((payload: { payload: unknown }) => void) | null,
  processExited: null as ((payload: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => {
  const makeEvent = (key: 'installProgress' | 'processSpawned' | 'processExited') => ({
    listen: (cb: (payload: { payload: unknown }) => void) => {
      listeners[key] = cb;
      return Promise.resolve(() => {});
    },
  });
  return {
    events: {
      installProgress: makeEvent('installProgress'),
      processSpawned: makeEvent('processSpawned'),
      processExited: makeEvent('processExited'),
    },
  };
});

async function flush() {
  // Allow onMount + listener registration to complete.
  await tick();
  await new Promise((r) => setTimeout(r, 0));
  await tick();
}

describe('PhaseStatusRow', () => {
  it('renders nothing before any installProgress event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });

  it('shows phase label after installProgress event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.installProgress?.({
      payload: { phase: 'libraries', files_done: 5, files_total: 10, bytes_done: 0 },
    });
    await flush();
    expect(container.textContent).toContain('Downloading libraries');
  });

  it('clears row on processSpawned event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.installProgress?.({
      payload: { phase: 'libraries', files_done: 5, files_total: 10, bytes_done: 0 },
    });
    await flush();
    expect(container.textContent).toContain('Downloading libraries');
    listeners.processSpawned?.({ payload: { pid: 1234, version_id: 'test' } });
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });

  it('clears row on processExited event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.installProgress?.({
      payload: { phase: 'complete', files_done: 1, files_total: 1, bytes_done: 0 },
    });
    await flush();
    expect(container.textContent).toContain('Install complete');
    listeners.processExited?.({ payload: { code: 1, log_path: '/tmp/log' } });
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });
});
