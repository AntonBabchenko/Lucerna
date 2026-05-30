<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import MicrosoftLogo from './microsoft-logo.svelte';

  let {
    signingIn = $bindable(false),
    onSignedIn,
    onError,
  } = $props<{
    signingIn?: boolean;
    onSignedIn?: (account: unknown) => void;
    onError?: (err: unknown) => void;
  }>();

  async function handleClick() {
    if (signingIn) return;
    signingIn = true;
    const result = await commands.beginMicrosoftSignin();
    signingIn = false;
    if (result.status === 'ok') {
      onSignedIn?.(result.data);
    } else {
      onError?.(result.error);
    }
  }
</script>

<button
  type="button"
  data-tour="ms-signin-btn"
  aria-label="Sign in with Microsoft"
  class="ms-signin-btn"
  disabled={signingIn}
  onclick={handleClick}
>
  <MicrosoftLogo />
  <span>Sign in with Microsoft</span>
</button>

<style>
  /*
    Per MS Identity Branding Guidelines: white background with #5E5E5E
    label in light mode; #2F2F2F background with #FFFFFF label in dark.
    1px solid #8C8C8C border in light. 41px recommended height.
    The MS button is deliberately NOT a token-driven .btn-* — it must
    follow Microsoft's exact brand spec across both themes. The four
    logo squares are also fixed-colour, not tokenised.
  */
  .ms-signin-btn {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 41px;
    padding: 0 12px;
    font-family: 'Segoe UI', system-ui, sans-serif;
    font-size: 15px;
    background: #ffffff;
    color: #5e5e5e;
    border: 1px solid #8c8c8c;
    border-radius: 0.25rem;
    cursor: pointer;
  }
  .ms-signin-btn:hover:not(:disabled) {
    background: #f3f3f3;
  }
  .ms-signin-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .ms-signin-btn:focus-visible {
    outline: 2px solid rgb(var(--accent));
    outline-offset: 2px;
  }
  :global(.dark) .ms-signin-btn {
    background: #2f2f2f;
    color: #ffffff;
    border-color: #5e5e5e;
  }
  :global(.dark) .ms-signin-btn:hover:not(:disabled) {
    background: #3a3a3a;
  }
</style>
