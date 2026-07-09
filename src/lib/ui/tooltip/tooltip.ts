// src/lib/ui/tooltip/tooltip.ts
// Svelte action: `use:tooltip={param}`. Attaches hover + keyboard-focus events
// to a trigger and drives the shared tooltip controller. Mirrors the existing
// action shape (trap-focus.ts / RenderedBody interceptLinks): returns
// { update, destroy }.
//
// Usage:
//   <button aria-label={label} use:tooltip={label}><Icon .../></button>
//   <span use:tooltip={{ text: reason, describe: false }}><button disabled>…</button></span>
//   <span class="truncate" use:tooltip={{ text: name, whenOverflowing: true }}>{name}</span>
//
// Only valid on DOM elements — wrap Svelte components (e.g. BusyButton) in a span.
import type { Placement } from './position';
import { hideTooltip, showTooltip, TOOLTIP_ID } from './tooltip-controller.svelte';

export type TooltipParam =
  | string
  | {
      text: string;
      placement?: Placement;
      whenOverflowing?: boolean;
      describe?: boolean;
    }
  | null
  | undefined;

interface Normalized {
  text: string;
  placement: Placement;
  whenOverflowing: boolean;
  describe: boolean | undefined;
}

function normalize(param: TooltipParam): Normalized | null {
  if (param == null) return null;
  if (typeof param === 'string') {
    return param.trim()
      ? { text: param, placement: 'top', whenOverflowing: false, describe: undefined }
      : null;
  }
  return param.text && param.text.trim()
    ? {
        text: param.text,
        placement: param.placement ?? 'top',
        whenOverflowing: param.whenOverflowing ?? false,
        describe: param.describe,
      }
    : null;
}

export function tooltip(node: HTMLElement, param: TooltipParam) {
  let opts = normalize(param);

  const isClipped = () => node.scrollWidth > node.clientWidth;
  const shouldShow = () => !!opts && (!opts.whenOverflowing || isClipped());
  // Focus surfaces the tooltip only for genuine keyboard focus. Programmatic
  // focus — a modal's focus trap landing on its close button when it opens, or
  // focus restored to the trigger button when the modal closes — is not
  // :focus-visible in Chromium, so it no longer pops a spurious tooltip the
  // instant a dialog opens or closes. Real Tab navigation still matches
  // :focus-visible, so the keyboard a11y hint is preserved. Hover is unaffected
  // (it never consults this). Falls back to showing if the engine lacks
  // :focus-visible support, preserving the prior behaviour rather than
  // regressing the hint.
  const isFocusVisible = () => {
    try {
      return node.matches(':focus-visible');
    } catch {
      return true;
    }
  };
  const shouldDescribe = () => {
    if (!opts) return false;
    if (opts.describe === false) return false;
    if (opts.describe === true) return true;
    // Auto: if the node already exposes an accessible name via aria-label, the
    // tooltip text is redundant for a screen reader — skip aria-describedby.
    return !node.hasAttribute('aria-label');
  };

  function open(immediate: boolean) {
    if (!opts || !shouldShow()) return;
    showTooltip(node.getBoundingClientRect(), opts.text, {
      placement: opts.placement,
      immediate,
      owner: node,
    });
    if (shouldDescribe()) node.setAttribute('aria-describedby', TOOLTIP_ID);
  }

  function close() {
    hideTooltip(node);
    node.removeAttribute('aria-describedby');
  }

  const onEnter = () => open(false);
  const onLeave = () => close();
  const onFocus = () => {
    if (!isFocusVisible()) return;
    open(true);
  };
  const onBlur = () => close();

  node.addEventListener('mouseenter', onEnter);
  node.addEventListener('mouseleave', onLeave);
  node.addEventListener('focusin', onFocus);
  node.addEventListener('focusout', onBlur);

  return {
    update(next: TooltipParam) {
      opts = normalize(next);
      if (!opts) close();
    },
    destroy() {
      node.removeEventListener('mouseenter', onEnter);
      node.removeEventListener('mouseleave', onLeave);
      node.removeEventListener('focusin', onFocus);
      node.removeEventListener('focusout', onBlur);
      close();
    },
  };
}
