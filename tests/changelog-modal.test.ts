import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// `vi.mock` factories are hoisted above top-level declarations, so the mock fn
// must come from `vi.hoisted` (a bare `const` would be uninitialised when the
// hoisted factory runs).
const { modsChangelog } = vi.hoisted(() => ({ modsChangelog: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { modsChangelog } }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import ChangelogModal from '$lib/mods/ChangelogModal.svelte';

const props = {
  source: 'modrinth' as const,
  projectId: 'sodium',
  title: 'Sodium 0.5.8 → 0.6.0',
  targetVersionId: 'v3',
  baseVersionId: 'v1',
  onClose: () => {},
};

describe('ChangelogModal', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders a section per version, newest first', async () => {
    modsChangelog.mockResolvedValue({
      status: 'ok',
      data: {
        sections: [
          {
            version_id: 'v3',
            version_number: '0.6.0',
            published_at: '2026-06-03T00:00:00Z',
            body_html: '<p>new</p>',
          },
          {
            version_id: 'v2',
            version_number: '0.5.9',
            published_at: '2026-05-01T00:00:00Z',
            body_html: '<p>fixes</p>',
          },
        ],
        truncated: null,
      },
    });
    render(ChangelogModal, { props });
    expect(await screen.findByText('0.6.0')).toBeTruthy();
    expect(screen.getByText('0.5.9')).toBeTruthy();
  });

  it('shows the empty placeholder when there are no sections', async () => {
    modsChangelog.mockResolvedValue({ status: 'ok', data: { sections: [], truncated: null } });
    render(ChangelogModal, { props });
    expect(await screen.findByText(/No changelog/i)).toBeTruthy();
  });

  it('shows a retry control on failure', async () => {
    modsChangelog.mockResolvedValue({
      status: 'error',
      error: { kind: 'ModsNetwork', url: 'x', details: 'boom' },
    });
    render(ChangelogModal, { props });
    expect(await screen.findByText(/Try again/i)).toBeTruthy();
  });
});
