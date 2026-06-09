import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import AttentionPanel from '$lib/overview/AttentionPanel.svelte';
import type { AttentionItem } from '$lib/overview/attention';

describe('AttentionPanel', () => {
  it('renders nothing when there are no items', () => {
    const { queryByTestId } = render(AttentionPanel, {
      props: { items: [] as AttentionItem[], onAction: () => {} },
    });
    expect(queryByTestId('overview-attention')).toBeNull();
  });

  it('renders one row per item', () => {
    const items: AttentionItem[] = [
      { kind: 'missing_mods', count: 3 },
      { kind: 'integrity', count: 2 },
    ];
    const { getByTestId, queryByTestId } = render(AttentionPanel, {
      props: { items, onAction: () => {} },
    });
    expect(getByTestId('overview-attention-missing_mods')).toBeTruthy();
    expect(getByTestId('overview-attention-integrity')).toBeTruthy();
    expect(queryByTestId('overview-attention-incompatible')).toBeNull();
    expect(getByTestId('overview-attention-missing_mods').textContent).toContain('3');
    expect(getByTestId('overview-attention-integrity').textContent).toContain('2');
  });

  it('fires onAction with the row kind when clicked', async () => {
    const onAction = vi.fn();
    const { getByTestId } = render(AttentionPanel, {
      props: { items: [{ kind: 'incompatible', count: 1 }], onAction },
    });
    await fireEvent.click(getByTestId('overview-attention-incompatible'));
    expect(onAction).toHaveBeenCalledWith('incompatible');
  });
});
