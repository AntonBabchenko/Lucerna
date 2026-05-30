<script lang="ts">
  // Renders the backend-sanitized description HTML. The HTML is the only
  // {@html} sink in the app and is sanitized in Rust (mods::render) before
  // it ever crosses IPC — see the security note in the spec. Links are
  // intercepted via an action (not an inline handler, which would trip the
  // a11y static-interaction rule) and opened externally so an <a> click
  // can't navigate the Tauri webview away from the SPA.
  let { html }: { html: string } = $props();

  function interceptLinks(node: HTMLElement) {
    function onClick(e: MouseEvent) {
      const anchor = (e.target as HTMLElement).closest('a');
      const href = anchor?.getAttribute('href');
      if (!href) return;
      e.preventDefault();
      if (/^https?:/i.test(href)) {
        void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(href));
      }
    }
    node.addEventListener('click', onClick);
    return { destroy: () => node.removeEventListener('click', onClick) };
  }
</script>

<div class="prose-body text-sm text-secondary leading-relaxed selectable" use:interceptLinks>
  {@html html}
</div>
