<script lang="ts">
  import type { Snippet } from 'svelte';
  import Collapse from './Collapse.svelte';

  interface Props {
    title: string;
    children: Snippet;
    open?: boolean;
  }

  let { title, children, open = false }: Props = $props();

  let isOpen = $state(false);
  const contentId = $props.id();

  $effect(() => {
    isOpen = open;
  });
</script>

<button type="button" class="disc" aria-expanded={isOpen} aria-controls={contentId} onclick={() => (isOpen = !isOpen)}>
  <span class="disc-title">{title}</span>
  <svg class="i chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
</button>
<Collapse open={isOpen} id={contentId}>
  {@render children()}
</Collapse>

<style>
  .disc-title { min-width: 0; overflow-wrap: anywhere; }
</style>
