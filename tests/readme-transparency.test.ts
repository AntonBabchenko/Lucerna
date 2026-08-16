// The README's transparency paragraph is a shipped claim about the code.
// 'Every process goes through one Rust module' was false: the opener
// plugin spawns the default browser / file manager outside process::,
// from call sites enumerated by structural_no_raw_spawn.rs and
// tools/check-opener-calls.mjs. The paragraph must say that.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const readme = readFileSync(resolve(process.cwd(), 'README.md'), 'utf8');
const flat = readme.replace(/\s+/g, ' ');

describe('README transparency paragraph', () => {
  it('does not claim every spawn goes through one Rust module', () => {
    expect(flat).not.toContain('every process the launcher spawns goes through one Rust module');
  });

  it('names both spawn surfaces truthfully', () => {
    expect(flat).toContain('Every subprocess the launcher runs is built in one Rust module');
    expect(flat).toContain('your default browser or file manager');
    expect(flat).toContain('a fixed, test-enforced set of call sites');
  });
});
