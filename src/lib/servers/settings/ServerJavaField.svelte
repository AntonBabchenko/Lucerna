<script lang="ts">
  // The Java a server runs on (spec 2026-10-11-custom-java §6.2). A thin host
  // for `JavaChoiceField`: the need is the server's cached Java major (servers
  // keep no version JSON), the write is `serverSetJava`, and «Default» follows
  // Settings' row for that major. Applies from the next start.
  import { t } from '$lib/i18n';
  import type { JavaChoice, ServerWithStatus } from '$lib/ipc/bindings';
  import JavaChoiceField, {
    type IpcResult,
    type JavaSavedValue,
  } from '$lib/java/JavaChoiceField.svelte';
  import { defaultOptionFor } from '$lib/java/default-option';
  import { serverState } from '$lib/servers/server-state.svelte';

  let { server }: { server: ServerWithStatus } = $props();

  // By value: the server list is replaced on every update.
  const reloadKey = $derived(JSON.stringify([server.id, server.mc_version, server.java_major]));

  async function save(choice: JavaChoice | null): Promise<IpcResult<JavaSavedValue>> {
    const r = await serverState.setJava(server.id, choice);
    if (r.status !== 'ok') return r;
    return { status: 'ok', data: { value: r.data.server.java, report: r.data.report } };
  }
</script>

<JavaChoiceField
  {reloadKey}
  value={server.java}
  label={$t('servers.settings.java')}
  loadRequired={async () => ({ status: 'ok', data: server.java_major })}
  {save}
  defaultOption={defaultOptionFor('server')}
  lucernaWrites={{ kind: 'bundled' }}
  gpuNote={false}
  testid="server-java-field"
/>
