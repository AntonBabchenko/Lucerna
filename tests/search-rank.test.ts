import { describe, expect, it } from 'vitest';
import { prioritizeByTitle } from '$lib/mods/search-rank';

// Test fixtures mirror the shape both ModBrowseView (ModSummary.name) and
// ModpackBrowseView (ModpackHit.title) feed into prioritizeByTitle. The
// helper itself is generic; these tests use plain { name } objects.

const mk = (name: string) => ({ name });
const titleOf = (h: { name: string }) => h.name;

describe('prioritizeByTitle', () => {
  it('returns the same hits in the same order when the query is empty', () => {
    const hits = [mk('A'), mk('B'), mk('C')];
    expect(prioritizeByTitle(hits, '', titleOf)).toEqual(hits);
  });

  it('returns the same hits in the same order when the query is whitespace', () => {
    const hits = [mk('A'), mk('B')];
    expect(prioritizeByTitle(hits, '   ', titleOf)).toEqual(hits);
  });

  it('pushes name-matched hits ahead of merely-description-matched ones', () => {
    // Mimics the screenshot bug: query "create", platform returns popular
    // mods that mention create in their description before the actual
    // Create-named mods because the sort is "downloads".
    const xaero = mk("Xaero's Minimap"); // description-only match
    const create = mk('Create');
    const createFactory = mk('Create: The Factory Must Grow');
    const yeetus = mk('Yeetus Experimentus'); // description-only match
    const ranked = prioritizeByTitle([xaero, yeetus, create, createFactory], 'create', titleOf);
    expect(ranked.map(titleOf)).toEqual([
      'Create',
      'Create: The Factory Must Grow',
      "Xaero's Minimap",
      'Yeetus Experimentus',
    ]);
  });

  it('is case-insensitive', () => {
    const hits = [mk('lowercase only'), mk('CREATE All Caps'), mk('mixed Create case')];
    const ranked = prioritizeByTitle(hits, 'CrEaTe', titleOf);
    expect(ranked.map(titleOf)).toEqual(['CREATE All Caps', 'mixed Create case', 'lowercase only']);
  });

  it('preserves intra-partition order (stable sort)', () => {
    // The platform sort (downloads) is preserved within each partition,
    // so name-matched #1 comes before name-matched #2 by download rank,
    // and same for the non-matched group.
    const hits = [
      mk('Create: First Match (high downloads)'),
      mk('Other 1'),
      mk('Create: Second Match (mid downloads)'),
      mk('Other 2'),
      mk('Create: Third Match (low downloads)'),
    ];
    const ranked = prioritizeByTitle(hits, 'create', titleOf);
    expect(ranked.map(titleOf)).toEqual([
      'Create: First Match (high downloads)',
      'Create: Second Match (mid downloads)',
      'Create: Third Match (low downloads)',
      'Other 1',
      'Other 2',
    ]);
  });

  it('returns an empty array for an empty input', () => {
    expect(prioritizeByTitle([], 'create', titleOf)).toEqual([]);
  });

  it('does not mutate the input list', () => {
    const hits = [mk("Xaero's Minimap"), mk('Create')];
    const snapshot = hits.map(titleOf);
    prioritizeByTitle(hits, 'create', titleOf);
    expect(hits.map(titleOf)).toEqual(snapshot);
  });
});
