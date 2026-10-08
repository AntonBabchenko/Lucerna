<script lang="ts">
  import type { DepRoot, DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import DepTree from '../DepTree.svelte';
  import {
    DEPS_UNKNOWN_KEY,
    type DepTreeCtx,
    EMPTY_TREE_CTX,
    type ModTarget,
  } from '../dep-node-state';
  import type { RequiredByEntry } from './dep-graph.svelte';

  let {
    id = undefined,
    root,
    requiredBy,
    onInstall,
    onJump,
    onOpenDetail,
    treeCtx = EMPTY_TREE_CTX,
    tourAnchors = false,
  }: {
    /** Its element id: the row's relation cell names it in `aria-controls`. */
    id?: string;
    root: DepRoot;
    requiredBy: RequiredByEntry[];
    // The node and the mod that declared it (null under an absent parent) — see DepTree.
    onInstall: (node: DepTreeNode, dependentSha1: string | null) => void;
    // A «Required by» entry passes its jar too: the jump goes to that very row.
    onJump: (target: ModTarget) => void;
    onOpenDetail: (source: ModSource, projectId: string) => void;
    // What the trees need to say what the loader does about each dependency.
    treeCtx?: DepTreeCtx;
    /** This panel is the deps tour's target: its Requires / Optional blocks carry the tour's
     *  anchors (only one panel may — the tour finds its anchor by the first match). */
    tourAnchors?: boolean;
  } = $props();

  // The headings name the trees (`aria-labelledby`); a row's mod appears once in the list.
  const reqId = $derived(`dep-req-${root.sha1}`);
  const optId = $derived(`dep-opt-${root.sha1}`);
  // Why the platform could not describe this mod's installed version, when it could not.
  const unknownWhy = $derived(root.deps_unknown ?? null);
  // The panel's own mod, for every level of its trees: an edge back to it reads «this mod».
  const rootKey = $derived(`${root.source}:${root.project_id}`);

  // «Required by» rows (spec 2026-10-07 §8): a library can be required by dozens of mods, so the
  // first few show and the rest wait for the ask.
  const BY_SHOWN = 5;
  let showAllBy = $state(false);
  const shownBy = $derived(showAllBy ? requiredBy : requiredBy.slice(0, BY_SHOWN));
  const byId = $derived(`dep-by-${root.sha1}`);
  // A dependent's very jar: its switch, Remove and «show» act on that row, never a namesake's.
  const targetOf = (e: RequiredByEntry): ModTarget => ({
    source: e.source,
    project_id: e.projectId,
    name: e.name,
    sha1: e.sha1,
  });

  // A row is clicked like a tree row: anywhere on it opens the mod, except its own buttons (the
  // name's click bubbles here too).
  function onRowClick(e: MouseEvent, entry: RequiredByEntry) {
    if ((e.target as Element).closest('button')) return;
    onOpenDetail(entry.source, entry.projectId);
  }
</script>

<!-- onAdd and onInstall both resolve to the same install handler here: in this
     view, "install missing required" and "add recommended" trigger the identical
     resolve-and-install path. DepTree keeps them separate for other callers. -->
<!-- Inset, bordered, and gapped below so the expanded tree reads as nested
     content under its mod and is clearly separated from the next mod row
     (a full-width grey block blended into the following row). On the surface,
     not a grey tint: the muted headings and cycle marker were 4.46:1 on the old
     bg-subtle/40 in the light theme. Its right edge is the card's: 7 px of margin, the
     border's 1 and a row's 4 make the card row's 12 (`CardShell` `pr-3`), so the rows' switch
     and Remove stand under the mod row's own (spec 2026-10-07 §8). -->
<div
  {id}
  class="mb-2 ml-3 mr-[7px] rounded-md border border-border-subtle bg-surface py-2 pl-2 pr-0"
  data-dep-section
>
  {#if unknownWhy}
    <!-- Its lists are empty because they are unknown — never an empty «Requires». Say so,
         and why. -->
    <p class="text-xs text-secondary mt-1 pr-3" data-testid="deps-unknown">
      {$t(DEPS_UNKNOWN_KEY[unknownWhy])}
    </p>
  {/if}
  <!-- «Requires» is safe to say again: each absent node now carries its truthful state
       (loader-required / platform-only / unknown) instead of the heading hedging for all of
       them (spec 2026-09-28 D4, overriding 2026-08-03 descriptor-authority §6). -->
  <!-- Each section is a block with a stripe of its own (spec 2026-10-07 §9): «Requires» the accent,
       headed by the relation cell's ⛓; «Optional» dashed; «Required by» its own hue, headed by the
       cell's ↑ — told apart before a word is read. -->
  {#if root.required.length > 0}
    <section
      class="mt-1 border-l-2 border-accent pl-1.5"
      data-dep-block="requires"
      data-tour-ctx={tourAnchors ? 'deps-requires' : undefined}
    >
      <div
        id={reqId}
        class="flex items-center gap-1 text-[10px] uppercase tracking-wide text-muted"
      >
        <Icon name="link" size={12} class="text-accent" />{$t('mods.installed.sectionRequires')}
      </div>
      <DepTree
        nodes={root.required}
        labelledby={reqId}
        {rootKey}
        dependentSha1={root.sha1}
        ctx={treeCtx}
        {onInstall}
        onAdd={onInstall}
        {onJump}
        {onOpenDetail}
      />
    </section>
  {/if}
  {#if root.optional.length > 0}
    <section
      class="mt-2 border-l-2 border-dashed border-border-emphasis pl-1.5"
      data-dep-block="optional"
      data-tour-ctx={tourAnchors ? 'deps-optional' : undefined}
    >
      <div id={optId} class="text-[10px] uppercase tracking-wide text-muted">
        {$t('mods.installed.sectionOptional')}
      </div>
      <DepTree
        nodes={root.optional}
        labelledby={optId}
        {rootKey}
        dependentSha1={root.sha1}
        ctx={treeCtx}
        {onInstall}
        onAdd={onInstall}
        {onJump}
        {onOpenDetail}
      />
    </section>
  {/if}
  {#if requiredBy.length > 0}
    <section class="mt-2 border-l-2 border-relation-by pl-1.5" data-dep-block="required-by">
      <div id={byId} class="flex items-center gap-1 text-[10px] uppercase tracking-wide text-muted">
        <Icon name="arrowUp" size={12} class="text-relation-by" />{$t(
          'mods.installed.sectionRequiredBy',
        )}
      </div>
      <!-- Rows with a tree row's anatomy, its chevron column empty: «show in the list» before the
         name, the dependent's switch and Remove at the end, under the tree's and the mod row's
         own. A dependent is installed and enabled — the list counts no other. -->
      <ul class="text-xs" aria-labelledby={byId}>
        {#each shownBy as e (e.sha1)}
          {@const jump = $t('mods.deps.jumpToTitle', { name: e.name })}
          {@const off = $t('mods.deps.disableAriaLabel', { name: e.name })}
          {@const remove = $t('mods.deps.uninstallAriaLabel', { name: e.name })}
          <li>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions — the
               pointer's shortcut to the name button, which is the keyboard path. -->
            <div
              class="group flex min-h-7 cursor-pointer flex-wrap items-center gap-x-2 rounded px-1 transition-colors hover:bg-subtle"
              data-required-by-row
              onclick={(ev) => onRowClick(ev, e)}
            >
              <span class="flex min-w-0 grow basis-[12ch] items-center gap-1.5">
                <span class="inline-block w-7 shrink-0" aria-hidden="true"></span>
                <span class="inline-flex w-7 shrink-0 justify-center" data-slot="locate">
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm"
                    aria-label={jump}
                    use:tooltip={jump}
                    onclick={() => onJump(targetOf(e))}><Icon name="locate" size={15} /></button
                  >
                </span>
                <button
                  type="button"
                  class="min-w-0 truncate rounded text-left text-secondary transition-colors group-hover:text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
                  use:tooltip={{ text: e.name, whenOverflowing: true, describe: false }}
                  onclick={() => onOpenDetail(e.source, e.projectId)}>{e.name}</button
                >
              </span>
              <span class="ml-auto flex shrink-0 items-center gap-1">
                <span class="inline-flex w-7 shrink-0 justify-center" data-slot="toggle">
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm btn-icon-success"
                    aria-label={off}
                    use:tooltip={off}
                    onclick={() => treeCtx.onDisable(targetOf(e))}
                    ><Icon name="power" size={15} /></button
                  >
                </span>
                <span class="inline-flex w-7 shrink-0 justify-center" data-slot="presence">
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm btn-icon-danger"
                    aria-label={remove}
                    use:tooltip={remove}
                    onclick={() => treeCtx.onUninstall(targetOf(e))}
                    ><Icon name="trash" size={15} /></button
                  >
                </span>
              </span>
            </div>
          </li>
        {/each}
      </ul>
      {#if requiredBy.length > BY_SHOWN}
        <!-- In the name's column, like a row's text. -->
        <div class="flex min-h-7 items-center gap-1.5 px-1 text-xs">
          <span class="inline-block w-7 shrink-0" aria-hidden="true"></span>
          <span class="inline-block w-7 shrink-0" aria-hidden="true"></span>
          <button type="button" class="btn-link" onclick={() => (showAllBy = !showAllBy)}>
            {showAllBy
              ? $t('mods.installed.requiredByFewer')
              : $t('mods.installed.requiredByMore', { count: requiredBy.length - BY_SHOWN })}
          </button>
        </div>
      {/if}
    </section>
  {/if}
</div>
