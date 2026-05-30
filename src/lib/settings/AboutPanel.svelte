<script lang="ts">
  // About section of Settings — required for Minecraft Usage Guidelines
  // compliance (verbatim disclaimer + identity of who ships this).
  // The disclaimer text lives in `./disclaimer.ts` so the test file and
  // the panel never disagree on the exact string.
  import pkg from '../../../package.json' with { type: 'json' };
  import { DISCLAIMER_TEXT, REPO_URL } from './disclaimer';

  const version = pkg.version;

  function openRepo() {
    // Lazy import keeps the bundle slim and matches the pattern used in
    // CurseForgeKeyForm.svelte / ModDetailDrawer.svelte.
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(REPO_URL));
  }
</script>

<section class="space-y-3 text-sm">
  <h3 class="text-base font-semibold text-primary">About</h3>
  <p class="font-medium text-primary">FTlauncher v{version}</p>
  <p class="text-secondary">{DISCLAIMER_TEXT}</p>
  <p>
    <button
      type="button"
      class="text-accent underline hover:text-accent"
      title={REPO_URL}
      aria-label={`Open FTlauncher repository on GitHub (${REPO_URL})`}
      onclick={openRepo}
    >
      View on GitHub
    </button>
  </p>
  <p class="text-xs text-muted">
    Licensed under GPL-3.0-or-later. The Java runtime and Minecraft files are downloaded from Mojang
    at runtime and are never modified.
  </p>
  <p class="text-xs text-muted">
    Minecraft and Mojang are trademarks of Mojang Synergies AB and Microsoft Corporation. This
    launcher is not affiliated with either.
  </p>
  <p class="text-xs text-muted">
    Microsoft refresh tokens are stored in the OS keyring (Windows Credential Manager) and removed
    when you sign out.
  </p>
</section>
