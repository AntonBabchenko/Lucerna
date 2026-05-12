import { describe, expect, test } from 'vitest';

describe('test infrastructure', () => {
  test('vitest runs', () => {
    expect(1 + 1).toBe(2);
  });

  test('strings work', () => {
    expect('hello'.toUpperCase()).toBe('HELLO');
  });
});
