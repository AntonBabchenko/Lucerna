import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { TaskDetail } from '$lib/ipc/bindings';
import TaskReportModal from '$lib/tasks/TaskReportModal.svelte';
import type { Task } from '$lib/tasks/types';

// Fixture rows — one per DetailOutcome kind, plus a SECOND `installed` row
// (cached, not downloaded) so the downloaded/cached distinction is actually
// exercised rather than assumed from a single sample.
const DOWNLOADED_LINKED: TaskDetail = {
  name: 'sodium.jar',
  install_path: 'C:/instances/x/mods/sodium.jar',
  origin: 'modrinth',
  host: 'cdn.modrinth.com',
  bytes: 1_500_000,
  sha1: 'aaaa1111',
  outcome: { kind: 'installed', fetched: 'downloaded', placement: 'linked' },
};
const CACHED_LINKED: TaskDetail = {
  name: 'fabric-api.jar',
  install_path: 'C:/instances/x/mods/fabric-api.jar',
  origin: 'modrinth',
  host: 'cdn.modrinth.com',
  bytes: 900_000,
  sha1: 'bbbb2222',
  outcome: { kind: 'installed', fetched: 'cached', placement: 'linked' },
};
const UNCHANGED: TaskDetail = {
  name: 'iris.jar',
  install_path: 'C:/instances/x/mods/iris.jar',
  origin: 'modrinth',
  host: null,
  bytes: 500_000,
  sha1: 'cccc3333',
  outcome: { kind: 'unchanged' },
};
const SKIPPED: TaskDetail = {
  name: 'huge-pack.zip',
  install_path: 'C:/instances/x/mods/huge-pack.zip',
  origin: 'archive',
  host: null,
  bytes: null,
  sha1: null,
  outcome: { kind: 'skipped', reason: 'Exceeded per-file size cap' },
};
const FAILED: TaskDetail = {
  name: 'broken-mod.jar',
  install_path: 'C:/instances/x/mods/broken-mod.jar',
  origin: 'curseforge',
  host: 'edge.forgecdn.net',
  bytes: 200_000,
  sha1: null,
  outcome: { kind: 'failed', reason: 'Checksum mismatch' },
};

const ALL_DETAILS = [DOWNLOADED_LINKED, CACHED_LINKED, UNCHANGED, SKIPPED, FAILED];

function makeTask(details: TaskDetail[]): Task {
  return {
    id: 'task-1',
    kind: 'mod-install',
    scope: { instanceId: 'inst-1' },
    title: 'Create All',
    phase: null,
    progress: null,
    rate: null,
    state: 'ok',
    lane: 'concurrent',
    caps: { cancellable: false, reorderable: false },
    details,
    startedAt: 0,
    finishedAt: 1000,
  };
}

describe('TaskReportModal', () => {
  beforeAll(() => locale.set('en'));

  it('filter chips narrow the list to the chosen outcome', async () => {
    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    expect(screen.getByText('sodium.jar')).toBeTruthy();
    expect(screen.getByText('huge-pack.zip')).toBeTruthy();

    await fireEvent.click(screen.getByRole('radio', { name: /skipped/i }));

    expect(screen.getByText('huge-pack.zip')).toBeTruthy();
    expect(screen.queryByText('sodium.jar')).toBeNull();
    expect(screen.queryByText('iris.jar')).toBeNull();
    expect(screen.queryByText('broken-mod.jar')).toBeNull();
  });

  it('a row expands to reveal sha1 / install path / host', async () => {
    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    expect(screen.queryByText('aaaa1111')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: /sodium\.jar/i }));

    expect(screen.getByText('aaaa1111')).toBeTruthy();
    expect(screen.getByText('cdn.modrinth.com')).toBeTruthy();
    expect(screen.getByText('C:/instances/x/mods/sodium.jar')).toBeTruthy();
  });

  it('a row that was both downloaded and linked does NOT read as "cached"', () => {
    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    const row = screen.getByText('sodium.jar').closest('li');
    expect(row).not.toBeNull();
    const text = (row as HTMLLIElement).textContent ?? '';
    expect(text.toLowerCase()).not.toContain('cached');
    expect(text.toLowerCase()).toContain('downloaded');
    expect(text.toLowerCase()).toContain('linked');
  });

  it('the cached+linked row does NOT read as "not downloaded" and is distinguishable from the downloaded row', () => {
    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    const row = screen.getByText('fabric-api.jar').closest('li');
    const text = ((row as HTMLLIElement).textContent ?? '').toLowerCase();
    expect(text).toContain('cached');
    expect(text).toContain('linked');
    expect(text).not.toContain('downloaded');
  });

  it('an unchanged row is visibly distinct from an installed row', () => {
    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    const installedRow = screen.getByText('sodium.jar').closest('li') as HTMLLIElement;
    const unchangedRow = screen.getByText('iris.jar').closest('li') as HTMLLIElement;
    expect(within(installedRow).getByText('Installed')).toBeTruthy();
    expect(within(unchangedRow).getByText('Unchanged')).toBeTruthy();
    expect(within(unchangedRow).queryByText('Installed')).toBeNull();
  });

  it('Copy report reflects the active filter', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });

    render(TaskReportModal, { props: { task: makeTask(ALL_DETAILS), onClose: vi.fn() } });
    await fireEvent.click(screen.getByRole('radio', { name: /failed/i }));
    await fireEvent.click(screen.getByRole('button', { name: /copy report/i }));

    expect(writeText).toHaveBeenCalledTimes(1);
    const copied = writeText.mock.calls[0][0] as string;
    expect(copied).toContain('broken-mod.jar');
    expect(copied).not.toContain('sodium.jar');
    expect(copied).not.toContain('iris.jar');
    expect(copied).not.toContain('huge-pack.zip');

    vi.unstubAllGlobals();
  });
});
