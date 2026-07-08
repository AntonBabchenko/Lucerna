// Shared state + command hub for the instance-picture flow. There is no
// viewer modal: entry points (Overview avatar, Manage modal) call `pick()` to
// open the OS file picker directly, or `requestRemove()` to delete the
// picture. The single <InstanceIconDialog> at the page root registers the
// concrete handlers on mount — it owns the hidden file input, the IPC
// mutations, and the refresh callback. `pick()` invokes the picker
// synchronously so the OS dialog opens within the user activation of the
// originating click; the crop dialog itself opens only after a picked file
// decodes (`show()`).
type IconHandlers = {
  pick: () => void;
  remove: (id: string) => void;
};

class InstanceIconDialogState {
  open = $state(false);
  instanceId = $state<string | null>(null);

  #handlers: IconHandlers | null = null;

  /** Wired by <InstanceIconDialog> on mount; returns an unregister cleanup. */
  registerHandlers(handlers: IconHandlers): () => void {
    this.#handlers = handlers;
    return () => {
      if (this.#handlers === handlers) this.#handlers = null;
    };
  }

  /** Change flow: remember the target and open the OS file picker directly. */
  pick(id: string): void {
    this.instanceId = id;
    this.#handlers?.pick();
  }

  /** Remove the instance's picture — no dialog involved. */
  requestRemove(id: string): void {
    this.#handlers?.remove(id);
  }

  /** Open the crop dialog (after a successful decode). Also the test seam. */
  show(id: string): void {
    this.instanceId = id;
    this.open = true;
  }

  close(): void {
    this.open = false;
    this.instanceId = null;
  }
}

export const iconDialog = new InstanceIconDialogState();
