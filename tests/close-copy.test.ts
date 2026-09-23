// What the close dialog says. Checked against the real English locale, so the
// server plurals are exercised through the same formatter the dialog uses.
import { get } from 'svelte/store';
import { afterEach, describe, expect, it } from 'vitest';
import {
  closeConfirmLabel,
  closeConfirmVariant,
  closeLines,
  nativeCloseLabels,
} from '$lib/close/close-copy';
import { locale, t } from '$lib/i18n';
import type { CloseLosses } from '$lib/ipc/bindings';

const none: CloseLosses = { games: false, servers: 0, operation: false, unchecked: false };
const tr = () => get(t);

afterEach(() => {
  locale.set('en');
});

describe('the close dialog body', () => {
  it('says nothing when nothing would be lost', () => {
    expect(closeLines(none, tr())).toEqual([]);
  });

  it('names a running game on its own line', () => {
    const lines = closeLines({ ...none, games: true }, tr());
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatch(/^Minecraft is still running/);
  });

  it('reads one server in the singular and several with the count', () => {
    expect(closeLines({ ...none, servers: 1 }, tr())[0]).toMatch(/^A server is still running/);
    expect(closeLines({ ...none, servers: 3 }, tr())[0]).toMatch(/^3 servers are still running/);
  });

  it('keeps the Russian pronoun singular only for exactly one server', () => {
    // ICU "one" also covers 21, 31… — the pronoun clause uses an exact =1.
    locale.set('ru');
    expect(closeLines({ ...none, servers: 1 }, tr())[0]).toContain('он будет остановлен');
    expect(closeLines({ ...none, servers: 21 }, tr())[0]).toContain('21 сервер');
    expect(closeLines({ ...none, servers: 21 }, tr())[0]).toContain('они будут остановлены');
  });

  it('keeps a fixed order: games, servers, operation, unchecked', () => {
    const lines = closeLines({ games: true, servers: 2, operation: true, unchecked: true }, tr());
    expect(lines).toHaveLength(4);
    expect(lines[0]).toMatch(/Minecraft/);
    expect(lines[1]).toMatch(/servers are/);
    expect(lines[2]).toMatch(/operation/);
    expect(lines[3]).toMatch(/earlier session/);
  });

  it('does not threaten to stop a server it could not identify', () => {
    const [line] = closeLines({ ...none, unchecked: true }, tr());
    // The exit hook cannot kill what it cannot see: the sentence says it keeps running.
    expect(line).toMatch(/keeps running after Lucerna closes/);
    expect(line).not.toMatch(/forces/);
  });
});

describe('the close dialog confirm', () => {
  it('reads "Close everything", styled as destructive, when something is killed', () => {
    for (const l of [
      { ...none, games: true },
      { ...none, servers: 1 },
      { ...none, operation: true },
    ]) {
      expect(closeConfirmLabel(l, tr())).toBe('Close everything');
      expect(closeConfirmVariant(l)).toBe('danger');
    }
  });

  it('reads "Close Lucerna", not destructive, when only an unchecked server is involved', () => {
    const l = { ...none, unchecked: true };
    expect(closeConfirmLabel(l, tr())).toBe('Close Lucerna');
    expect(closeConfirmVariant(l)).toBe('primary');
  });
});

describe('the native fallback words', () => {
  it('carry every line in the interface language, plural-free, with two distinct buttons', () => {
    const n = nativeCloseLabels(tr());
    expect(n.title).toBe('Close Lucerna?');
    expect(n.servers_one).toMatch(/^A server is still running/);
    expect(n.servers_many).toMatch(/^Servers are still running/);
    // The native dialog maps its answer by comparing labels.
    expect(n.close_everything).not.toBe(n.cancel);
    expect(n.close_lucerna).not.toBe(n.cancel);
    for (const v of Object.values(n)) expect(v).not.toBe('');
  });
});
