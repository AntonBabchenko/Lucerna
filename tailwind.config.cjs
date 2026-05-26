/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./src/app.html', './src/**/*.{svelte,ts,js}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Surfaces
        base: 'rgb(var(--bg-base) / <alpha-value>)',
        surface: 'rgb(var(--bg-surface) / <alpha-value>)',
        subtle: 'rgb(var(--bg-subtle) / <alpha-value>)',
        muted: 'rgb(var(--bg-muted) / <alpha-value>)',
        // Borders
        'border-subtle': 'rgb(var(--border-subtle) / <alpha-value>)',
        'border-emphasis': 'rgb(var(--border-emphasis) / <alpha-value>)',
        // Accent / state
        accent: 'rgb(var(--accent) / <alpha-value>)',
        'accent-soft': 'rgb(var(--accent-soft) / <alpha-value>)',
        success: 'rgb(var(--success) / <alpha-value>)',
        danger: 'rgb(var(--danger) / <alpha-value>)',
        'warning-bg': 'rgb(var(--warning-bg) / <alpha-value>)',
        'warning-text': 'rgb(var(--warning-text) / <alpha-value>)',
      },
      // Note: textColor overrides Tailwind's colors map for text-*
      // utilities, so `text-muted` here resolves to --text-muted
      // (not colors.muted's --bg-muted). The bare-key naming
      // (primary, secondary, muted, placeholder) is the right
      // shape — using `text-X` as the key would generate the
      // surprising `text-text-X` utility.
      textColor: {
        primary: 'rgb(var(--text-primary) / <alpha-value>)',
        secondary: 'rgb(var(--text-secondary) / <alpha-value>)',
        muted: 'rgb(var(--text-muted) / <alpha-value>)',
        placeholder: 'rgb(var(--text-placeholder) / <alpha-value>)',
      },
    },
  },
  plugins: [],
};
