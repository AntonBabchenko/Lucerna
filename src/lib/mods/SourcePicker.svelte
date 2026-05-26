<script lang="ts">
  import type { ModSource } from '$lib/ipc/bindings';

  let { value = $bindable<ModSource>('modrinth') }: { value: ModSource } = $props();
</script>

<label class="text-sm text-secondary inline-flex items-center gap-1">
  Source:
  <select aria-label="Mod source" class="border rounded px-2 py-1 text-sm bg-surface" bind:value>
    <option value="modrinth">Modrinth</option>
    <option value="curseforge">CurseForge</option>
  </select>
</label>
