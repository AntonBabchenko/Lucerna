// The per-instance action menu, built once for every surface that shows it
// (the sidebar list and the Settings tab's Delete) so they cannot drift apart.
//
// Labels follow the app's menu convention: an item that mirrors a control
// reuses that control's key (as the mod-card and log-row menus do), so the
// button and the menu item can never say different things.
import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import type { IconName } from '$lib/ui/icons';
import type { ContextMenuItem } from '$lib/ui/menu-item';

export type DeleteBlock = 'running' | 'busy' | 'last' | null;

/** Why an instance cannot be deleted right now. Running outranks busy outranks
 *  last: the first two the user can wait out, the last needs another instance.
 *  `busy` is a task scoped to the instance (install, verify, mod install, pack
 *  update, world migration) — the backend's delete gate does not see those. The
 *  Settings tab's Delete button and the menu item both ask here, so they cannot
 *  disagree. */
export function deleteBlockedReason(s: {
  running: boolean;
  busy: boolean;
  last: boolean;
}): DeleteBlock {
  if (s.running) return 'running';
  if (s.busy) return 'busy';
  if (s.last) return 'last';
  return null;
}

/** A missing handler omits its item — a surface offers only what it can do. */
export interface InstanceMenuHandlers {
  onActivate?: (id: string) => void;
  onRename?: (id: string) => void;
  onClone?: (id: string) => void;
  onShortcut?: (id: string) => void;
  onTranslations?: (id: string) => void;
  onExport?: (id: string) => void;
  onOpenFolder?: (id: string) => void;
  onOpenModsFolder?: (id: string) => void;
  onDelete?: (id: string) => void;
}

export interface InstanceMenuContext {
  instance: Pick<InstanceWithStatus, 'id' | 'loader'>;
  isActive: boolean;
  isRunning: boolean;
  /** A task runs on the instance; blocks Delete. Defaults to false. */
  isBusy?: boolean;
  isLast: boolean;
  /** `manage-ctx` / `sidebar-ctx` — keeps each surface's test ids stable. */
  testIdPrefix: string;
  t: Translate;
  handlers: InstanceMenuHandlers;
}

// Groups render with a rule between them. Empty groups vanish first, so an
// omitted item can never leave a leading or doubled separator behind.
function joinGroups(groups: ContextMenuItem[][]): ContextMenuItem[] {
  return groups
    .filter((group) => group.length > 0)
    .flatMap((group, gi) =>
      group.map((item, ii) => (gi > 0 && ii === 0 ? { ...item, separatorBefore: true } : item)),
    );
}

type Handler = ((id: string) => void) | undefined;
type Extra = Partial<ContextMenuItem>;

const blocked = (t: Translate, reason: TranslationKey | null): Extra =>
  reason ? { disabled: true, disabledReason: t(reason) } : {};

function deleteReasonKey(block: DeleteBlock): TranslationKey | null {
  if (block === 'running') return 'instance.manage.runningBlocked';
  if (block === 'busy') return 'instance.settings.busyBlocked';
  if (block === 'last') return 'instance.menu.reasonLast';
  return null;
}

// One item, or none when the surface has no handler for it.
function entryFor(ctx: InstanceMenuContext) {
  // Plain capture: Menu calls onClose() before onSelect(), and a caller may
  // null its reactive menu state in onClose — a handler must not read it.
  const id = ctx.instance.id;
  return (
    slug: string,
    label: TranslationKey,
    icon: IconName,
    handler: Handler,
    extra: Extra = {},
  ): ContextMenuItem[] =>
    handler
      ? [
          {
            label: ctx.t(label),
            icon,
            testId: `${ctx.testIdPrefix}-${slug}`,
            onSelect: () => handler(id),
            ...extra,
          },
        ]
      : [];
}

export function buildInstanceMenuItems(ctx: InstanceMenuContext): ContextMenuItem[] {
  const { instance, isActive, isRunning, isLast, t, handlers: h } = ctx;
  const entry = entryFor(ctx);
  // Vanilla loads no mods: nothing to export, no mods folder worth opening.
  const modded = instance.loader !== 'vanilla';
  const whileRunning = blocked(t, isRunning ? 'instance.manage.runningBlocked' : null);
  const deleteBlock = blocked(
    t,
    deleteReasonKey(
      deleteBlockedReason({ running: isRunning, busy: ctx.isBusy ?? false, last: isLast }),
    ),
  );
  const activate = isActive ? undefined : h.onActivate;

  return joinGroups([
    [
      ...entry('make-active', 'instance.menu.makeActive', 'switch', activate),
      ...entry('rename', 'instance.menu.rename', 'edit', h.onRename),
    ],
    [
      ...entry('clone-instance', 'instance.manage.cloneBtn', 'copy', h.onClone, whileRunning),
      ...entry('create-shortcut', 'instance.menu.createShortcut', 'monitor', h.onShortcut),
      ...entry('translations', 'instance.manage.translationsBtn', 'languages', h.onTranslations),
      ...entry('export', 'page.overview.exportModpack', 'package', modded ? h.onExport : undefined),
    ],
    [
      ...entry('open-folder', 'instance.menu.openFolder', 'folderOpen', h.onOpenFolder),
      ...entry(
        'open-mods-folder',
        'instance.menu.openModsFolder',
        'folderOpen',
        modded ? h.onOpenModsFolder : undefined,
      ),
    ],
    [
      ...entry('delete', 'instance.manage.deleteBtn', 'trash', h.onDelete, {
        danger: true,
        ...deleteBlock,
      }),
    ],
  ]);
}
