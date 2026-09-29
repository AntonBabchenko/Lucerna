/**
 * The guarded-operation dialog's sentences follow the ACTUAL counts (plan §5b V1): «Без него не
 * загрузятся:» over a single dependent, «Пока они отключены, он не загрузится» over a single
 * requirement, read wrong. Each pronoun and verb now agrees with the count it stands for — the
 * mods leaving or coming on, the dependents, the requirements.
 */
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { ImpactView } from '$lib/mods/mod-ops.svelte';
import ModImpactDialog from '$lib/mods/ops/ModImpactDialog.svelte';

const mod = (sha1: string, name: string) => ({ sha1, name });
const dependent = (sha1: string, name: string) => ({ sha1, name, needs: ['Sodium'] });
const body = () => screen.getByRole('dialog').textContent ?? '';
const show = (view: ImpactView) => render(ModImpactDialog, { props: { view, onChoose: () => {} } });

beforeEach(() => locale.set('en'));
afterEach(() => locale.set('en'));

describe('dependents: the lead and the note agree with both counts', () => {
  const disabling = (targets: number, dependents: number): ImpactView => ({
    mode: 'dependents-on-disable',
    targets: [mod('s', 'Sodium'), mod('f', 'Fabric API')].slice(0, targets),
    dependents: [dependent('i', 'Indium'), dependent('r', 'Iris')].slice(0, dependents),
  });

  it('one mod, one dependent', () => {
    show(disabling(1, 1));
    expect(body()).toContain("This mod won't load without it:");
    expect(body()).toContain("The game won't start while this mod is enabled.");
  });

  it('one mod, several dependents', () => {
    show(disabling(1, 2));
    expect(body()).toContain("These mods won't load without it:");
    expect(body()).toContain("The game won't start while these mods are enabled.");
  });

  it('several mods, one dependent', () => {
    show(disabling(2, 1));
    expect(body()).toContain("This mod won't load without them:");
  });

  it('in Russian, verb and pronouns follow the counts', () => {
    locale.set('ru');
    const { unmount } = show(disabling(1, 1));
    expect(body()).toContain('Без него не загрузится:');
    expect(body()).toContain('Пока этот мод включён, игра не запустится.');
    unmount();
    const second = show(disabling(1, 2));
    expect(body()).toContain('Без него не загрузятся:');
    expect(body()).toContain('Пока эти моды включены, игра не запустится.');
    second.unmount();
    show(disabling(2, 1));
    expect(body()).toContain('Без них не загрузится:');
  });
});

describe('enable: the body names who won’t load while what is disabled', () => {
  const enabling = (targets: number, requirements: number): ImpactView => ({
    mode: 'enable-with-requirements',
    targets: [mod('d', 'Dynamic FPS'), mod('z', 'Zoomify')].slice(0, targets),
    requirements: [mod('y', 'YetAnotherConfigLib'), mod('c', 'Cloth Config API')].slice(
      0,
      requirements,
    ),
  });

  it('one mod, one requirement', () => {
    show(enabling(1, 1));
    expect(body()).toContain("Dynamic FPS won't load while YetAnotherConfigLib is disabled.");
  });

  it('several of each', () => {
    show(enabling(2, 2));
    expect(body()).toContain(
      "These mods won't load while YetAnotherConfigLib, Cloth Config API are disabled.",
    );
  });

  it('in Russian, the verbs follow the counts', () => {
    locale.set('ru');
    const { unmount } = show(enabling(1, 1));
    expect(body()).toContain('Dynamic FPS не загрузится, пока YetAnotherConfigLib отключён.');
    unmount();
    show(enabling(2, 2));
    expect(body()).toContain(
      'Эти моды не загрузятся, пока отключены YetAnotherConfigLib, Cloth Config API.',
    );
  });
});

// Plan §5c V3 (screenshot 04d): «Не удалось проверить, зависят ли от него другие моды» — «него» had
// nothing to refer to: a failed check lists no mods, so the title's pronoun pointed at nothing. The
// title names the mod it was about, or counts them.
describe('a check that could not run names what it was about', () => {
  const failed = (action: 'disable' | 'uninstall' | 'enable', targets: number): ImpactView => ({
    mode: 'impact-check-failed',
    action,
    targets: [mod('f', 'FerriteCore'), mod('s', 'Sodium')].slice(0, targets),
    error: 'os error 32',
  });
  const title = () => screen.getByRole('dialog').getAttribute('aria-labelledby') ?? '';
  const titleText = () => document.getElementById(title())?.textContent?.trim();

  it('names the one mod, and counts several', () => {
    const { unmount } = show(failed('uninstall', 1));
    expect(titleText()).toBe("Couldn't check whether other mods depend on FerriteCore");
    unmount();
    show(failed('disable', 2));
    expect(titleText()).toBe("Couldn't check whether other mods depend on 2 mods");
  });

  it('an enable names the mod too', () => {
    const { unmount } = show(failed('enable', 1));
    expect(titleText()).toBe("Couldn't check whether FerriteCore needs disabled mods");
    unmount();
    show(failed('enable', 2));
    expect(titleText()).toBe("Couldn't check whether 2 mods need disabled mods");
  });

  it('in Russian, with the cases the count takes', () => {
    locale.set('ru');
    const cases: [ImpactView, string][] = [
      [failed('uninstall', 1), 'Не удалось проверить, зависят ли от FerriteCore другие моды'],
      [failed('disable', 2), 'Не удалось проверить, зависят ли от 2 модов другие моды'],
      [failed('enable', 1), 'Не удалось проверить, нужны ли моду FerriteCore отключённые моды'],
      [failed('enable', 2), 'Не удалось проверить, нужны ли 2 модам отключённые моды'],
    ];
    for (const [view, expected] of cases) {
      const { unmount } = show(view);
      expect(titleText()).toBe(expected);
      unmount();
    }
  });
});
