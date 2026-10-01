import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { modsChangelog: vi.fn(() => new Promise(() => {})) },
}));

import UpdateReviewDialog from '$lib/mods/installed/UpdateReviewDialog.svelte';

const items = [
  {
    sha1: 'a',
    name: 'Sodium',
    from: '0.5.0',
    to: '0.6.0',
    changelog: {
      source: 'modrinth' as const,
      projectId: 'sodium',
      targetVersionId: 'v6',
      baseVersionId: 'v5',
    },
  },
  { sha1: 'b', name: 'Jade', from: '15.5', to: '15.6', changelog: null },
];
beforeAll(() => locale.set('en'));

describe('UpdateReviewDialog', () => {
  it('ticks every pending update and confirms only the ticked ones', async () => {
    const onConfirm = vi.fn();
    render(UpdateReviewDialog, { props: { items, onCancel: vi.fn(), onConfirm } });
    const sodium = screen.getByRole('checkbox', { name: 'Sodium' }) as HTMLInputElement;
    expect(sodium.checked).toBe(true);
    expect((screen.getByRole('checkbox', { name: 'Jade' }) as HTMLInputElement).checked).toBe(true);
    await fireEvent.click(sodium);
    await fireEvent.click(screen.getByRole('button', { name: 'Update 1' }));
    expect(onConfirm).toHaveBeenCalledWith(['b']);
  });

  it('shows old → new and turns the confirm off while nothing is ticked', async () => {
    render(UpdateReviewDialog, { props: { items, onCancel: vi.fn(), onConfirm: vi.fn() } });
    expect(screen.getByTestId('update-review-list').textContent).toMatch(/v0\.5\.0\s*v0\.6\.0/);
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Sodium' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Jade' }));
    expect((screen.getByTestId('update-review-confirm') as HTMLButtonElement).disabled).toBe(true);
  });

  // Plan §5b V1: the «v» goes only before a version that starts with a digit — never «vmc1.21.1».
  it('glues no «v» to a version that starts with a letter', () => {
    const lithium = {
      sha1: 'c',
      name: 'Lithium',
      from: 'mc1.21.1-0.13.1',
      to: 'mc1.21.1-0.13.2',
      changelog: null,
    };
    render(UpdateReviewDialog, {
      props: { items: [lithium], onCancel: vi.fn(), onConfirm: vi.fn() },
    });
    const text = screen.getByTestId('update-review-list').textContent ?? '';
    expect(text).toMatch(/mc1\.21\.1-0\.13\.1\s*mc1\.21\.1-0\.13\.2/);
    expect(text).not.toContain('vmc');
  });

  it('holds the list still while the updates run', () => {
    render(UpdateReviewDialog, {
      props: { items, busy: true, onCancel: vi.fn(), onConfirm: vi.fn() },
    });
    for (const box of screen.getAllByRole('checkbox')) {
      expect((box as HTMLInputElement).disabled).toBe(true);
    }
    expect((screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("opens a mod's changelog on top, only where one is published", async () => {
    render(UpdateReviewDialog, { props: { items, onCancel: vi.fn(), onConfirm: vi.fn() } });
    expect(screen.getAllByRole('button', { name: /^Changelog:/ })).toHaveLength(1);
    await fireEvent.click(screen.getByRole('button', { name: 'Changelog: Sodium' }));
    expect(await screen.findAllByRole('dialog')).toHaveLength(2);
  });
});
