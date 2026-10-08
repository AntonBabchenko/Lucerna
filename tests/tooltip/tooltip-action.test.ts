// tests/tooltip/tooltip-action.test.ts
import { afterEach, describe, expect, it, vi } from 'vitest';
import { tooltip } from '$lib/ui/tooltip/tooltip';
import { hideTooltip, TOOLTIP_ID, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';

// The tooltip only shows on focus when the focus is keyboard-driven
// (`:focus-visible`). The action calls `node.matches(':focus-visible')` to
// decide; happy-dom cannot model real focus modality, so stub it. Default
// `true` = keyboard focus (the a11y path the focus tests exercise); pass
// `focusVisible: false` to model programmatic focus (a modal's focus trap or a
// focus-restore on close), which must NOT surface a tooltip.
function mount(param: Parameters<typeof tooltip>[1], { focusVisible = true } = {}) {
  const node = document.createElement('button');
  vi.spyOn(node, 'matches').mockReturnValue(focusVisible);
  document.body.appendChild(node);
  const handle = tooltip(node, param);
  return { node, handle };
}

afterEach(() => {
  hideTooltip();
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('use:tooltip action', () => {
  it('shows immediately on keyboard focus and sets aria-describedby when no aria-label', () => {
    const { node } = mount('Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Grid view');
    expect(node.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
  });

  it('stays hidden on programmatic (non-:focus-visible) focus', () => {
    // A modal's focus trap focusing its close button on open, or focus being
    // restored to the trigger on close, is not :focus-visible — no tooltip.
    const { node } = mount('Grid view', { focusVisible: false });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
    expect(node.hasAttribute('aria-describedby')).toBe(false);
  });

  // A disabled-capable icon button carries its tooltip on a wrapping span (DESIGN.md §5), and
  // keyboard focus lands on the button inside it, never on the wrapper. The wrapper has to
  // accept a focus-visible DESCENDANT, or a keyboard user never learns what the icon does.
  it('a wrapper shows its tooltip while a control inside it has keyboard focus', () => {
    const wrapper = document.createElement('span');
    const inner = document.createElement('button');
    wrapper.appendChild(inner);
    document.body.appendChild(wrapper);
    vi.spyOn(wrapper, 'matches').mockImplementation((sel) => sel === ':has(:focus-visible)');
    tooltip(wrapper, { text: 'Load PNG', describe: false });
    inner.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Load PNG');
  });

  it('a wrapper stays hidden when the control inside got programmatic focus', () => {
    const wrapper = document.createElement('span');
    const inner = document.createElement('button');
    wrapper.appendChild(inner);
    document.body.appendChild(wrapper);
    vi.spyOn(wrapper, 'matches').mockReturnValue(false);
    tooltip(wrapper, { text: 'Load PNG', describe: false });
    inner.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(tooltipState.visible).toBe(false);
  });

  it('skips aria-describedby when the node already has an aria-label', () => {
    const { node } = mount('Grid view');
    node.setAttribute('aria-label', 'Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    expect(node.hasAttribute('aria-describedby')).toBe(false);
  });

  // A blur hides one microtask later, past the Svelte batch it may fire in (a focused trigger
  // removed by a block — tests/tooltip/tooltip-teardown.test.ts).
  it('hides and clears aria-describedby on blur', async () => {
    const { node } = mount('Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    node.dispatchEvent(new FocusEvent('focusout'));
    await Promise.resolve();
    expect(tooltipState.visible).toBe(false);
    expect(node.hasAttribute('aria-describedby')).toBe(false);
  });

  // Focus leaving and coming back — or moving between two controls inside a wrapper trigger —
  // must not lose the tooltip to the blur's late hide.
  it('keeps a tooltip shown again before the blur’s hide lands', async () => {
    const { node } = mount('Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    node.dispatchEvent(new FocusEvent('focusout'));
    node.dispatchEvent(new FocusEvent('focusin'));
    await Promise.resolve();
    expect(tooltipState.visible).toBe(true);
    expect(node.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
  });

  it('does nothing for a null / empty param', () => {
    const { node } = mount(null);
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });

  it('whenOverflowing suppresses the tooltip while the text fits', () => {
    const { node } = mount({ text: 'Full name', whenOverflowing: true });
    // happy-dom reports 0/0; stub the clip check: not overflowing.
    Object.defineProperty(node, 'scrollWidth', { value: 50, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });

  it('whenOverflowing shows the tooltip when the text is clipped', () => {
    const { node } = mount({ text: 'Full name', whenOverflowing: true });
    Object.defineProperty(node, 'scrollWidth', { value: 200, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
  });

  // Plan §5c V3: a version cut short («v0.10…») keeps its file name as its tooltip, and the
  // tooltip must then carry the version whole too — the text the node no longer shows.
  it('clippedText stands in for the text while the node is clipped', () => {
    const { node } = mount({ text: 'a.jar', clippedText: 'v0.102.0 · a.jar' });
    Object.defineProperty(node, 'scrollWidth', { value: 50, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.text).toBe('a.jar');
    node.dispatchEvent(new FocusEvent('focusout'));
    Object.defineProperty(node, 'scrollWidth', { value: 200, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.text).toBe('v0.102.0 · a.jar');
  });

  // Plan §5d M1: a version with no room left wraps out of its row, and the name beside it then
  // carries it. The node is not clipped itself; what its neighbour gave up is its to say.
  it('alsoClipped counts the node as clipped while a neighbour it speaks for is gone', () => {
    let versionGone = false;
    const { node } = mount({
      text: 'Alpha',
      whenOverflowing: true,
      clippedText: 'Alpha · v1.0',
      alsoClipped: () => versionGone,
    });
    Object.defineProperty(node, 'scrollWidth', { value: 50, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
    node.dispatchEvent(new FocusEvent('focusout'));
    versionGone = true;
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Alpha · v1.0');
  });

  it('update(null) hides an open tooltip', () => {
    const { node, handle } = mount('Hi');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    handle?.update?.(null);
    expect(tooltipState.visible).toBe(false);
  });

  it('destroy removes listeners and hides', () => {
    const { node, handle } = mount('Hi');
    handle?.destroy?.();
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });
});
