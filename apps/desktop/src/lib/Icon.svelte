<script lang="ts">
  // Line icons from the approved preview (24px grid); every shape is expressed as a path.
  let { name = '', size = 18, strokeWidth = 1.7 }: { name?: string; size?: number; strokeWidth?: number } = $props();

  const paths: Record<string, string[]> = {
    cube: ['M12 3 3.5 7.5v9L12 21l8.5-4.5v-9L12 3z','M3.5 7.5 12 12l8.5-4.5M12 12v9M7.5 5.5l9 4.5'],
    lamp: ['M3 9h18M5 6h14v3H5zM12 9v4M7 13l-2 4M12 14v4M17 13l2 4'],
    overview: ['M4.5 16a7.5 7.5 0 1 1 15 0', 'M12 16l3.5-4.5', 'M10.8 16a1.2 1.2 0 1 0 2.4 0a1.2 1.2 0 1 0-2.4 0'],
    fan: [
      'M10.2 12a1.8 1.8 0 1 0 3.6 0a1.8 1.8 0 1 0-3.6 0',
      'M12 10.2C11.6 6 12.6 3 15.3 3c2 0 2.8 2 1.8 3.6-1 1.6-2.8 2.7-5.1 3.6M13.8 12c4.2-.4 7.2.6 7.2 3.3 0 2-2 2.8-3.6 1.8-1.6-1-2.7-2.8-3.6-5.1M12 13.8c.4 4.2-.6 7.2-3.3 7.2-2 0-2.8-2-1.8-3.6 1-1.6 2.8-2.7 5.1-3.6M10.2 12C6 12.4 3 11.4 3 8.7c0-2 2-2.8 3.6-1.8 1.6 1 2.7 2.8 3.6 5.1'
    ],
    settings: ['M4 7.5h9M17 7.5h3M4 16.5h3M11 16.5h9', 'M13 7.5a2 2 0 1 0 4 0a2 2 0 1 0-4 0', 'M7 16.5a2 2 0 1 0 4 0a2 2 0 1 0-4 0'],
    plugin: [
      'M6 4h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z',
      'M15 4h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z',
      'M6 13h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2z',
      'M16.5 13.5v6M13.5 16.5h6'
    ],
    key: ['M5.5 6h13A2.5 2.5 0 0 1 21 8.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 15.5v-7A2.5 2.5 0 0 1 5.5 6z', 'M7 10h.01M10.5 10h.01M14 10h.01M17.5 10h.01M8 14h8'],
    refresh: ['M19.5 12a7.5 7.5 0 1 1-2.2-5.3', 'M19.5 4.5v4.2h-4.2'],
    moon: ['M20 14.2A8 8 0 0 1 9.8 4a8 8 0 1 0 10.2 10.2z'],
    sun: ['M8 12a4 4 0 1 0 8 0a4 4 0 1 0-8 0', 'M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4'],
    chev: ['M9.5 6l6 6-6 6'],
    check: ['M6 12.5l4 4 8-9'],
    warn: ['M12 8.5v4.5M12 16.5h.01', 'M10.3 4.2 2.9 17.3A2 2 0 0 0 4.6 20.3h14.8a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z'],
    lock: ['M7 11h10a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2z', 'M8 11V8a4 4 0 0 1 8 0v3'],
    bolt: ['M13 3 5 13.5h6L10 21l8-10.5h-6z'],
    shield: ['M12 3 5 6v5.5c0 4.2 3 7.8 7 9.5 4-1.7 7-5.3 7-9.5V6z', 'M9 12l2 2 4-4'],
    mouse: ['M12 3a5.5 5.5 0 0 1 5.5 5.5v7a5.5 5.5 0 0 1-11 0v-7A5.5 5.5 0 0 1 12 3z', 'M12 7v3'],
    list: ['M8 7h12M8 12h12M8 17h12M4 7h.01M4 12h.01M4 17h.01'],
    x: ['M7 7l10 10M17 7 7 17'],
    sidebar: ['M6.5 5h11a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3h-11a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3z', 'M9.5 5v14'],
    info: ['M12 11v6M12 7h.01', 'M22 12a10 10 0 1 1-20 0a10 10 0 0 1 20 0']
  };

  const shapes = $derived(paths[name] ?? paths.info);
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={strokeWidth} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#each shapes as d}<path {d} />{/each}
</svg>

<style>
  .icon { display: block; flex: none; }
</style>
