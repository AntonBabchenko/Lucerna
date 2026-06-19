import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
import Layout from '../src/routes/+layout.svelte';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return { ...actual };
});

beforeEach(() => {
  document.documentElement.classList.remove('fx-icon-zoom');
});

afterEach(() => {
  iconZoomFx.set(true); // restore default-on for other suites
  document.documentElement.classList.remove('fx-icon-zoom');
});

describe('+layout icon-zoom root class', () => {
  it('adds fx-icon-zoom to <html> when the preference is enabled', () => {
    iconZoomFx.set(true);
    render(Layout, { props: { children: undefined } });
    expect(document.documentElement.classList.contains('fx-icon-zoom')).toBe(true);
  });

  it('removes fx-icon-zoom from <html> when the preference is disabled', () => {
    iconZoomFx.set(false);
    render(Layout, { props: { children: undefined } });
    expect(document.documentElement.classList.contains('fx-icon-zoom')).toBe(false);
  });
});
