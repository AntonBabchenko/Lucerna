<script lang="ts">
  import type { ModSource, ModVersion } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import type { UnresolvableDetail } from '$lib/mods/unresolvable-detail';
  import { Icon } from '$lib/ui/icons';

  // Modal that runs after ModBrowseView's startInstall surfaces deps the
  // user should look at — required mods auto-install (read-only list),
  // optional mods are checkboxes (unchecked by default), and incompatible
  // / unresolvable rows are warnings. The Install button label updates
  // live with the chosen count so the user knows exactly what will go in.
  //
  // No IPC here. The owning view drives the actual install through
  // modsInstallWithDeps once onConfirm fires with the chosen optionals.
  //
  // Each dep is described by `{ version, projectName, projectSource }`
  // rather than a raw ModVersion — the version's own `name` field on
  // Modrinth is typically the version's release title (e.g. "[NeoForge
  // 1.20.4] 13.3.1") rather than the project name (e.g. "Jade"). The
  // owning view enriches the dep list with the project name via
  // mods_project before opening the dialog so the user sees what they
  // expect.
  //
  // OptionalItem extends DepItem with `requires: DepItem[]` — the
  // optional's own transitive required sub-deps, enriched to names.
  // When the user checks an optional its sub-deps are revealed indented
  // beneath it so they can see what gets pulled in automatically.

  export type DepItem = {
    version: ModVersion;
    projectName: string;
    projectSource: ModSource;
  };

  export type OptionalItem = DepItem & {
    requires: DepItem[];
  };

  let {
    primary,
    primaryProjectName,
    required,
    optional,
    incompatible,
    unresolvable,
    loaderRequirements = [],
    loaderMismatch = null,
    onCancel,
    onConfirm,
  }: {
    primary: ModVersion;
    primaryProjectName: string;
    required: DepItem[];
    optional: OptionalItem[];
    incompatible: string[];
    unresolvable: UnresolvableDetail[];
    // Mod loaders the mod declared as dependencies (NeoForge, Fabric, …).
    // Informational only — loaders are managed at the instance level.
    loaderRequirements?: string[];
    // Set when the picked version's loaders don't include the active
    // instance's loader. Surfaces a red warning row so the user knows
    // installing will leave a mod jar that won't load at runtime.
    loaderMismatch?: { instanceLoader: string; modLoaders: string[] } | null;
    onCancel: () => void;
    onConfirm: (chosenOptional: ModVersion[]) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let optionalChosen = $state<boolean[]>(optional.map(() => false));

  // Deduped count of everything that will be installed:
  //   1 (primary) + required + each checked optional (the optional itself
  //   + its requires), deduped by source:project_id so a shared sub-dep
  //   that appears in multiple optionals (or in required) is counted once.
  const total = $derived.by((): number => {
    const seen = new Set<string>();
    // The primary is always 1; we don't track it in the set because it
    // won't appear in the dep lists.
    let count = 1;
    for (const r of required) {
      const key = `${r.version.source}:${r.version.project_id}`;
      if (!seen.has(key)) {
        seen.add(key);
        count += 1;
      }
    }
    for (let i = 0; i < optional.length; i++) {
      if (!optionalChosen[i]) continue;
      const o = optional[i]!;
      const topKey = `${o.version.source}:${o.version.project_id}`;
      if (!seen.has(topKey)) {
        seen.add(topKey);
        count += 1;
      }
      for (const sub of o.requires) {
        const subKey = `${sub.version.source}:${sub.version.project_id}`;
        if (!seen.has(subKey)) {
          seen.add(subKey);
          count += 1;
        }
      }
    }
    return count;
  });

  function confirm() {
    const chosen = optional.filter((_, i) => optionalChosen[i]).map((d) => d.version);
    onConfirm(chosen);
  }
</script>

<div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
  <div
    role="dialog"
    aria-modal="true"
    class="bg-surface rounded shadow-xl w-[480px] max-w-[90vw] p-5"
  >
    <h2 class="text-base font-semibold text-primary mb-3">
      {$t('mods.depDialog.heading', { name: primaryProjectName, version: primary.version_number })}
    </h2>

    {#if required.length > 0}
      <div class="mb-3">
        <div class="text-xs uppercase tracking-wide text-muted mb-1">
          {$t('mods.depDialog.requiredLabel')}
        </div>
        <ul class="text-sm text-primary list-disc pl-5">
          {#each required as r (r.version.version_id)}
            <li>
              {r.projectName}
              <span class="text-muted">· {r.version.version_number} · {r.projectSource}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if optional.length > 0}
      <div class="mb-3">
        <div class="text-xs uppercase tracking-wide text-muted mb-1">
          {$t('mods.depDialog.optionalLabel')}
        </div>
        <ul class="text-sm text-primary space-y-1">
          {#each optional as o, i (o.version.version_id)}
            <li>
              <label class="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={optionalChosen[i]}
                  onchange={(e) => {
                    optionalChosen[i] = (e.currentTarget as HTMLInputElement).checked;
                  }}
                />
                {o.projectName}
                <span class="text-muted">· {o.version.version_number} · {o.projectSource}</span>
              </label>
              {#if optionalChosen[i] && o.requires.length > 0}
                <ul class="pl-6 mt-0.5 space-y-0.5 text-xs text-muted">
                  {#each o.requires as sub (sub.version.version_id)}
                    <li>{$t('mods.depDialog.subRequires', { name: sub.projectName })}</li>
                  {/each}
                </ul>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if loaderMismatch}
      <div
        class="mb-3 bg-danger-bg border border-danger rounded p-2 text-sm text-danger flex items-start gap-1.5"
      >
        <Icon name="warning" class="text-danger shrink-0 mt-0.5" />
        <div>
          <span class="font-medium">{$t('mods.depDialog.loaderMismatchHeading')}</span>
          {$t('mods.depDialog.loaderMismatchBody', {
            modLoaders: loaderMismatch.modLoaders.join(' / '),
            instanceLoader: loaderMismatch.instanceLoader,
          })}
        </div>
      </div>
    {/if}

    {#if loaderRequirements.length > 0}
      <div
        class="mb-3 bg-accent-soft border border-accent rounded p-2 text-xs text-accent flex items-start gap-1.5"
      >
        <Icon name="info" class="text-accent shrink-0 mt-0.5" />
        <span
          >{$t('mods.depDialog.loaderRequirementsInfo', {
            loaders: loaderRequirements.join(' / '),
          })}</span
        >
      </div>
    {/if}

    {#if incompatible.length > 0}
      <div
        class="mb-3 bg-warning-bg border border-warning-text/30 rounded p-2 text-sm text-warning-text flex items-start gap-1.5"
      >
        <Icon name="warning" size={14} class="flex-shrink-0 mt-0.5" />
        <span>{$t('mods.depDialog.incompatibleWarning', { names: incompatible.join(', ') })}</span>
      </div>
    {/if}

    {#if unresolvable.length > 0}
      <div
        class="mb-3 bg-warning-bg border border-warning-text/30 rounded p-2 text-sm text-warning-text flex items-start gap-1.5"
      >
        <Icon name="warning" size={14} class="flex-shrink-0 mt-0.5" />
        <div>
          <div class="font-medium mb-1">{$t('mods.depDialog.unresolvableHeading')}</div>
          <ul class="list-disc pl-5 space-y-0.5">
            {#each unresolvable as u, i (i)}
              <li>
                {#if u.kind === 'noMc'}
                  {$t('mods.depDialog.unresolvableRowNoMc', {
                    name: u.name,
                    mcVersion: u.mcVersion,
                    list: u.list,
                  })}
                {:else if u.kind === 'wrongLoader'}
                  {$t('mods.depDialog.unresolvableRowWrongLoader', {
                    name: u.name,
                    loader: u.loader,
                    list: u.list,
                  })}
                {:else}
                  {$t('mods.depDialog.unresolvableRowNoVersions', { name: u.name })}
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      </div>
    {/if}

    <div class="flex justify-end gap-2 mt-4">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}
        >{$t('common.cancel')}</button
      >
      <button type="button" class="btn-primary btn-sm" onclick={confirm}>
        {$t('mods.depDialog.installBtn', { count: total })}
      </button>
    </div>
  </div>
</div>
