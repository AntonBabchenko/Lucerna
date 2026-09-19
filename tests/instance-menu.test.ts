import { describe, expect, it, vi } from 'vitest';
import type { Translate } from '$lib/i18n';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import {
  buildInstanceMenuItems,
  deleteBlockedReason,
  type InstanceMenuContext,
  type InstanceMenuHandlers,
} from '$lib/instances/instance-menu';

// The key IS the label here, so assertions name what the item would say.
const t = ((key: string) => key) as Translate;

function allHandlers(): Required<InstanceMenuHandlers> {
  return {
    onActivate: vi.fn(),
    onRename: vi.fn(),
    onClone: vi.fn(),
    onShortcut: vi.fn(),
    onTranslations: vi.fn(),
    onExport: vi.fn(),
    onOpenFolder: vi.fn(),
    onOpenModsFolder: vi.fn(),
    onDelete: vi.fn(),
  };
}

function build(over: Partial<InstanceMenuContext> = {}) {
  return buildInstanceMenuItems({
    instance: { id: 'b', loader: 'fabric' },
    isActive: false,
    isRunning: false,
    isLast: false,
    testIdPrefix: 'x',
    t,
    handlers: allHandlers(),
    ...over,
  });
}

const ids = (items: ReturnType<typeof build>) => items.map((i) => i.testId);
const byId = (items: ReturnType<typeof build>, id: string) => items.find((i) => i.testId === id);

describe('buildInstanceMenuItems', () => {
  it('lists all nine items in order for a modded, idle, non-active instance', () => {
    expect(ids(build())).toEqual([
      'x-make-active',
      'x-rename',
      'x-clone-instance',
      'x-create-shortcut',
      'x-translations',
      'x-export',
      'x-open-folder',
      'x-open-mods-folder',
      'x-delete',
    ]);
  });

  it('starts each group after the first with a separator, and only there', () => {
    const separated = build()
      .filter((i) => i.separatorBefore)
      .map((i) => i.testId);
    expect(separated).toEqual(['x-clone-instance', 'x-open-folder', 'x-delete']);
  });

  // Menu.svelte keys its rows by label, so two items that SAY the same thing
  // would crash it — checked against the real dictionaries, not the keys.
  it.each([
    ['en', en],
    ['ru', ru],
  ])('gives every item a distinct label in %s', (_lang, dict) => {
    const resolve = ((key: string) =>
      key
        .split('.')
        .reduce<unknown>(
          (node, part) => (node as Record<string, unknown>)?.[part],
          dict,
        )) as Translate;
    const labels = build({ t: resolve }).map((i) => i.label);
    expect(labels.every((l) => typeof l === 'string' && l.length > 0)).toBe(true);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it('says what the mirrored control says', () => {
    const items = build();
    expect(byId(items, 'x-clone-instance')?.label).toBe('instance.manage.cloneBtn');
    expect(byId(items, 'x-translations')?.label).toBe('instance.manage.translationsBtn');
    expect(byId(items, 'x-export')?.label).toBe('page.overview.exportModpack');
    expect(byId(items, 'x-delete')?.label).toBe('instance.manage.deleteBtn');
  });

  it('omits Make active on the active instance', () => {
    expect(ids(build({ isActive: true }))).not.toContain('x-make-active');
  });

  it('omits Export and Mods folder on vanilla', () => {
    const got = ids(build({ instance: { id: 'a', loader: 'vanilla' } }));
    expect(got).not.toContain('x-export');
    expect(got).not.toContain('x-open-mods-folder');
    expect(got).toContain('x-open-folder');
  });

  it('omits an item whose handler is missing, without leaving a stray separator', () => {
    const items = build({ handlers: { onClone: vi.fn(), onShortcut: vi.fn() } });
    expect(ids(items)).toEqual(['x-clone-instance', 'x-create-shortcut']);
    expect(items.some((i) => i.separatorBefore)).toBe(false);
  });

  it('disables Clone and Delete with the running reason while the game runs', () => {
    // isLast too: running must win over last — it is the one the user can act on.
    const items = build({ isRunning: true, isLast: true });
    for (const id of ['x-clone-instance', 'x-delete']) {
      expect(byId(items, id)?.disabled).toBe(true);
      expect(byId(items, id)?.disabledReason).toBe('instance.manage.runningBlocked');
    }
  });

  it('leaves every other item enabled while the game runs', () => {
    const others = build({ isRunning: true }).filter(
      (i) => i.testId !== 'x-clone-instance' && i.testId !== 'x-delete',
    );
    expect(others.some((i) => i.disabled)).toBe(false);
  });

  it('disables Delete on the last instance with its own reason', () => {
    const del = byId(build({ isLast: true }), 'x-delete');
    expect(del?.disabled).toBe(true);
    expect(del?.disabledReason).toBe('instance.menu.reasonLast');
    expect(del?.danger).toBe(true);
  });

  it('enables Delete and gives no reason when nothing blocks it', () => {
    const del = byId(build(), 'x-delete');
    expect(del?.disabled).toBeUndefined();
    expect(del?.disabledReason).toBeUndefined();
  });

  it('passes the instance id to the handler', () => {
    const handlers = allHandlers();
    byId(build({ handlers }), 'x-export')?.onSelect();
    expect(handlers.onExport).toHaveBeenCalledWith('b');
  });
});

describe('deleteBlockedReason', () => {
  it('ranks running over last', () => {
    expect(deleteBlockedReason(true, true)).toBe('running');
    expect(deleteBlockedReason(false, true)).toBe('last');
    expect(deleteBlockedReason(false, false)).toBeNull();
  });
});
