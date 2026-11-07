<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    children: Snippet;
    id?: string;
  }

  let { open, children, id }: Props = $props();
</script>

<div class="collapse" class:open {id}>
  <div class="inner" inert={!open}>
    {@render children()}
  </div>
</div>
