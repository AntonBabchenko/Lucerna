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
  modInstallProgress: null as ((payload: { payload: unknown }) => void) | null,
  modInstalled: null as ((payload: { payload: unknown }) => void) | null,
  modInstallFailed: null as ((payload: { payload: unknown }) => void) | null,
}));

vi.mock('$lib/ipc/bindings', () => {
  type Key =
    | 'installProgress'
    | 'processSpawned'
    | 'processExited'
    | 'modInstallProgress'
    | 'modInstalled'
    | 'modInstallFailed';
  const makeEvent = (key: Key) => ({
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
      modInstallProgress: makeEvent('modInstallProgress'),
      modInstalled: makeEvent('modInstalled'),
      modInstallFailed: makeEvent('modInstallFailed'),
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

  it('shows mod download progress and percent when modInstallProgress fires', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: {
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 50,
        bytes_total: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Downloading mod');
    expect(container.textContent).toContain('50%');
  });

  it('shows verifying mod phase without percent', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: {
        phase: 'verifying',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Verifying mod');
    expect(container.textContent).not.toContain('%');
  });

  it('shows copying mod phase', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: { phase: 'copying', instance_id: 'i', project_id: 'p' },
    });
    await flush();
    expect(container.textContent).toContain('Copying mod');
  });

  it('mod install progress takes precedence over install progress', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.installProgress?.({
      payload: { phase: 'libraries', files_done: 5, files_total: 10, bytes_done: 0 },
    });
    await flush();
    expect(container.textContent).toContain('Downloading libraries');
    listeners.modInstallProgress?.({
      payload: {
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 25,
        bytes_total: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Downloading mod');
    expect(container.textContent).not.toContain('Downloading libraries');
  });

  it('clears mod row on modInstalled event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: {
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 50,
        bytes_total: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Downloading mod');
    listeners.modInstalled?.({
      payload: { instance_id: 'i', sha1: 'abc', filename: 'm.jar', name: 'Mod' },
    });
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });

  it('clears mod row on modInstallFailed event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: {
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 50,
        bytes_total: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Downloading mod');
    listeners.modInstallFailed?.({
      payload: {
        instance_id: 'i',
        project_id: 'p',
        error: { type: 'msg', message: 'boom' },
      },
    });
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });

  it('clears mod row on processSpawned event', async () => {
    const { container } = render(PhaseStatusRow);
    await flush();
    listeners.modInstallProgress?.({
      payload: {
        phase: 'downloading',
        instance_id: 'i',
        project_id: 'p',
        bytes_done: 50,
        bytes_total: 100,
      },
    });
    await flush();
    expect(container.textContent).toContain('Downloading mod');
    listeners.processSpawned?.({ payload: { pid: 1234, version_id: 'test' } });
    await flush();
    expect(container.textContent?.trim()).toBe('');
  });
});
