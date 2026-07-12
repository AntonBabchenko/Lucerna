import { render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it } from 'vitest';
import DialogTitle from '$lib/ui/DialogTitle.svelte';

// Locks in DESIGN.md §3's forward rule: new dialog titles default to
// `text-base`; `text-lg` is reserved for full-screen-replacing flows and must
// be opted into explicitly (the legacy text-lg dialogs pass size="lg").
const title = () => createRawSnippet(() => ({ render: () => `<span>Title</span>` }));

describe('DialogTitle', () => {
  it('defaults to text-base', () => {
    const { container } = render(DialogTitle, { props: { children: title() } });
    const cls = (container.querySelector('h3') as HTMLElement).className.split(/\s+/);
    expect(cls).toContain('text-base');
    expect(cls).not.toContain('text-lg');
  });

  it('renders text-lg only when size="lg"', () => {
    const { container } = render(DialogTitle, { props: { children: title(), size: 'lg' } });
    const cls = (container.querySelector('h3') as HTMLElement).className.split(/\s+/);
    expect(cls).toContain('text-lg');
    expect(cls).not.toContain('text-base');
  });
});
