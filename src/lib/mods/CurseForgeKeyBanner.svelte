<script lang="ts">
  // Shown in place of the search UI when source = CurseForge and no API
  // key is stored. Modrinth works anonymously; CurseForge requires an
  // API key. The CTA opens Settings → CurseForge so the user can paste
  // their key — see settingsOpen in $lib/settings/state.svelte.

  let { onOpenSettings }: { onOpenSettings: () => void } = $props();
</script>

<div
  class="p-6 bg-warning-bg border border-border-subtle rounded mx-3 my-4 text-sm text-warning-text"
>
  <div class="font-medium mb-1">CurseForge requires an API key</div>
  <p class="text-warning-text">
    Modrinth works anonymously, but CurseForge needs a free API key. It takes about 5 minutes to
    obtain one — get a free account at console.curseforge.com, generate a key, and paste it into
    Settings.
  </p>
  <button type="button" class="btn-warning btn-sm" onclick={onOpenSettings}>
    Open Settings → CurseForge
  </button>
</div>
