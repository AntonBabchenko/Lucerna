<script lang="ts">
  // The Java an instance runs on (spec 2026-10-11-custom-java §6). A thin host
  // for `JavaChoiceField`: the need comes from the instance's installed version,
  // the write is `setInstanceJava`, and «Default» follows Settings' row for that
  // need. Mounted in one line, under Manage → Advanced.
  import { t } from '$lib/i18n';
  import { commands, type InstanceWithStatus, type JavaChoice } from '$lib/ipc/bindings';
  import JavaChoiceField, {
    type IpcResult,
    type JavaSavedValue,
  } from '$lib/java/JavaChoiceField.svelte';
  import { defaultOptionFor } from '$lib/java/default-option';

  let {
    instanceId,
    value,
    mcVersion,
    loader,
    loaderVersion,
    ready,
    onChanged,
  }: {
    instanceId: string;
    value: JavaChoice | null;
    /** Reload keys: the Java an instance needs changes with its version and
     *  loader, and is unknown until it is installed. */
    mcVersion: string;
    loader: string;
    loaderVersion: string | null;
    ready: boolean;
    onChanged: (instance: InstanceWithStatus) => void;
  } = $props();

  // By value: Manage replaces its instance list wholesale on every refresh,
  // and a new object with the same values is not a new instance.
  const reloadKey = $derived(JSON.stringify([instanceId, mcVersion, loader, loaderVersion, ready]));

  async function save(choice: JavaChoice | null): Promise<IpcResult<JavaSavedValue>> {
    const r = await commands.setInstanceJava(instanceId, choice);
    if (r.status !== 'ok') return r;
    // Saved, whatever the field shows meanwhile: the list must learn of it.
    onChanged(r.data.instance);
    return { status: 'ok', data: { value: r.data.instance.java, report: r.data.report } };
  }
</script>

<JavaChoiceField
  {reloadKey}
  {value}
  label={$t('instance.manage.java.label')}
  loadRequired={() => commands.instanceJavaRequirement(instanceId)}
  {save}
  defaultOption={defaultOptionFor('instance')}
  lucernaWrites={{ kind: 'bundled' }}
/>
