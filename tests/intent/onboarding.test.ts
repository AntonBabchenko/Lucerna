// Onboarding group intent coverage.
//
// tests/instance-concept-tooltip.test.ts covers open/close behaviour and
// text content. tests/tour-overlay.test.ts covers TourOverlay navigation,
// keyboard handling, and aria attributes. This file adds CLASS/VARIANT
// assertions that the behaviour tests intentionally omit.
//
// Regression lock for fix 9ce0f0a (sidebar tooltip clip): the popover uses
// position:fixed and carries normal-case + tracking-normal to override any
// upstream uppercase/spaced lettering inherited from section headers.

import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// ── mocks required by state.svelte.ts / TourOverlay ───────────────────────
const appSettingsGet = vi.fn();
const appSettingsMarkTourCompleted = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...args: unknown[]) => appSettingsGet(...args),
    appSettingsMarkTourCompleted: (...args: unknown[]) => appSettingsMarkTourCompleted(...args),
  },
}));

import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
import { MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
import { tourState } from '$lib/onboarding/state.svelte';
import TourOverlay from '$lib/onboarding/TourOverlay.svelte';

beforeEach(() => {
  localStorage.clear();
  appSettingsMarkTourCompleted.mockResolvedValue({ status: 'ok', data: null });
});

// ── InstanceConceptTooltip ─────────────────────────────────────────────────

// The popover id is generated per-instance by the shared HelpPopover, so locate
// it via the trigger's aria-controls rather than a hardcoded id.
function popoverEl(): HTMLElement {
  const trigger = screen.getByRole('button', { name: /what is an instance/i });
  const id = trigger.getAttribute('aria-controls') as string;
  return document.getElementById(id) as HTMLElement;
}

describe('InstanceConceptTooltip — trigger button classes', () => {
  it('trigger has text-xs class', () => {
    render(InstanceConceptTooltip);
    const btn = screen.getByRole('button', { name: /what is an instance/i });
    expect(btn.className).toContain('text-xs');
  });

  it('trigger has text-placeholder class', () => {
    render(InstanceConceptTooltip);
    const btn = screen.getByRole('button', { name: /what is an instance/i });
    expect(btn.className).toContain('text-placeholder');
  });

  it('trigger has hover:text-secondary class', () => {
    render(InstanceConceptTooltip);
    const btn = screen.getByRole('button', { name: /what is an instance/i });
    expect(btn.className).toContain('hover:text-secondary');
  });

  it('trigger is NOT a .btn-* variant (plain button only)', () => {
    render(InstanceConceptTooltip);
    const btn = screen.getByRole('button', { name: /what is an instance/i });
    expect(btn.className).not.toMatch(/\bbtn-\w/);
  });
});

describe('InstanceConceptTooltip — popover classes (9ce0f0a regression lock)', () => {
  it('popover has normal-case class (override upstream uppercase)', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover).not.toBeNull();
    expect(popover.className).toContain('normal-case');
  });

  it('popover has tracking-normal class (override upstream letter-spacing)', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover.className).toContain('tracking-normal');
  });

  it('popover has bg-surface class', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover.className).toContain('bg-surface');
  });

  it('popover has border-border-subtle class', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover.className).toContain('border-border-subtle');
  });

  it('popover has rounded class', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover.className).toContain('rounded');
  });

  it('popover has shadow-md class', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    expect(popover.className).toContain('shadow-md');
  });

  it('close button inside popover carries .btn-icon variant', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const closeBtn = screen.getByRole('button', { name: /close tooltip/i }) as HTMLElement;
    expect(closeBtn).toHaveBtnVariant('icon');
  });

  it('close button wrapper is absolute top-1 right-1', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    const wrapper = popover.querySelector('.absolute.top-1.right-1') as HTMLElement;
    expect(wrapper).not.toBeNull();
  });

  it('body text has text-xs text-secondary leading-snug pr-6', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    const popover = popoverEl();
    const p = popover.querySelector('p') as HTMLElement;
    expect(p).not.toBeNull();
    expect(p.className).toContain('text-xs');
    expect(p.className).toContain('text-secondary');
    expect(p.className).toContain('leading-snug');
    expect(p.className).toContain('pr-6');
  });
});

// ── ContextualTour ─────────────────────────────────────────────────────────

