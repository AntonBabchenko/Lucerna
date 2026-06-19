import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
import AppearancePanel from '$lib/settings/AppearancePanel.svelte';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return { ...actual };
});

beforeEach(() => {
  iconZoomFx.set(true);
});

afterEach(() => {
  iconZoomFx.set(true); // restore default-on for other suites
});

describe('AppearancePanel icon-zoom toggle', () => {
  it('renders checked when iconZoomFx is enabled', () => {
    const { getByTestId } = render(AppearancePanel);
    const box = getByTestId('icon-zoom-toggle') as HTMLInputElement;
    expect(box.checked).toBe(true);
  });

  it('unchecking the box disables iconZoomFx', async () => {
    const { getByTestId } = render(AppearancePanel);
    const box = getByTestId('icon-zoom-toggle') as HTMLInputElement;
    await fireEvent.click(box);
    expect(iconZoomFx.enabled).toBe(false);
  });
});
