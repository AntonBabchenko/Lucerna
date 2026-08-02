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
        'success-bg': 'rgb(var(--success-bg) / <alpha-value>)',
        danger: 'rgb(var(--danger) / <alpha-value>)',
        'danger-bg': 'rgb(var(--danger-bg) / <alpha-value>)',
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
        // Deliberately text-only: bg-danger / border-danger / .btn-danger must
        // keep the saturated colors.danger fill so white labels stay readable.
        danger: 'rgb(var(--danger-text) / <alpha-value>)',
      },
      // Default border color follows the theme token so bare `border`
      // / `border-b` / `border-t` utilities pick up the dark-palette
      // colour instead of Tailwind's hardcoded #e5e7eb preflight
      // default (which produced a stuck light-grey hairline in dark
      // mode across ~60 sites).
      borderColor: {
        DEFAULT: 'rgb(var(--border-subtle) / <alpha-value>)',
      },
    },
  },
  plugins: [],
};
