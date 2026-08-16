import { render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The chokepoint dynamic-imports this, so mocking the plugin exercises the
// whole chain — RenderedBody -> openExternalHttps -> opener — rather than
// asserting that one module calls another.
const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import RenderedBody from '$lib/ui/RenderedBody.svelte';

describe('RenderedBody', () => {
  beforeEach(() => {
    openUrlMock.mockClear(); // module-level mock accumulates across tests
  });

  it('injects sanitized html', () => {
    const { container } = render(RenderedBody, {
      props: { html: '<h1>Title</h1><p>body</p>' },
    });
    expect(container.querySelector('h1')?.textContent).toBe('Title');
    expect(container.querySelector('p')?.textContent).toBe('body');
  });

  it('renders an empty container for empty html', () => {
    const { container } = render(RenderedBody, { props: { html: '' } });
    expect(container.querySelector('.prose-body')?.textContent).toBe('');
  });

  it('opens an https description link in the system browser', async () => {
    const { container } = render(RenderedBody, {
      props: { html: '<p><a href="https://modrinth.com/mod/sodium">sodium</a></p>' },
    });
    container.querySelector('a')?.click();
    await vi.waitFor(() => expect(openUrlMock).toHaveBeenCalledTimes(1));
    expect(openUrlMock).toHaveBeenCalledWith('https://modrinth.com/mod/sodium');
  });

  // A mod description is third-party text and its hrefs are whatever the
  // author typed; ammonia's default allowlist admits `http`, so cleartext is
  // reachable here by construction. Each case is clicked alongside a known-good
  // https link so the negative assertion is anchored on a positive signal
  // instead of passing vacuously when nothing reaches the opener at all.
  it.each([
    'http://example.com/cleartext',
    'javascript:alert(1)',
    'file:///C:/Windows/System32/calc.exe',
    'mailto:victim@example.com',
    'ms-msdt:/id PCWDiagnostic',
  ])('never hands a %s description link to the opener', async (href) => {
    const { container } = render(RenderedBody, {
      props: {
        html: `<p><a href="${href}">bad</a><a href="https://modrinth.com/ok">good</a></p>`,
      },
    });
    const [bad, good] = Array.from(container.querySelectorAll('a'));
    bad.click();
    good.click();
    await vi.waitFor(() => expect(openUrlMock).toHaveBeenCalledTimes(1));
    expect(openUrlMock).toHaveBeenCalledWith('https://modrinth.com/ok');
  });

  it('swallows the click even for a scheme the opener refuses', () => {
    const { container } = render(RenderedBody, {
      props: { html: '<p><a href="http://example.com/cleartext">x</a></p>' },
    });
    const anchor = container.querySelector('a');
    const event = new MouseEvent('click', { bubbles: true, cancelable: true });
    // `dispatchEvent` returns false once preventDefault has run — a refused
    // link must not fall through and navigate the webview away from the SPA.
    expect(anchor?.dispatchEvent(event)).toBe(false);
  });
});
