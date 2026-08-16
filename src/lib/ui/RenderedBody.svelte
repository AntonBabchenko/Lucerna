<script lang="ts">
  import { openExternalHttps } from './safe-open';

  // Renders the backend-sanitized description HTML. The HTML is the only
  // {@html} sink in the app and is sanitized in Rust (mods::render) before
  // it ever crosses IPC — see the security note in the spec. Links are
  // intercepted via an action (not an inline handler, which would trip the
  // a11y static-interaction rule) and opened externally so an <a> click
  // can't navigate the Tauri webview away from the SPA.
  //
  // This is the highest-risk remote-content surface in the app: every href
  // here was authored by whoever wrote the Modrinth / CurseForge description.
  // Sanitization does not settle the question — ammonia's default allowlist
  // admits `http` hrefs, so the old `/^https?:/i` test permitted cleartext by
  // construction. `openExternalHttps` is the one place that decides which
  // schemes reach the OS opener, and this handler defers to it.
  let { html }: { html: string } = $props();

  function interceptLinks(node: HTMLElement) {
    function onClick(e: MouseEvent) {
      const anchor = (e.target as HTMLElement).closest('a');
      const href = anchor?.getAttribute('href');
      if (!href) return;
      // Every click is swallowed regardless of scheme — a link the opener
      // refuses must not fall through to the webview and navigate the SPA
      // away either.
      e.preventDefault();
      void openExternalHttps(href);
    }
    node.addEventListener('click', onClick);
    return { destroy: () => node.removeEventListener('click', onClick) };
  }
</script>

<div class="prose-body text-sm text-secondary leading-relaxed selectable" use:interceptLinks>
  {@html html}
</div>
