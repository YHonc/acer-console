<script lang="ts">
  import { untrack } from 'svelte';
  import Collapse from './Collapse.svelte';
  import Icon from '../Icon.svelte';
  // Observe confirmed readback only; domain-specific draft adoption stays in the caller.
  let { readKey, edited, matches, label, onadopt, note='草稿已保留。', icon=false }: {
    readKey: string | null; edited: boolean; matches: boolean; label: string; onadopt: () => void;
    note?:string; icon?:boolean;
  } = $props();
  let previous: string | null = null;
  let conflict = $state(false);
  $effect(() => {
    const next = readKey;
    if (next !== null && next !== previous) {
      if (previous !== null && untrack(() => edited && !matches)) conflict = true;
      previous = next;
    }
  });
  $effect(() => { if (matches || !edited) conflict = false; });
</script>

<Collapse open={conflict && edited && !matches}>
  <div class="conflict" role="status">
    {#if icon}<Icon name="warn" size={16}/>{/if}
    <span class="row-l"><b>设备已变为 {label}</b><small>{note}</small></span>
    <span class="conflict-actions">
      <button type="button" class="btn" onclick={() => conflict = false}>保留草稿</button>
      <button type="button" class="btn primary" onclick={() => { onadopt(); conflict = false; }}>采用设备值</button>
    </span>
  </div>
</Collapse>