describe('ContextualTour — popover button variants', () => {
  it('Back button is btn-secondary btn-sm', () => {
    // localStorage is clear so hasSeen returns false → tour auto-starts.
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const back = screen.getByRole('button', { name: /back/i }) as HTMLElement;
    expect(back).toHaveBtnVariant('secondary');
    expect(back).toHaveBtnSize('sm');
  });

  it('Next button is btn-primary btn-sm', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const next = screen.getByRole('button', { name: /next/i }) as HTMLElement;
    expect(next).toHaveBtnVariant('primary');
    expect(next).toHaveBtnSize('sm');
  });

  it('Skip button is btn-ghost (button-like, not a link)', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const skip = screen.getByRole('button', { name: /skip/i }) as HTMLElement;
    expect(skip).toHaveBtnVariant('ghost');
    expect(skip).toHaveBtnSize('sm');
    // Must not look like a hyperlink — btn-tertiary carries the hover underline.
    expect(skip.classList.contains('btn-tertiary')).toBe(false);
  });

  it('popover step counter is text-xs text-muted', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const popover = screen.getByRole('dialog') as HTMLElement;
    const counter = popover.querySelector('.text-xs.text-muted') as HTMLElement;
    expect(counter).not.toBeNull();
    expect(counter.textContent).toMatch(/Step 1 of/);
  });

  it('popover title is font-semibold text-sm text-primary', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const popover = screen.getByRole('dialog') as HTMLElement;
    const title = popover.querySelector('h3') as HTMLElement;
    expect(title.className).toContain('font-semibold');
    expect(title.className).toContain('text-sm');
    expect(title.className).toContain('text-primary');
  });

  it('popover body is text-sm text-secondary', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const popover = screen.getByRole('dialog') as HTMLElement;
    const body = popover.querySelector('p') as HTMLElement;
    expect(body.className).toContain('text-sm');
    expect(body.className).toContain('text-secondary');
  });

  it('popover has bg-surface rounded shadow-xl', () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    const popover = screen.getByRole('dialog') as HTMLElement;
    expect(popover.className).toContain('bg-surface');
    expect(popover.className).toContain('rounded');
    expect(popover.className).toContain('shadow-xl');
  });

  it('does not render when tour has already been seen', () => {
    localStorage.setItem('ftl.tour.manage.v1.done', '1');
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

// ── TourOverlay ────────────────────────────────────────────────────────────

describe('TourOverlay — button variants', () => {
  beforeEach(() => {
    tourState.active = true;
    // Step 0 is the Basic/Advanced chooser (no Back/Skip/Next); the standard
    // nav controls live on the regular steps, so assert their variants there.
    tourState.currentStep = 1;
  });

  it('Back button is btn-secondary btn-sm', () => {
    render(TourOverlay);
    const back = screen.getByRole('button', { name: /back/i }) as HTMLElement;
    expect(back).toHaveBtnVariant('secondary');
    expect(back).toHaveBtnSize('sm');
  });

  it('Next button is btn-primary btn-sm', () => {
    render(TourOverlay);
    const next = screen.getByRole('button', { name: /next/i }) as HTMLElement;
    expect(next).toHaveBtnVariant('primary');
    expect(next).toHaveBtnSize('sm');
  });

  it('Skip tour button is btn-ghost (button-like, not a link)', () => {
    render(TourOverlay);
    const skip = screen.getByRole('button', { name: /skip/i }) as HTMLElement;
    expect(skip).toHaveBtnVariant('ghost');
    expect(skip).toHaveBtnSize('sm');
    // Must not look like a hyperlink — btn-tertiary carries the hover underline.
    expect(skip.classList.contains('btn-tertiary')).toBe(false);
  });

  it('popover has bg-surface rounded shadow-xl', () => {
    render(TourOverlay);
    const dialog = screen.getByRole('dialog') as HTMLElement;
    expect(dialog.className).toContain('bg-surface');
    expect(dialog.className).toContain('rounded');
    expect(dialog.className).toContain('shadow-xl');
  });

  it('popover step counter has text-xs text-muted classes', () => {
    render(TourOverlay);
    const dialog = screen.getByRole('dialog') as HTMLElement;
    const counter = dialog.querySelector('.text-xs.text-muted') as HTMLElement;
    expect(counter).not.toBeNull();
    expect(counter.textContent).toMatch(/Step 2 of/);
  });

  it('popover title is font-semibold text-sm text-primary', () => {
    render(TourOverlay);
    const dialog = screen.getByRole('dialog') as HTMLElement;
    const title = dialog.querySelector('h3') as HTMLElement;
    expect(title.className).toContain('font-semibold');
    expect(title.className).toContain('text-sm');
    expect(title.className).toContain('text-primary');
  });

  it('popover body is text-sm text-secondary', () => {
    render(TourOverlay);
    const dialog = screen.getByRole('dialog') as HTMLElement;
    const body = dialog.querySelector('p') as HTMLElement;
    expect(body.className).toContain('text-sm');
    expect(body.className).toContain('text-secondary');
  });

  it('dim overlay uses bg-black/55 when no spotlight target', () => {
    // Step 0 has targetSelector: null → no spotlight rect → plain full-screen dim.
    render(TourOverlay);
    const dim = document.querySelector('.fixed.inset-0.bg-black\\/55') as HTMLElement;
    expect(dim).not.toBeNull();
  });
});
