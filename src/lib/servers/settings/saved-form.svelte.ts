// The "Saved" confirmation pill idiom shared by the Settings tab's save
// sections (#34): after a successful save the pill shows until any field
// diverges from the saved snapshot. Consumers derive a JSON signature of
// their fields and feed it to sync() from an $effect; markSaved(sig) is
// called on save success. Extracted from the identical copies that lived in
// ServerGeneralSettings.svelte and ServerSettings.svelte.

export class SavedForm {
  saved = $state(false);
  private savedSig: string | null = null;

  markSaved(sig: string): void {
    this.savedSig = sig;
    this.saved = true;
  }

  /** Call from an $effect with the current form signature. */
  sync(sig: string): void {
    if (this.saved && sig !== this.savedSig) this.saved = false;
  }
}
