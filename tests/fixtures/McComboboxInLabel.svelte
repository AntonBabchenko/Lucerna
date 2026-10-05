<script lang="ts">
  // Test fixture: the field as its hosts place it — inside a <label> (the Browse filter bar's "MC",
  // the launcher import's version label), between two other controls, so Tab and Shift+Tab have
  // somewhere to go.
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';

  let value = $state('');
</script>

<button type="button">Previous</button>
<label>
  MC
  <McVersionCombobox bind:value dataTestid="mc" />
</label>
<button type="button">Next</button>
