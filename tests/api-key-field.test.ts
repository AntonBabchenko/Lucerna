// The shared API-key field: what it owns regardless of which form drives it.
// DESIGN.md "API-key fields" — order, tones, one live region per element,
// Enter-to-save, `disabled` gating every control, the optional disclosure.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ApiKeyField from '$lib/settings/ApiKeyField.svelte';
import type { FieldStatus } from '$lib/settings/api-key-field';

function base(over: Record<string, unknown> = {}) {
  return {
    statusLabel: 'API key:',
    status: { text: 'Not stored', tone: 'secondary' } as FieldStatus,
    inputLabel: 'API key',
    placeholder: 'Paste your key',
    saveLabel: 'Save key',
    onSave: vi.fn(),
    note: 'Stored in your keyring.',
    testIdPrefix: 'ai-key' as const,
    ...over,
  };
}

describe('ApiKeyField', () => {
  it('Enter in the field saves, but not while the field is empty', async () => {
    const onSave = vi.fn();
    render(ApiKeyField, { props: base({ onSave }) });
    const input = screen.getByTestId('ai-key-input') as HTMLInputElement;
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSave).not.toHaveBeenCalled();
    await fireEvent.input(input, { target: { value: 'k' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('disabled gates the input, Save and Clear alike', () => {
    render(ApiKeyField, {
      props: base({ disabled: true, value: 'k', clearLabel: 'Clear key', onClear: vi.fn() }),
    });
    expect((screen.getByTestId('ai-key-input') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId('ai-key-save') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('ai-key-clear') as HTMLButtonElement).disabled).toBe(true);
  });

  it('a busy status is exactly one live region whose text is the status text', () => {
    render(ApiKeyField, {
      props: base({ status: { text: 'Checking…', tone: 'placeholder', busy: true } }),
    });
    const status = screen.getByTestId('ai-key-status');
    // `textContent` must be the label alone: a second sr-only "Loading…" would
    // both break this and announce the state twice (INT-15).
    expect(status.textContent?.trim()).toBe('Checking…');
    const inside = status.querySelectorAll('[role="status"]').length;
    const self = status.getAttribute('role') === 'status' ? 1 : 0;
    expect(inside + self).toBe(1);
    expect(screen.queryByText(/Loading…/)).toBeNull();
    expect(status.className).toContain('text-placeholder');
  });

  it('tones map to the design-system text families', () => {
    render(ApiKeyField, { props: base({ status: { text: 'Stored', tone: 'success' } }) });
    const status = screen.getByTestId('ai-key-status');
    expect(status.className).toContain('text-success');
    expect(status.getAttribute('role')).toBe('status');
  });

  it('a status action renders next to the status and runs on click', async () => {
    const onClick = vi.fn();
    render(ApiKeyField, { props: base({ statusAction: { label: 'Check again', onClick } }) });
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('a status detail is a second line under the status', () => {
    render(ApiKeyField, { props: base({ statusDetail: 'Platform secure storage failure' }) });
    expect(screen.getByTestId('ai-key-status-reason').textContent).toContain('secure storage');
  });

  it('collapsed wraps the field and Save in a closed disclosure and leaves the status outside', () => {
    render(ApiKeyField, {
      props: base({
        collapsed: { summary: 'Use my own key', testId: 'cf-key-own-key' },
        testIdPrefix: 'cf-key',
      }),
    });
    const details = screen.getByTestId('cf-key-own-key') as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain('Use my own key');
    expect(details.contains(screen.getByTestId('cf-key-input'))).toBe(true);
    expect(details.contains(screen.getByTestId('cf-key-save'))).toBe(true);
    expect(details.contains(screen.getByTestId('cf-key-status'))).toBe(false);
  });

  it('a failed result is an alert; a saved note is a status', () => {
    const { unmount } = render(ApiKeyField, {
      props: base({ result: { tone: 'danger', text: 'Rejected' }, resultTestId: 'ai-key-error' }),
    });
    const box = screen.getByTestId('ai-key-error');
    expect(box.querySelector('[role="alert"]')?.textContent).toContain('Rejected');
    unmount();
    render(ApiKeyField, {
      props: base({ result: { tone: 'info', text: 'Saved' }, resultTestId: 'ai-key-error' }),
    });
    expect(
      screen.getByTestId('ai-key-error').querySelector('[role="status"]')?.textContent,
    ).toContain('Saved');
  });

  it('the input never autocompletes or spell-checks a secret', () => {
    render(ApiKeyField, { props: base() });
    const input = screen.getByTestId('ai-key-input') as HTMLInputElement;
    expect(input.getAttribute('autocomplete')).toBe('off');
    expect(input.getAttribute('spellcheck')).toBe('false');
    expect(input.getAttribute('type')).toBe('password');
  });
});
