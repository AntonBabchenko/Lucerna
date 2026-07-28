import { describe, expect, it } from 'vitest';
import { resolveCapeAction } from '$lib/accounts/skin-library/cape-action';

describe('resolveCapeAction', () => {
  it('clears the cape when the entry saved "no cape"', () => {
    expect(resolveCapeAction(null, ['a', 'b'])).toEqual({ kind: 'clear' });
  });

  it('sets the cape when the applying account owns it', () => {
    expect(resolveCapeAction('b', ['a', 'b'])).toEqual({ kind: 'set', id: 'b' });
  });

  it('skips the cape step when the account does not own the saved cape', () => {
    expect(resolveCapeAction('c', ['a', 'b'])).toEqual({ kind: 'skip' });
  });

  it('skips when the account owns no capes at all', () => {
    expect(resolveCapeAction('a', [])).toEqual({ kind: 'skip' });
  });
});
