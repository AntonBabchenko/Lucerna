<script lang="ts">
  // Test fixture: a row that answers a new MC filter in an update of its OWN. Its `$effect` sets
  // `answered`, and only the update that write starts puts the button in front of the combobox —
  // after the keystroke's own update, where the combobox measures first. (The Browse filter bar
  // answers through `$derived` props, inside the keystroke's update; a host is free to answer
  // later.) happy-dom lays nothing out: the test reads the button's presence as the field's new
  // position.
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';

  let value = $state('');
  let answered = $state(false);
  $effect(() => {
    if (value) answered = true;
  });
</script>

{#if answered}
  <button type="button" data-testid="row-answer">Clear all</button>
{/if}
<McVersionCombobox bind:value dataTestid="mc" />
