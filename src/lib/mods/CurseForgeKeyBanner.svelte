<script lang="ts">
  // Shown in place of the search UI when source = CurseForge and no API
  // key is stored. Modrinth works anonymously; CurseForge requires an
  // API key. The CTA opens Settings → CurseForge so the user can paste
  // their key — see settingsOpen in $lib/settings/state.svelte.

  let { onOpenSettings }: { onOpenSettings: () => void } = $props();
</script>

<div class="p-6 bg-amber-50 border border-amber-200 rounded mx-3 my-4 text-sm text-amber-900">
  <div class="font-medium mb-1">CurseForge requires an API key</div>
  <p class="text-amber-800">
    Modrinth works anonymously, but CurseForge needs a free API key. It takes about 5 minutes to
    obtain one — get a free account at console.curseforge.com, generate a key, and paste it into
    Settings.
  </p>
  <button
    type="button"
    class="mt-3 px-3 py-1 bg-amber-700 hover:bg-amber-800 text-white text-sm rounded"
    onclick={onOpenSettings}
  >
    Open Settings → CurseForge
  </button>
</div>
