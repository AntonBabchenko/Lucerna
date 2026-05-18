import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

// SvelteKit plugin gives us `$lib/...` alias resolution and `.svelte` file
// compilation for tests. happy-dom is the DOM environment so Svelte
// components can mount; `resolve.conditions: ['browser']` ensures
// @testing-library/svelte v5 picks up the browser entry of svelte/internal.
// svelteTesting() wires up per-test DOM cleanup so renders don't bleed
// across test cases.
export default defineConfig({
  plugins: [sveltekit(), svelteTesting()],
  test: {
    include: ['tests/**/*.test.ts'],
    environment: 'happy-dom',
  },
  resolve: {
    conditions: ['browser'],
  },
});
