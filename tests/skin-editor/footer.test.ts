import { fireEvent, render, screen, within } from '@testing-library/svelte';
import type { ComponentProps } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SkinEditorFooter from '$lib/accounts/SkinEditorFooter.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import SkinEditorFooterBound from '../fixtures/SkinEditorFooterBound.svelte';

// The tooltip state is module-level: a case that fails before hiding it must not leak a
// visible tooltip into the next one.
afterEach(() => hideTooltip());

type Props = ComponentProps<typeof SkinEditorFooter>;

function setup(over: Partial<Props> = {}) {
  const props: Props = {
    colour: [224, 224, 224, 255],
    brush: 1,
    activeLayer: 'base',
    bg: 'neutral',
    pose: 'default',
    onPose: vi.fn(),
    variant: 'classic',
    onVariant: vi.fn(),
    baseVisible: true,
    overlayVisible: false,
    onToggleBase: vi.fn(),
    onToggleOverlay: vi.fn(),
    busy: false,
    isMicrosoft: true,
    saveError: null,
    applied: false,
    onLoadPng: vi.fn(),
    onExportPng: vi.fn(),
    onSaveToLibrary: vi.fn(),
    onOpenLibrary: vi.fn(),
    onApply: vi.fn(),
    ...over,
  };
  render(SkinEditorFooter, { props });
  return props;
}

const group = (name: string) => screen.getByRole('group', { name });
const pressed = (g: HTMLElement) =>
  within(g)
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true');
const nameOf = (b: HTMLElement) => b.getAttribute('aria-label') ?? b.textContent?.trim();

describe('SkinEditorFooter — single choices are segmented controls (DESIGN.md §6)', () => {
  it.each([
    ['Brush', 'Thin'],
    ['Paint on', 'Base'],
    ['Pose', 'Default'],
    ['Model', 'Classic'],
    ['Background', 'Neutral'],
  ])('%s is a named group whose one chosen option (%s) carries the accent mark', (name, chosen) => {
    setup();
    const on = pressed(group(name));
    expect(on).toHaveLength(1);
    expect(nameOf(on[0])).toBe(chosen);
    expect(on[0].querySelector('[data-segment-mark]')).not.toBeNull();
  });

  // The old footer filled a chosen option with the accent wash — the look of a
  // button to press, which DESIGN.md §6 rules out for a state.
  it('no control is filled with the accent wash', () => {
    setup();
    for (const b of screen.getAllByRole('button')) {
      expect(b.className).not.toMatch(/bg-accent-soft/);
    }
  });

  it('every model option says whether it is chosen', () => {
    setup({ variant: 'slim' });
    const opts = within(group('Model')).getAllByRole('button');
    expect(opts.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'true']);
  });

  it('choosing a pose or a model calls back with the value', async () => {
    const p = setup();
    await fireEvent.click(within(group('Pose')).getByRole('button', { name: 'Walk' }));
    expect(p.onPose).toHaveBeenCalledWith('walk');
    await fireEvent.click(within(group('Model')).getByRole('button', { name: 'Slim' }));
    expect(p.onVariant).toHaveBeenCalledWith('slim');
  });

  it('shows the choices it is given', () => {
    setup({ pose: 'walk', variant: 'slim', brush: 5, activeLayer: 'overlay', bg: 'nether' });
    expect(nameOf(pressed(group('Pose'))[0])).toBe('Walk');
    expect(nameOf(pressed(group('Model'))[0])).toBe('Slim');
    expect(nameOf(pressed(group('Brush'))[0])).toBe('Thick');
    expect(nameOf(pressed(group('Paint on'))[0])).toBe('Overlay');
    expect(nameOf(pressed(group('Background'))[0])).toBe('Nether');
  });

  // Brush and background options are pictures; their names are translated
  // words, not «Brush 3» or raw ids.
  it('pictured options are named in the interface language', () => {
    setup();
    const names = (g: string) =>
      within(group(g))
        .getAllByRole('button')
        .map((b) => b.getAttribute('aria-label'));
    expect(names('Brush')).toEqual(['Thin', 'Medium', 'Thick']);
    expect(names('Background')).toEqual(['Neutral', 'Plains', 'Nether', 'Stronghold']);
  });
});

