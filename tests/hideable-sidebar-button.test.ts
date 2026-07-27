import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import HideableSidebarButton from '$lib/layout/HideableSidebarButton.svelte';
import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';
import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

const label = (text: string) => createRawSnippet(() => ({ render: () => `<span>${text}</span>` }));

const hideItems: ContextMenuItem[] = [
  { label: 'Hide', icon: 'eyeOff', testId: 'ctx-hide', onSelect: () => {} },
];

const base = {
  hideItems,
  contextMenuAria: 'Button options',
  children: label('Gallery'),
};

describe('HideableSidebarButton', () => {
  beforeEach(() => initSidebarButtons([]));

  it('renders the secondary-button shell with class recipe, testid and children when visible', () => {
    render(HideableSidebarButton, {
      props: { ...base, id: 'gallery', testid: 'sidebar-open-gallery', onclick: () => {} },
    });
    const btn = screen.getByTestId('sidebar-open-gallery');
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.className).toContain('btn-secondary');
    expect(btn.className).toContain('btn-xs');
    expect(btn.className).toContain('w-full');
    expect(btn.textContent).toContain('Gallery');
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });

  it('renders nothing when the button id is hidden', () => {
    initSidebarButtons(['gallery']);
    render(HideableSidebarButton, {
      props: { ...base, id: 'gallery', testid: 'sidebar-open-gallery', onclick: () => {} },
    });
    expect(screen.queryByTestId('sidebar-open-gallery')).toBeNull();
  });

  it('forwards data-tour onto the button', () => {
    render(HideableSidebarButton, {
      props: {
        ...base,
        id: 'browse_modpacks',
        testid: 'sidebar-open-modpacks',
        dataTour: 'open-modpacks',
        onclick: () => {},
      },
    });
    expect(screen.getByTestId('sidebar-open-modpacks').getAttribute('data-tour')).toBe(
      'open-modpacks',
    );
  });

  it('disabledTooltip: renders a disabled button wrapped in an inline-flex tooltip span', () => {
    render(HideableSidebarButton, {
      props: {
        ...base,
        id: 'import_launcher',
        testid: 'sidebar-open-launcher-import',
        disabledTooltip: 'Data location unavailable',
        onclick: () => {},
      },
    });
    const btn = screen.getByTestId('sidebar-open-launcher-import') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    const span = btn.parentElement;
    expect(span?.tagName).toBe('SPAN');
    expect(span?.className).toContain('inline-flex');
    expect(span?.className).toContain('w-full');
  });

  it('no disabledTooltip: renders an enabled button not wrapped in a span', () => {
    render(HideableSidebarButton, {
      props: {
        ...base,
        id: 'import_launcher',
        testid: 'sidebar-open-launcher-import',
        onclick: () => {},
      },
    });
    const btn = screen.getByTestId('sidebar-open-launcher-import') as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    // The immediate parent is the ContextMenu's display:contents div, not a tooltip span.
    expect(btn.parentElement?.tagName).not.toBe('SPAN');
  });
});
