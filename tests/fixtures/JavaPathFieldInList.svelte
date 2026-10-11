<script lang="ts">
  import JavaPathField from '$lib/instances/JavaPathField.svelte';
  import type { InstanceWithStatus, JavaChoice } from '$lib/ipc/bindings';

  // Mirrors Manage: the field's props are read off `selected`, a value derived
  // from a list the host REPLACES wholesale on every refresh — the same
  // values in a new object. A refresh button stands in for `refreshInstances`.
  type Row = Pick<
    InstanceWithStatus,
    'id' | 'mc_version' | 'loader' | 'loader_version' | 'ready'
  > & { java: JavaChoice | null };

  let instances = $state<Row[]>([
    {
      id: 'a',
      mc_version: '1.21.1',
      loader: 'fabric',
      loader_version: '0.16.5',
      ready: true,
      java: { kind: 'custom', path: '/opt/jdk-21/bin/java' },
    },
  ]);
  const selected = $derived(instances.find((i) => i.id === 'a') ?? instances[0]);

  function refresh() {
    instances = instances.map((i) => ({ ...i }));
  }
</script>

<JavaPathField
  instanceId={selected.id}
  value={selected.java}
  mcVersion={selected.mc_version}
  loader={selected.loader}
  loaderVersion={selected.loader_version}
  ready={selected.ready}
  onChanged={refresh}
/>
<button type="button" onclick={refresh}>host refresh</button>