// The modal binds these four, and the eyedropper writes `colour` from the modal's side: the
// move out of SkinEditorModal must keep both directions of each binding.
describe('SkinEditorFooter — bound choices reach the editor and back', () => {
  const bound = () => screen.getByTestId('bound').textContent;

  it('a palette swatch, the custom colour and the segmented choices write through', async () => {
    render(SkinEditorFooterBound);
    const swatches = screen.getAllByRole('button', { name: /^#[0-9a-f]{6}$/ });
    await fireEvent.click(swatches[1]);
    expect(bound()).toBe('#3c3c3c|1|base|neutral');
    expect(swatches[1].className).toContain('outline-accent');
    expect(swatches[0].className).not.toContain('outline-accent');
    await fireEvent.input(screen.getByLabelText('Custom colour'), {
      target: { value: '#00ff00' },
    });
    expect(bound()).toBe('#00ff00|1|base|neutral');
    await fireEvent.click(within(group('Brush')).getByRole('button', { name: 'Thick' }));
    await fireEvent.click(within(group('Paint on')).getByRole('button', { name: 'Overlay' }));
    await fireEvent.click(within(group('Background')).getByRole('button', { name: 'Stronghold' }));
    expect(bound()).toBe('#00ff00|5|overlay|stronghold');
  });

  it('a colour the editor picks (the eyedropper) shows in the footer', async () => {
    render(SkinEditorFooterBound);
    await fireEvent.click(screen.getByTestId('pick-red'));
    expect((screen.getByLabelText('Custom colour') as HTMLInputElement).value).toBe('#ff0000');
  });
});

describe('SkinEditorFooter — layer visibility is a pair of toggle chips', () => {
  it('each chip says whether its layer is shown and toggles it', async () => {
    const p = setup({ baseVisible: true, overlayVisible: false });
    const g = group('Layer visibility');
    const base = within(g).getByRole('button', { name: 'Base' });
    const overlay = within(g).getByRole('button', { name: 'Overlay' });
    expect(base.getAttribute('aria-pressed')).toBe('true');
    expect(overlay.getAttribute('aria-pressed')).toBe('false');
    expect(base.className).toMatch(/rounded-full/);
    await fireEvent.click(base);
    await fireEvent.click(overlay);
    expect(p.onToggleBase).toHaveBeenCalledOnce();
    expect(p.onToggleOverlay).toHaveBeenCalledOnce();
  });
});

describe('SkinEditorFooter — file and library actions are icon buttons (DESIGN.md §5)', () => {
  const ACTIONS = [
    ['Load PNG', 'onLoadPng'],
    ['Download PNG', 'onExportPng'],
    ['Save to library', 'onSaveToLibrary'],
    ['Library…', 'onOpenLibrary'],
  ] as const;

  it.each(ACTIONS)('%s has no visible text, a name, a tooltip and its action', async (name, cb) => {
    const p = setup();
    const btn = screen.getByRole('button', { name });
    expect(btn.classList.contains('btn-icon')).toBe(true);
    expect(btn.textContent?.trim()).toBe('');
    // Disabled while busy, so the tooltip sits on a wrapper (DESIGN.md §5). Keyboard focus
    // lands on the button, as in a browser, and bubbles to the wrapper, which then holds a
    // focus-visible descendant (happy-dom cannot model focus modality: stubbed).
    const wrapper = btn.parentElement as HTMLElement;
    wrapper.matches = (sel: string) => sel === ':has(:focus-visible)';
    btn.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe(name);
    await fireEvent.click(btn);
    expect(p[cb]).toHaveBeenCalledOnce();
  });

  it('the actions are disabled while busy', () => {
    setup({ busy: true });
    for (const [name] of ACTIONS) {
      expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('Apply to account applies for a Microsoft account', async () => {
    const p = setup({ isMicrosoft: true });
    await fireEvent.click(screen.getByRole('button', { name: 'Apply to account' }));
    expect(p.onApply).toHaveBeenCalledOnce();
  });

  it('Apply to account stays the labelled primary action, offline-disabled', () => {
    setup({ isMicrosoft: false });
    const apply = screen.getByRole('button', { name: 'Apply to account' });
    expect(apply).toHaveBtnVariant('primary');
    expect((apply as HTMLButtonElement).disabled).toBe(true);
  });
});
