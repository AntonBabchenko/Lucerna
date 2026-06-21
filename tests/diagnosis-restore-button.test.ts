import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import DiagnosisRestoreButton from '$lib/ui/DiagnosisRestoreButton.svelte';

describe('DiagnosisRestoreButton', () => {
  beforeAll(() => {
    locale.set('en');
  });

  it('renders a labelled button and fires onRestore on click', async () => {
    const onRestore = vi.fn();
    render(DiagnosisRestoreButton, { props: { onRestore, testid: 'restore-x' } });
    const btn = screen.getByTestId('restore-x');
    expect(btn.getAttribute('aria-label')).toBe('Show warning');
    await fireEvent.click(btn);
    expect(onRestore).toHaveBeenCalledOnce();
  });
});
