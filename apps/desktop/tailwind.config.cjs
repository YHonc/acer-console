/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./index.html', './src/**/*.{svelte,ts,js}'],
  // Embedded Svelte media queries are CSS, not daisyUI menu state classes.
  blocklist: ['forced-colors:active'],
  corePlugins: { preflight: false },
  theme: { extend: {} },
  plugins: [require('daisyui')],
  daisyui: {
    themes: [{ light: {
      primary: '#2563eb', 'primary-content': '#ffffff',
      secondary: '#64748b', 'secondary-content': '#ffffff',
      accent: '#0284c7', 'accent-content': '#ffffff',
      neutral: '#334155', 'neutral-content': '#f8fafc',
      'base-100': '#ffffff', 'base-200': '#f8fafc', 'base-300': '#f1f5f9', 'base-content': '#0f172a',
      info: '#0284c7', success: '#16a34a', warning: '#d97706', error: '#dc2626'
    } }, 'dark'],
    darkTheme: 'dark', logs: false
  }
};
