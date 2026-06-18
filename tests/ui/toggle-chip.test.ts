import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ToggleChip from '$lib/ui/ToggleChip.svelte';
import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';

describe('ToggleChip', () => {
  it('renders the label and reflects active via aria-pressed', () => {
    render(ToggleChip, {
      props: { active: true, tone: 'success', label: 'Enabled', onToggle: vi.fn() },
    });
    const btn = screen.getByRole('button', { name: /Enabled/ });
    expect(btn.getAttribute('aria-pressed')).toBe('true');
  });

  it('applies a tone class when active and a neutral class when inactive', () => {
    const { rerender } = render(ToggleChip, {
      props: { active: true, tone: 'danger', label: 'Issues', onToggle: vi.fn() },
    });
    expect(screen.getByRole('button').className).toContain('border-danger');
    rerender({ active: false, tone: 'danger', label: 'Issues', onToggle: vi.fn() });
    expect(screen.getByRole('button').className).not.toContain('border-danger');
  });

  it('renders a count when given and forwards testId', () => {
    render(ToggleChip, {
      props: {
        active: false,
        tone: 'neutral',
        label: 'All',
        count: 7,
        onToggle: vi.fn(),
        testId: 'chip-all',
      },
    });
    const btn = screen.getByTestId('chip-all');
    expect(btn.textContent).toContain('All');
    expect(btn.textContent).toContain('7');
  });

  it('calls onToggle on click', async () => {
    const onToggle = vi.fn();
    render(ToggleChip, { props: { active: false, tone: 'neutral', label: 'All', onToggle } });
    await fireEvent.click(screen.getByRole('button'));
    expect(onToggle).toHaveBeenCalledOnce();
  });
});

const GROUP = [
  { value: 'all', label: 'All', tone: 'neutral' as const, testId: 'flt-all' },
  { value: 'enabled', label: 'Enabled', tone: 'success' as const, testId: 'flt-enabled' },
  { value: 'issues', label: 'Issues', tone: 'danger' as const, testId: 'flt-issues' },
];

function setupGroup(props: Record<string, unknown> = {}) {
  const onChange = vi.fn();
  render(ToggleChipGroup, {
    props: { options: GROUP, value: 'all', onChange, ariaLabel: 'Filter', ...props },
  });
  return { onChange };
}

describe('ToggleChipGroup', () => {
  it('renders a radiogroup with one radio per option', () => {
    setupGroup();
    expect(screen.getByRole('radiogroup', { name: 'Filter' })).toBeTruthy();
    expect(screen.getAllByRole('radio')).toHaveLength(3);
  });

  it('marks the selected option with aria-checked', () => {
    setupGroup({ value: 'enabled' });
    expect(screen.getByTestId('flt-all').getAttribute('aria-checked')).toBe('false');
    expect(screen.getByTestId('flt-enabled').getAttribute('aria-checked')).toBe('true');
  });

  it('single-selects: clicking emits the clicked value', async () => {
    const { onChange } = setupGroup();
    await fireEvent.click(screen.getByTestId('flt-issues'));
    expect(onChange).toHaveBeenCalledWith('issues');
  });

  it('arrow keys move selection with wrap', async () => {
    const { onChange } = setupGroup({ value: 'all' });
    const group = screen.getByRole('radiogroup', { name: 'Filter' });
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('enabled');
    onChange.mockClear();
    await fireEvent.keyDown(group, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('issues'); // wraps from all back to issues
  });
});
