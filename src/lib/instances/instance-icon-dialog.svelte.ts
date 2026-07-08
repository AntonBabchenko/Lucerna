// Shared, app-level open state for the instance-picture dialog. Both entry
// points (Overview header avatar, Manage Instances modal) call `show(...)`; a
// single <InstanceIconDialog> renders at the page root and reads this.
class InstanceIconDialogState {
  open = $state(false);
  instanceId = $state<string | null>(null);
  hasIcon = $state(false);

  show(id: string, hasIcon: boolean): void {
    this.instanceId = id;
    this.hasIcon = hasIcon;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.instanceId = null;
  }
}

export const iconDialog = new InstanceIconDialogState();
