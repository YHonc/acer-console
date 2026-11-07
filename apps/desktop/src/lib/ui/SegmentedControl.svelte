<script lang="ts">
  import { tick, onMount } from 'svelte';

  interface Option {
    value: string;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value: string;
    options: Option[];
    onchange: (value: string) => void;
    label?: string;
    disabled?: boolean;
    current?: string;
    size?: 'regular' | 'small';
    block?: boolean;
  }

  let { value, options, onchange, label, disabled = false, current, size = 'regular', block = false }: Props = $props();

  let root = $state<HTMLDivElement>();
  let buttonEls: Record<string, HTMLButtonElement | null> = $state({});
  let thumbX = $state(0);
  let thumbW = $state(0);
  let instant = $state(true);
  let disposed = false;

  const selected = $derived(options.find((opt) => opt.value === value));
  const firstEnabledValue = $derived(options.find((opt) => !opt.disabled)?.value ?? null);

  function place(noAnim: boolean) {
    const btn = selected ? buttonEls[selected.value] : null;
    if (!btn || !root?.offsetWidth) return;
    if (noAnim) instant = true;
    thumbW = btn.offsetWidth;
    thumbX = btn.offsetLeft;
    if (noAnim) requestAnimationFrame(() => { if (!disposed) instant = false; });
  }

  $effect(() => {
    void value;
    void options.length;
    tick().then(() => { if (!disposed) place(false); });
  });

  onMount(() => {
    place(true);
    const ro = new ResizeObserver(() => place(true));
    if (root) ro.observe(root);
    return () => {
      disposed = true;
      ro.disconnect();
    };
  });

  function selectOption(opt: Option) {
    if (disabled || opt.disabled || opt.value === value) return;
    onchange(opt.value);
  }

  function isFocusable(opt: Option) {
    if (disabled || opt.disabled) return false;
    if (value && options.some((o) => o.value === value && !o.disabled)) return opt.value === value;
    return opt.value === firstEnabledValue;
  }

  function handleKeyDown(event: KeyboardEvent, currentIndex: number) {
    if (disabled) return;
    const enabled = options.map((opt, idx) => ({ opt, idx })).filter(({ opt }) => !opt.disabled);
    if (!enabled.length) return;
    const pos = enabled.findIndex(({ idx }) => idx === currentIndex);
    let next: Option | undefined;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = enabled[(pos + 1) % enabled.length].opt;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = enabled[(pos - 1 + enabled.length) % enabled.length].opt;
    else if (event.key === 'Home') next = enabled[0].opt;
    else if (event.key === 'End') next = enabled[enabled.length - 1].opt;
    if (!next) return;
    event.preventDefault();
    const target = next;
    selectOption(target);
    tick().then(() => { if (!disposed) buttonEls[target.value]?.focus(); });
  }
</script>

<div
  bind:this={root}
  class="seg"
  class:small={size === 'small'}
  class:block
  class:disabled
  class:no-anim={instant}
  role="radiogroup"
  aria-label={label}
  aria-disabled={disabled || undefined}
>
  <i class="seg-thumb" aria-hidden="true" style:opacity={selected && thumbW ? 1 : 0}
    style:width="{thumbW}px" style:transform="translateX({thumbX}px)"></i>
  {#each options as opt, index (opt.value)}
    {@const isSelected = opt.value === value}
    <button
      type="button"
      bind:this={buttonEls[opt.value]}
      role="radio"
      aria-checked={isSelected}
      tabindex={isFocusable(opt) ? 0 : -1}
      disabled={disabled || !!opt.disabled}
      data-current={current !== undefined && current !== '' && opt.value === current ? '' : undefined}
      title={current !== undefined && opt.value === current && !isSelected ? `${opt.label}（设备当前）` : opt.label}
      onclick={() => selectOption(opt)}
      onkeydown={(e) => handleKeyDown(e, index)}
    >{opt.label}</button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    max-width: 100%;
    padding: 2px;
    border-radius: 9px;
    background: var(--fill);
    user-select: none;
    vertical-align: middle;
  }
  .seg.block { display: grid; width: 100%; }
  .seg-thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 0;
    border-radius: 7px;
    background: var(--thumb);
    box-shadow: var(--shadow-thumb);
    pointer-events: none;
    transition: transform var(--d-snappy) var(--spring-snappy), width var(--d-snappy) var(--spring-snappy), opacity var(--d-fast);
  }
  .seg.no-anim .seg-thumb { transition: none; }
  button {
    position: relative;
    z-index: 1;
    min-width: 0;
    height: 28px;
    padding: 0 14px;
    border: 0;
    border-radius: 7px;
    background: none;
    font-size: 13px;
    font-weight: 500;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transition: opacity var(--d-fast);
  }
  button[aria-checked="false"]:hover:not(:disabled) { opacity: .7; }
  button:disabled { opacity: .32; cursor: default; }
  button[data-current][aria-checked="false"]::after {
    content: "";
    position: absolute;
    left: 50%;
    bottom: 2px;
    width: 4px;
    height: 4px;
    margin-left: -2px;
    border-radius: 50%;
    background: var(--orange);
    animation: fadeIn var(--d-base) var(--ease-out);
  }
  .seg.small button { height: 24px; font-size: 12px; padding: 0 10px; }
  .seg.disabled { opacity: .5; }
  .seg.disabled button:disabled { opacity: 1; }
  @media (forced-colors: active) {
    button[aria-checked="true"] { outline: 1px solid Highlight; }
  }
</style>
