<script lang="ts">
  import type { DepRoot, DepTreeNode, ModSource } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import DepTree from '../DepTree.svelte';
  import { DEPS_UNKNOWN_KEY, type DepTreeCtx, EMPTY_TREE_CTX } from '../dep-node-state';
  import type { RequiredByEntry } from './dep-graph.svelte';

  let {
    id = undefined,
    root,
    requiredBy,
    onInstall,
    onJump,
    onOpenDetail,
    treeCtx = EMPTY_TREE_CTX,
  }: {
    /** Its element id: the row's relation cell names it in `aria-controls`. */
    id?: string;
    root: DepRoot;
    requiredBy: RequiredByEntry[];
    // The node and the mod that declared it (null under an absent parent) — see DepTree.
    onInstall: (node: DepTreeNode, dependentSha1: string | null) => void;
    // A «Required by» entry passes its jar too: the jump goes to that very row.
    onJump: (target: {
      source: ModSource;
      project_id: string;
      name: string;
      sha1?: string;
    }) => void;
    onOpenDetail: (source: ModSource, projectId: string) => void;
    // What the trees need to say what the loader does about each dependency.
    treeCtx?: DepTreeCtx;
  } = $props();

  // The headings name the trees (`aria-labelledby`); a row's mod appears once in the list.
  const reqId = $derived(`dep-req-${root.sha1}`);
  const optId = $derived(`dep-opt-${root.sha1}`);
  // Why the platform could not describe this mod's installed version, when it could not.
  const unknownWhy = $derived(root.deps_unknown ?? null);
  // The panel's own mod, for every level of its trees: an edge back to it reads «this mod».
  const rootKey = $derived(`${root.source}:${root.project_id}`);

  // A chip is clicked like a tree row (spec 2026-10-07 D6): anywhere on it opens the mod, except
  // its own buttons (the name's click bubbles here too).
  function onChipClick(e: MouseEvent, entry: RequiredByEntry) {
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
     bg-subtle/40 in the light theme. -->
<div {id} class="mx-3 mb-2 rounded-md border border-border-subtle bg-surface px-3 py-2">
  {#if unknownWhy}
    <!-- Its lists are empty because they are unknown — never an empty «Requires». Say so,
         and why. -->
    <p class="text-xs text-secondary mt-1" data-testid="deps-unknown">
      {$t(DEPS_UNKNOWN_KEY[unknownWhy])}
    </p>
  {/if}
  <!-- «Requires» is safe to say again: each absent node now carries its truthful state
       (loader-required / platform-only / unknown) instead of the heading hedging for all of
       them (spec 2026-09-28 D4, overriding 2026-08-03 descriptor-authority §6). -->
  {#if root.required.length > 0}
    <div id={reqId} class="text-[10px] uppercase tracking-wide text-muted mt-1">
      {$t('mods.installed.sectionRequires')}
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
  {/if}
  {#if root.optional.length > 0}
    <div id={optId} class="text-[10px] uppercase tracking-wide text-muted mt-2">
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
  {/if}
  {#if requiredBy.length > 0}
    <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
      {$t('mods.installed.sectionRequiredBy')}
    </div>
    <div class="flex flex-wrap gap-x-1.5 gap-y-0.5 text-xs">
      {#each requiredBy as e (e.sha1)}
        <!-- A chip, not a row: a library can be required by dozens of mods. It reads like a
             tree row — the row's hover fill, a click on it opens the mod — and its locate button
             shows the requiring mod's own row, as in the tree. The two stay together when the
             list wraps. -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions — the
             pointer's shortcut to the name button, which is the keyboard path. -->
        <span
          class="group inline-flex cursor-pointer items-center rounded pl-1.5 transition-colors hover:bg-subtle"
          onclick={(ev) => onChipClick(ev, e)}
        >
          <button
            type="button"
            class="rounded text-left text-secondary transition-colors group-hover:text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
            onclick={() => onOpenDetail(e.source, e.projectId)}>{e.name}</button
          >
          <button
            type="button"
            class="btn-icon btn-icon-sm"
            use:tooltip={$t('mods.deps.jumpToTitle', { name: e.name })}
            aria-label={$t('mods.deps.jumpToTitle', { name: e.name })}
            onclick={() =>
              onJump({ source: e.source, project_id: e.projectId, name: e.name, sha1: e.sha1 })}
            ><Icon name="locate" size={15} /></button
          >
        </span>
      {/each}
    </div>
  {/if}
</div>
