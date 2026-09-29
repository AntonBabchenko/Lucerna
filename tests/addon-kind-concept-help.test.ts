// AddonKindConceptHelp — the (i) at the end of the Add-ons kind row. It shows
// the concept explainer of the ACTIVE content kind. The fragments below are
// verbatim substrings of the EN values in src/lib/i18n/locales/en.json. The
// data pack branch delegates to DatapackConceptHelp, whose copy is asserted in
// tests/datapack-concept-help.test.ts, so only its identity is checked here.

import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import AddonKindConceptHelp from '../src/lib/onboarding/AddonKindConceptHelp.svelte';
import { explanationState } from '../src/lib/onboarding/explanation-level.svelte';

// explanationState is a module singleton; reset so no level leaks across tests.
afterEach(() => {
  explanationState.level = 'basic';
});

async function openParagraphs(name: RegExp): Promise<string[]> {
  const trigger = await screen.findByRole('button', { name });
  await fireEvent.click(trigger);
  const id = trigger.getAttribute('aria-controls') as string;
  const popover = document.getElementById(id);
  if (!popover) throw new Error('concept popover is not open');
  return [...popover.querySelectorAll('p')].map((p) => p.textContent ?? '');
}

const CASES = [
  {
    kind: 'mod',
    name: /what are mods\?/i,
    advanced: [
      'code that changes the game itself',
      'mod loader',
      "every player's game",
      'Pick a mod when',
    ],
    basicFirst: 'Mods change the game itself',
    advancedOnly: 'code that changes',
  },
  {
    kind: 'resource_pack',
    name: /what are resource packs\?/i,
    advanced: [
      'how the game looks and sounds',
      'No loader needed',
      'Options → Resource Packs',
      'Pick a resource pack when',
    ],
    basicFirst: "the game's look and sound",
    advancedOnly: 'models',
  },
  {
    kind: 'shader',
    name: /what are shaders\?/i,
    advanced: [
      'the lighting and the picture',
      'shader loader',
      'Options → Video Settings',
      'Pick shaders for the visuals',
    ],
    basicFirst: 'make the picture prettier',
    advancedOnly: 'sun rays',
  },
] as const;

describe('AddonKindConceptHelp', () => {
  for (const c of CASES) {
    it(`${c.kind}: four paragraphs in order at the Advanced level`, async () => {
      explanationState.level = 'advanced';
      render(AddonKindConceptHelp, { props: { kind: c.kind } });
      const paragraphs = await openParagraphs(c.name);
      expect(paragraphs).toHaveLength(4);
      c.advanced.forEach((fragment, i) => {
        expect(paragraphs[i]).toContain(fragment);
      });
    });

    it(`${c.kind}: the Basic wording replaces the Advanced one`, async () => {
      explanationState.level = 'basic';
      render(AddonKindConceptHelp, { props: { kind: c.kind } });
      const paragraphs = await openParagraphs(c.name);
      expect(paragraphs).toHaveLength(4);
      expect(paragraphs[0]).toContain(c.basicFirst);
      expect(paragraphs[0]).not.toContain(c.advancedOnly);
    });
  }

  it('datapack: delegates to the data pack explainer', async () => {
    render(AddonKindConceptHelp, { props: { kind: 'datapack' } });
    expect(await screen.findByRole('button', { name: /what are data packs\?/i })).toBeTruthy();
  });

  it('follows the kind prop: a new kind renames the trigger', async () => {
    const { rerender } = render(AddonKindConceptHelp, { props: { kind: 'mod' } });
    await screen.findByRole('button', { name: /what are mods\?/i });
    await rerender({ kind: 'shader' });
    expect(await screen.findByRole('button', { name: /what are shaders\?/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /what are mods\?/i })).toBeNull();
  });
});
