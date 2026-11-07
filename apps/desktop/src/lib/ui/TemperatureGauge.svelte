<script lang="ts">
  import AnimatedNumber from './AnimatedNumber.svelte';

  interface Props {
    label: string;
    value: number | null;
    subtitle?: string;
    stale?: boolean;
  }

  let { label, value, subtitle, stale = false }: Props = $props();

  const R = 52;
  const C = 2 * Math.PI * R;
  const ARC = C * 0.75;

  const isKnown = $derived(typeof value === 'number' && Number.isFinite(value));
  const fraction = $derived(isKnown ? Math.max(0.02, Math.min(1, value! / 105)) : 0);
  const warm = $derived(isKnown && !stale && value! >= 85 && value! < 95);
  const hot = $derived(isKnown && !stale && value! >= 95);
  const valueMax = $derived(isKnown ? Math.max(105, Math.ceil(value!)) : 105);
</script>

<div
  class="gauge"
  class:stale={stale && isKnown}
  class:empty={!isKnown}
  class:warm
  class:hot
  role={isKnown ? 'meter' : 'group'}
  aria-label={isKnown ? `${label} 温度` : `${label} 温度未知`}
  aria-valuenow={isKnown ? value! : undefined}
  aria-valuemin={isKnown ? 0 : undefined}
  aria-valuemax={isKnown ? valueMax : undefined}
  aria-valuetext={isKnown ? `${value}°C${stale ? '（读数已过期）' : ''}` : undefined}
>
  <svg viewBox="0 0 120 120" aria-hidden="true">
    <circle class="g-track" cx="60" cy="60" r={R} style:stroke-dasharray="{ARC} {C}" />
    <circle class="g-val" cx="60" cy="60" r={R} style:stroke-dasharray="{ARC * fraction} {C}" style:opacity={isKnown ? 1 : 0} />
  </svg>
  <div class="g-center" aria-hidden="true">
    <span class="g-label">{label}</span>
    <span class="g-num">
      {#if isKnown}<AnimatedNumber value={value} mode="temperature" useGrouping={false} large={true} />{:else}<span class="dash">—</span>{/if}<small>°C</small>
    </span>
    <span class="g-sub">{subtitle ?? ''}</span>
  </div>
</div>

<style>
  .dash { font-weight: 200; }
  .g-sub { max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 580px) {
    .gauge { width: 150px; height: 150px; }
    .g-num { font-size: 38px; }
  }
</style>
