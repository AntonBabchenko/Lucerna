import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ImportProgressView from '$lib/modpacks/ImportProgressView.svelte';

// Channel-vs-event note: Task 9's `commands.modpackImport` threads two
// `Channel<T>` parameters (not global events), so `ImportProgressView`
// is render-only and receives the latest values via props. The parent
// (`ModpacksTab`) wires the channels and pushes updates here. These
// tests confirm the rendering surface, which is the contract the parent
// depends on.

describe('ImportProgressView', () => {
  it('renders nothing when there is no phase yet', () => {
    const { container } = render(ImportProgressView, {
      props: { phase: null, modBytes: null },
    });
    expect(container.querySelector('[data-testid="import-progress-view"]')).toBeNull();
  });

  it('renders the inspecting phase label', () => {
    const { getByText } = render(ImportProgressView, {
      props: {
        phase: { phase: 'inspecting' },
        modBytes: null,
      },
    });
    expect(getByText(/Inspecting pack/)).toBeTruthy();
  });

  it('renders the creating-instance phase with the chosen name', () => {
    const { getByText } = render(ImportProgressView, {
      props: {
        phase: { phase: 'creating_instance', name: 'All The Mods 9' },
        modBytes: null,
      },
    });
    expect(getByText(/Creating instance All The Mods 9/)).toBeTruthy();
  });

  it('renders "Installing file N/M: name" for the installing-file phase', () => {
    const { getByText } = render(ImportProgressView, {
      props: {
        phase: { phase: 'installing_file', current: 3, total: 12, file_name: 'Sodium' },
        modBytes: null,
      },
    });
    expect(getByText(/Installing file 3\/12: Sodium/)).toBeTruthy();
  });

  it('renders the extracting-overrides phase counter', () => {
    const { getByText } = render(ImportProgressView, {
      props: {
        phase: { phase: 'extracting_overrides', current: 5, total: 20 },
        modBytes: null,
      },
    });
    expect(getByText(/Extracting overrides 5\/20/)).toBeTruthy();
  });

  it('renders the bytes progress bar when modBytes has a total', () => {
    const { container } = render(ImportProgressView, {
      props: {
        phase: { phase: 'installing_file', current: 1, total: 3, file_name: 'Iris' },
        modBytes: { phase: 'downloading', current: 50, total: 100 },
      },
    });
    const bar = container.querySelector('.bg-blue-500') as HTMLElement | null;
    expect(bar).not.toBeNull();
    // 50/100 == 50% width style.
    expect(bar?.getAttribute('style')).toContain('50%');
  });

  it('hides the bytes bar when total is null (verify/copy phases)', () => {
    const { container } = render(ImportProgressView, {
      props: {
        phase: { phase: 'installing_file', current: 1, total: 3, file_name: 'Iris' },
        modBytes: { phase: 'verifying', current: null, total: null },
      },
    });
    expect(container.querySelector('.bg-blue-500')).toBeNull();
  });

  it('renders the done label', () => {
    const { getByText } = render(ImportProgressView, {
      props: {
        phase: { phase: 'done', instance_id: 'inst-123' },
        modBytes: null,
      },
    });
    expect(getByText('Done.')).toBeTruthy();
  });

  it('renders as a non-blocking corner toast', () => {
    const { container } = render(ImportProgressView, {
      props: { phase: { phase: 'inspecting' }, modBytes: null },
    });
    const root = container.querySelector('[data-testid="import-progress-view"]') as HTMLElement;
    expect(root).not.toBeNull();
    // Pinned to the top-right corner, not a full-screen overlay.
    expect(root.classList.contains('fixed')).toBe(true);
    expect(root.classList.contains('top-4')).toBe(true);
    expect(root.classList.contains('right-4')).toBe(true);
    expect(root.classList.contains('inset-0')).toBe(false);
  });
});
