<script lang="ts">
  import { onMount, tick } from 'svelte';

  interface LogEntry {
    time: string;
    status: string;
    message: string;
  }

  interface Props {
    log: LogEntry[];
    statusLabel?: (status: string) => string;
  }

  let { log, statusLabel }: Props = $props();

  let isOpen = $state(false);
  let closing = $state(false);
  let anchorEl = $state<HTMLElement>();
  let popoverEl = $state<HTMLElement>();
  let triggerBtnEl = $state<HTMLButtonElement>();
  let logSpace = $state(360);
  let closeTimer: ReturnType<typeof setTimeout> | null = null;

  const reduceMotion = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  function getStatusText(status: string) {
    if (statusLabel) return statusLabel(status);
    const labels: Record<string, string> = { applied: '已应用', authorized: '授权完成', failed: '操作失败', unconfirmed: '未确认', pending: '处理中' };
    return labels[status] ?? status;
  }

  function updateAvailableSpace() {
    if (!anchorEl) return;
    const scrollContainer = anchorEl.closest('main') || document.documentElement;
    const containerRect = scrollContainer.getBoundingClientRect();
    const anchorRect = anchorEl.getBoundingClientRect();
    const space = Math.floor(anchorRect.top - Math.max(0, containerRect.top) - 10 - 16);
    if (space < 80 || anchorRect.bottom > Math.min(window.innerHeight, containerRect.bottom)) {
      if (isOpen) close(false);
      logSpace = 0;
      return;
    }
    logSpace = space;
  }

  function open() {
    if (closeTimer) { clearTimeout(closeTimer); closeTimer = null; }
    closing = false;
    anchorEl?.scrollIntoView({ block: 'nearest' });
    isOpen = true;
    tick().then(updateAvailableSpace);
  }

  function close(restoreFocus: boolean) {
    if (!isOpen || closing) return;
    if (reduceMotion()) {
      isOpen = false;
    } else {
      closing = true;
      closeTimer = setTimeout(() => { isOpen = false; closing = false; closeTimer = null; }, 150);
    }
    if (restoreFocus) triggerBtnEl?.focus();
  }

  function toggle(e: MouseEvent) {
    e.stopPropagation();
    if (isOpen && !closing) close(false); else open();
  }

  onMount(() => {
    function handleDocClick(e: MouseEvent) {
      if (!isOpen) return;
      const target = e.target as Node;
      if (popoverEl && !popoverEl.contains(target) && anchorEl && !anchorEl.contains(target)) close(false);
    }
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && isOpen) close(true);
    }
    function handleScrollOrResize() {
      if (isOpen) updateAvailableSpace();
    }
    window.addEventListener('click', handleDocClick);
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('resize', handleScrollOrResize, { passive: true });
    const scrollContainer = anchorEl?.closest('main');
    scrollContainer?.addEventListener('scroll', handleScrollOrResize, { passive: true });
    return () => {
      if (closeTimer) clearTimeout(closeTimer);
      window.removeEventListener('click', handleDocClick);
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('resize', handleScrollOrResize);
      scrollContainer?.removeEventListener('scroll', handleScrollOrResize);
    };
  });
</script>

<div class="log-anchor" bind:this={anchorEl}>
  <button type="button" bind:this={triggerBtnEl} class="btn" aria-haspopup="dialog" aria-expanded={isOpen && !closing} onclick={toggle}>
    <svg class="i" viewBox="0 0 24 24" style="width:14px;height:14px" aria-hidden="true"><path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01" /></svg>
    操作记录{#if log.length}<span class="tnum">{log.length}</span>{/if}
  </button>

  {#if isOpen}
    <div class="popover log-popover" class:out={closing} bind:this={popoverEl} role="dialog" aria-label="操作记录" style="--log-space: {logSpace}px;">
      <div class="log-head">
        <strong>操作记录</strong>
        <span>共 {log.length} 条</span>
      </div>
      {#if !log.length}
        <div class="log-empty">本次会话尚无操作记录。</div>
      {:else}
        {#each log as entry}
          <div class="log-item">
            <time>{entry.time}</time>
            <span class="log-body">
              <span class="log-badge log-badge-{entry.status}">{getStatusText(entry.status)}</span>
              <span class="log-msg">{entry.message}</span>
            </span>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .log-anchor { position: relative; display: inline-flex; }
  .log-popover {
    bottom: calc(100% + 10px);
    width: min(360px, calc(100vw - 32px));
    max-height: min(320px, 50vh, var(--log-space, 320px));
    padding: 0 0 6px;
  }
  .log-head { position: sticky; top: 0; display: flex; align-items: baseline; justify-content: space-between; padding: 12px 16px 8px; font-size: 12px; color: var(--label-2); background: inherit; }
  .log-head strong { font-size: 13px; font-weight: 600; color: var(--label); }
  .log-body { display: flex; align-items: baseline; gap: 8px; min-width: 0; overflow-wrap: anywhere; }
  .log-badge { flex: none; font-size: 11px; font-weight: 600; color: var(--label-2); }
  .log-badge-applied, .log-badge-authorized { color: var(--green-text); }
  .log-badge-failed { color: var(--red-text); }
  .log-badge-unconfirmed { color: var(--orange-text); }
  .log-badge-pending { color: var(--accent); }
  .log-msg { color: var(--label); line-height: 1.45; }
  .log-empty { padding: 20px 16px 16px; text-align: center; font-size: 12px; color: var(--label-2); }
</style>
