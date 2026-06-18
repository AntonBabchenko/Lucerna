import { describe, expect, it } from 'vitest';
import { appendCapped } from '$lib/servers/console-buffer';

describe('appendCapped', () => {
  it('appends under cap', () => {
    expect(appendCapped(['a'], 'b', 3)).toEqual(['a', 'b']);
  });

  it('drops oldest over cap', () => {
    expect(appendCapped(['a', 'b', 'c'], 'd', 3)).toEqual(['b', 'c', 'd']);
  });

  it('handles exact cap size', () => {
    expect(appendCapped(['a', 'b'], 'c', 3)).toEqual(['a', 'b', 'c']);
  });

  it('handles empty array', () => {
    expect(appendCapped([], 'a', 3)).toEqual(['a']);
  });

  it('never grows beyond max', () => {
    let arr: string[] = [];
    for (let i = 0; i < 10; i++) arr = appendCapped(arr, String(i), 5);
    expect(arr.length).toBe(5);
    expect(arr).toEqual(['5', '6', '7', '8', '9']);
  });
});
