<script lang="ts">
  import type { ModSource, ModVersion } from '$lib/ipc/bindings';

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

  export type DepItem = {
    version: ModVersion;
    projectName: string;
    projectSource: ModSource;
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
    optional: DepItem[];
    incompatible: string[];
    unresolvable: string[];
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
  const total = $derived(1 + required.length + optionalChosen.filter(Boolean).length);

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
      Install {primaryProjectName}
      {primary.version_number}
    </h2>

    {#if required.length > 0}
      <div class="mb-3">
        <div class="text-xs uppercase tracking-wide text-muted mb-1">
          Required (will be installed)
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
        <div class="text-xs uppercase tracking-wide text-muted mb-1">Optional</div>
        <ul class="text-sm text-primary space-y-1">
          {#each optional as o, i (o.version.version_id)}
            <li>
              <label class="inline-flex items-center gap-2">
                <input type="checkbox" bind:checked={optionalChosen[i]} />
                {o.projectName}
                <span class="text-muted">· {o.version.version_number} · {o.projectSource}</span>
              </label>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if loaderMismatch}
      <div class="mb-3 bg-danger/10 border border-danger rounded p-2 text-sm text-danger">
        ⚠ <span class="font-medium">Loader mismatch.</span> This version targets
        {loaderMismatch.modLoaders.join(' / ')} but your instance uses
        {loaderMismatch.instanceLoader}. Installing will leave a jar in the mods folder that
        Minecraft cannot load. Pick another version or switch your instance's loader.
      </div>
    {/if}

    {#if loaderRequirements.length > 0}
      <div class="mb-3 bg-accent-soft border border-accent rounded p-2 text-xs text-accent">
        ⓘ This mod targets {loaderRequirements.join(' / ')}. Make sure your instance uses a
        compatible loader.
      </div>
    {/if}

    {#if incompatible.length > 0}
      <div
        class="mb-3 bg-warning-bg border border-warning-text/30 rounded p-2 text-sm text-warning-text"
      >
        ⚠ Incompatible with: {incompatible.join(', ')}
      </div>
    {/if}

    {#if unresolvable.length > 0}
      <div
        class="mb-3 bg-warning-bg border border-warning-text/30 rounded p-2 text-sm text-warning-text"
      >
        ⚠ Could not find compatible versions of: {unresolvable.join(', ')} — install anyway?
      </div>
    {/if}

    <div class="flex justify-end gap-2 mt-4">
      <button type="button" class="px-3 py-1 border rounded text-sm" onclick={onCancel}>
        Cancel
      </button>
      <button
        type="button"
        class="px-3 py-1 bg-accent hover:bg-accent text-white rounded text-sm"
        onclick={confirm}
      >
        Install ({total} mod{total === 1 ? '' : 's'})
      </button>
    </div>
  </div>
</div>
