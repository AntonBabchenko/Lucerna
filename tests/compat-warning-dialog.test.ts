import { render, screen } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import CompatWarningDialog from '../src/lib/mods/CompatWarningDialog.svelte';

describe('CompatWarningDialog', () => {
  test('lists each mismatched jar with its reason', () => {
    render(CompatWarningDialog, {
      rows: [{ filename: 'sodium.jar', reason: 'looks like a Fabric mod, instance is forge' }],
      onConfirm: () => {},
      onCancel: () => {},
    });
    expect(screen.getByText('sodium.jar')).toBeTruthy();
    expect(screen.getByText(/looks like a Fabric mod/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /install anyway/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /skip these/i })).toBeTruthy();
  });
});
