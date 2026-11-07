<script lang="ts">
  import { onMount, untrack } from 'svelte';
  interface Props {
    value: number | null | undefined;
    suffix?: string;
    decimals?: number;
    minimumFractionDigits?: number;
    useGrouping?: boolean;
    mode?: 'number' | 'temperature' | 'rpm';
    large?: boolean;
    smallSuffix?: boolean;
    suffixWhenUnknown?: boolean;
  }
  let { value, suffix = '', decimals = 0, minimumFractionDigits = 0, useGrouping = true,
    mode = 'number', large = false, smallSuffix = false, suffixWhenUnknown = false }: Props = $props();
  // Display-only checkpoint: no source values are changed or written back.
  let displayed = $state<number | null>(null);
  let accepted: number | null = null;
  let acceptedMode: Props['mode'];
  let ready = $state(false);
  const digitItems = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  $effect(() => {
    const currentMode = mode;
    const next = typeof value === 'number' && Number.isFinite(value)
      ? currentMode === 'rpm' ? Math.round(value / 10) * 10 : value : null;
    untrack(() => {
      const threshold = currentMode === 'temperature' ? 1 : currentMode === 'rpm' ? 20 : 0;
      if (next === null || accepted === null || acceptedMode !== currentMode || Math.abs(next - accepted) >= threshold) {
        displayed = next;
        accepted = next;
        acceptedMode = currentMode;
      }
    });
  });
  const known = $derived(displayed !== null);
  const formatted = $derived(known ? displayed!.toLocaleString('zh-CN', {
    maximumFractionDigits: decimals, minimumFractionDigits, useGrouping
  }) : '—');
  const chars = $derived(formatted.split(''));
  const visibleSuffix = $derived(known || suffixWhenUnknown ? suffix : '');
  onMount(() => {
    let secondFrame = 0;
    const firstFrame = requestAnimationFrame(() => {
      secondFrame = requestAnimationFrame(() => { ready = true; });
    });
    return () => { cancelAnimationFrame(firstFrame); cancelAnimationFrame(secondFrame); };
  });
</script>

<span class="animated-number" class:large class:unknown={!known} class:ready>
  <span class="sr-only">{known ? formatted : '未知'}{visibleSuffix}</span>
  <span class="visual" aria-hidden="true"><span class="digits">
    {#each chars as char, index (chars.length - index)}
      {#if char >= '0' && char <= '9'}
        <span class="digit-window" data-position={chars.length - index}><span class="digit-strip" style:transform="translateY(-{Number(char) * 1.15}em)">{#each digitItems as digit}<span class="digit">{digit}</span>{/each}</span></span>
      {:else}<span class="character">{char}</span>{/if}
    {/each}
  </span>{#if visibleSuffix}<span class="suffix" class:small={smallSuffix}>{visibleSuffix}</span>{/if}</span>
</span>

<style>
  .animated-number { position:relative; display:inline-flex; align-items:baseline; font-variant-numeric:tabular-nums; line-height:1.15; white-space:nowrap; }
  .large { letter-spacing:-.02em; }
  .visual,.digits { display:inline-flex; align-items:baseline; line-height:inherit; }
  .digit-window { display:inline-block; width:1ch; height:1.15em; overflow:hidden; text-align:center; -webkit-mask-image:linear-gradient(transparent,#000 12%,#000 88%,transparent); mask-image:linear-gradient(transparent,#000 12%,#000 88%,transparent); }
  .digit-strip { display:flex; flex-direction:column; }
  .ready .digit-strip { transition:transform var(--d-smooth) var(--spring-smooth); }
  .digit { display:block; flex:0 0 1.15em; height:1.15em; line-height:1.15em; }
  .character { height:1.15em; line-height:1.15em; }
  .suffix { white-space:pre; }
  .suffix.small { margin-left:2px; font-size:.75em; color:var(--label-2); font-weight:400; }
  .unknown { color:var(--label-3); }
  .sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip-path:inset(50%); white-space:nowrap; border:0; }
</style>
