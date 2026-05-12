<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import NetworkPopover from '$lib/network/NetworkPopover.svelte';

  let name = $state('World');
  let message = $state<string | null>(null);
  let networkOpen = $state(false);

  async function onGreet() {
    const result = await commands.greet(name);
    message = result.message;
  }
</script>

<main class="relative p-8 flex flex-col gap-4 items-start">
  <div class="absolute right-4 top-4">
    <button
      class="text-sm border rounded px-2 py-1 hover:bg-neutral-100"
      onclick={() => (networkOpen = !networkOpen)}
    >
      🌐 Network
    </button>
    <NetworkPopover bind:open={networkOpen} />
  </div>

  <h1 class="text-2xl font-bold">FTlauncher</h1>

  <label class="flex flex-col gap-1">
    <span class="text-sm">Your name</span>
    <input class="border rounded px-2 py-1" bind:value={name} placeholder="World" />
  </label>

  <button class="bg-blue-600 text-white px-3 py-1 rounded hover:bg-blue-700" onclick={onGreet}>
    Greet
  </button>

  {#if message}
    <p class="text-green-700">{message}</p>
  {/if}
</main>
