<script lang="ts">
  import { tick, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  type Option = { value: string | number; label: string; disabled?: boolean };
  let { value = $bindable<string | number>(''), options, current, unknown = false, disabled = false, id, onchange, children, ...rest }: {
    value?: string | number; options: Option[]; current?: string | number | null; unknown?: boolean; disabled?: boolean; id?: string;
    onchange?: (e: Event & { currentTarget: EventTarget & HTMLSelectElement }) => void; children?: Snippet;
    'aria-label'?: string; 'aria-describedby'?: string; 'aria-invalid'?: boolean;
  } = $props();
  const uid = $props.id();
  let button: HTMLButtonElement; let model: HTMLSelectElement; let menu = $state<HTMLDivElement>();
  let open = $state(false), closing = $state(false), active = $state(0), position = $state('visibility:hidden');
  let place = $state('align'); let typed = '', typedAt = 0; let timer: ReturnType<typeof setTimeout>;
  const PY: Record<string,string> = { 不:'b', 安:'a', 平:'p', 性:'x', 极:'j', 音:'y', 电:'d', 语:'y', 策:'c', 角:'j', 射:'s', 自:'z', 分:'f', 从:'c', 常:'c', 呼:'h', 波:'b', 霓:'n', 涟:'l', 关:'g', 重:'c', 上:'s', 最:'z', 补:'b' };
  const selected = $derived(unknown ? -1 : options.findIndex(o => String(o.value) === String(value)));
  function close(instant = false) {
    if (!open || closing) return;
    button?.focus({preventScroll:true});
    if (instant || matchMedia('(prefers-reduced-motion: reduce)').matches) { open = false; return; }
    closing = true; timer = setTimeout(() => { open = false; closing = false; },150);
  }
  async function show() {
    if (disabled) return;
    clearTimeout(timer); typed='';typedAt=0;closing = false; open = true; active = Math.max(0,selected); position = 'visibility:hidden';
    await tick(); if (!menu) return;
    document.body.append(menu);
    const b = button.getBoundingClientRect(); menu.style.minWidth = b.width + 24 + 'px';
    const opt = menu.querySelectorAll<HTMLElement>('.pop-opt')[active]; if (!opt) return;
    const mh = menu.offsetHeight, mw = menu.offsetWidth;
    let top = b.top + (b.height - opt.offsetHeight)/2 - opt.offsetTop, oy = opt.offsetTop + opt.offsetHeight/2;
    place = 'align'; let max = innerHeight - 16;
    if (top < 8 || top + mh > innerHeight - 8) { top = b.bottom + 4; oy = 0; place = 'below'; max = Math.max(96,innerHeight-top-8); }
    const left = Math.min(Math.max(8,b.left-19),innerWidth-mw-8);
    position = `left:${left}px;top:${top}px;min-width:${b.width+24}px;max-height:${max}px;transform-origin:${b.left+19-left}px ${oy}px`;
    await tick(); menu.focus({preventScroll:true});
  }
  async function choose(index: number) {
    const option = options[index]; if (!option || option.disabled) return;
    value = option.value; await tick();
    model.dispatchEvent(new Event('input',{bubbles:true}));
    model.dispatchEvent(new Event('change',{bubbles:true})); close();
  }
  function key(e: KeyboardEvent) {
    if (['ArrowDown','ArrowUp','Home','End','Enter',' ','Escape','Tab'].includes(e.key)) e.preventDefault();
    if (e.key === 'Escape' || e.key === 'Tab') { close(); return; }
    if (e.key === 'Enter' || e.key === ' ') { void choose(active); return; }
    if (e.key === 'Home') active = 0;
    else if (e.key === 'End') active = options.length-1;
    else if (e.key === 'ArrowDown') active = (active+1)%options.length;
    else if (e.key === 'ArrowUp') active = (active-1+options.length)%options.length;
    else if (e.key.length === 1) {
      typed = performance.now()-typedAt < 700 ? typed+e.key.toLowerCase() : e.key.toLowerCase(); typedAt = performance.now();
      const i = options.findIndex(o => o.label.toLowerCase().startsWith(typed) || [...o.label].map(c => PY[c] || c.toLowerCase()).join('').startsWith(typed));
      if (i >= 0) active = i;
    }
    menu?.querySelectorAll('.pop-opt')[active]?.scrollIntoView({block:'nearest'});
  }
  $effect(() => { if (disabled && open) close(true); });
  onMount(() => {
    const outside = (e: PointerEvent) => { if (open && !menu?.contains(e.target as Node) && !button.contains(e.target as Node)) close(); };
    const scroll = (e: Event) => { if (open && e.target !== menu) close(); };
    document.addEventListener('pointerdown',outside,true); document.addEventListener('scroll',scroll,true);
    return () => { clearTimeout(timer); document.removeEventListener('pointerdown',outside,true); document.removeEventListener('scroll',scroll,true); menu?.remove(); };
  });
</script>

<!-- Hidden native model preserves the original binding and change event contract. -->
<select hidden bind:this={model} bind:value {disabled} onchange={onchange} tabindex="-1" aria-hidden="true">
  {#each options as option}<option value={option.value} disabled={option.disabled}>{option.label}</option>{/each}
</select>
<button bind:this={button} type="button" class="popbtn" class:open={open && !closing} {id} {disabled} {...rest}
  aria-haspopup="listbox" aria-expanded={open && !closing} aria-controls={open ? uid+'-menu' : undefined}
  onclick={() => open ? close() : show()} onkeydown={e => { if (['ArrowDown','ArrowUp',' ','Enter'].includes(e.key)) { e.preventDefault(); void show(); } }}>
  <span class="pop-val">{#each options as option,i}<span class:on={i === selected}>{option.label}</span>{/each}<span class="unk" class:on={selected < 0}>—</span></span>
  <svg class="pop-ch" viewBox="0 0 9 15" aria-hidden="true"><path d="M1.5 5.5 4.5 2.5 7.5 5.5M1.5 9.5 4.5 12.5 7.5 9.5"/></svg>
</button>
{#if open}
  <div bind:this={menu} class="popmenu" class:out={closing} id={uid+'-menu'} role="listbox" tabindex="-1" aria-label={rest['aria-label']} aria-activedescendant={active >= 0 ? uid+'-'+active : undefined} style={position} data-place={place} onkeydown={key} onpointerleave={() => active=-1}>
    {#each options as option,i}
      <div class="pop-opt" class:active={i===active} role="option" tabindex="-1" aria-selected={i===selected} aria-disabled={option.disabled} id={uid+'-'+i}
        onpointermove={() => active=i} onpointerdown={e => { e.preventDefault(); void choose(i); }}>
        <svg class="pop-check" viewBox="0 0 14 14" aria-hidden="true"><path d="M2.5 7.5 5.5 10.5 11.5 3.5"/></svg><span class="pop-t">{option.label}</span>
        {#if current != null && String(current)===String(option.value)}<i class="pop-cur" aria-hidden="true"></i><span class="vh">设备当前值</span>{/if}
      </div>
    {/each}
  </div>
{/if}
