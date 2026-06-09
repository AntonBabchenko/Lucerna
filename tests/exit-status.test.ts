import { describe, expect, it } from 'vitest';
import { classifyExit } from '$lib/overview/exit-status';

describe('classifyExit', () => {
  it('reports a user-requested stop as "stopped", ignoring the OS code', () => {
    expect(classifyExit({ code: 1, user_requested: true })).toEqual({ kind: 'stopped' });
    // Unix signal-kill surfaces -1, but it is still a user stop.
    expect(classifyExit({ code: -1, user_requested: true })).toEqual({ kind: 'stopped' });
  });

  it('reports a code-0 exit the user did not stop as a clean close', () => {
    expect(classifyExit({ code: 0, user_requested: false })).toEqual({ kind: 'clean' });
  });

  it('reports a non-zero exit the user did not initiate as a crash, carrying the code', () => {
    expect(classifyExit({ code: 1, user_requested: false })).toEqual({ kind: 'crashed', code: 1 });
    expect(classifyExit({ code: -1, user_requested: false })).toEqual({
      kind: 'crashed',
      code: -1,
    });
  });
});
