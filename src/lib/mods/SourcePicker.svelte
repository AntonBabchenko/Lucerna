<script lang="ts">
  import type { ModSource } from '$lib/ipc/bindings';

  let { value = $bindable<ModSource>('modrinth') }: { value: ModSource } = $props();
</script>

<label class="text-sm text-secondary inline-flex items-center gap-1">
  Source:
  <select aria-label="Mod source" class="filter-control" bind:value>
    <option value="modrinth">Modrinth</option>
    <option value="curseforge">CurseForge</option>
  </select>
</label>
