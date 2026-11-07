<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { getCurrentWindow, type Window as TauriWindow } from '@tauri-apps/api/window';
  import type { UnlistenFn } from '@tauri-apps/api/event';

  // Native titlebar contract is 44px. The bar is transparent over the page; once the large
  // page title scrolls away (`scrolled`) it turns into a frosted toolbar with a small title.
  // Toolbar actions are rendered outside the drag region so they stay clickable.
  let {
    onMaximizedChange,
    title = '',
    scrolled = false,
    actions
  }: {
    onMaximizedChange?: (maximized: boolean) => void;
    title?: string;
    scrolled?: boolean;
    actions?: Snippet;
  } = $props();
  let available = $state(false);
  let maximized = $state(false);
  let error = $state('');
  let appWindow: TauriWindow | undefined;
  let disposed = false;
  let queryVersion = 0;
  let dragRegion: HTMLDivElement;

  function reportError(cause: unknown) {
    if (!disposed) error = `窗口操作失败：${String(cause)}`;
  }

  async function syncMaximized() {
    if (!appWindow) return;
    const version = ++queryVersion;
    try {
      const value = await appWindow.isMaximized();
      if (disposed || version !== queryVersion) return;
      maximized = value;
      onMaximizedChange?.(value);
    } catch (cause) {
      reportError(cause);
    }
  }

  async function act(action: 'minimize' | 'toggleMaximize' | 'close') {
    if (!available || !appWindow) return;
    error = '';
    try {
      // close() preserves the existing close-request / tray / restore policy.
      await appWindow[action]();
      if (action === 'toggleMaximize') await syncMaximized();
    } catch (cause) {
      reportError(cause);
    }
  }


  function handleDragMouseDown(event: MouseEvent) {
    if (!available || !appWindow || event.button !== 0 || (event.detail !== 1 && event.detail !== 2)) return;
    event.preventDefault();
    // Own this gesture locally; stop Tauri's document handler from dispatching it again.
    event.stopImmediatePropagation();
    if (event.detail === 2) void act('toggleMaximize');
    else {
      error = '';
      void appWindow.startDragging().catch(reportError);
    }
  }

  onMount(() => {
    disposed = false;
    if (!isTauri()) return;
    appWindow = getCurrentWindow();
    available = true;
    dragRegion.addEventListener('mousedown', handleDragMouseDown);
    let stopResize: UnlistenFn | undefined;
    void appWindow.onResized(() => { void syncMaximized(); }).then(stop => {
      if (disposed) stop();
      else {
        stopResize = stop;
        void syncMaximized();
      }
    }).catch(reportError);
    void syncMaximized();

    return () => {
      disposed = true;
      available = false;
      dragRegion.removeEventListener('mousedown', handleDragMouseDown);
      ++queryVersion;
      stopResize?.();
    };
  });
</script>

<header class="titlebar" class:maximized class:scrolled data-maximized={maximized} aria-label="应用标题栏">
  <!-- Local mouse handling uses native Window APIs; actions and controls remain outside the drag area. -->
  <div class="drag-region" bind:this={dragRegion} data-tauri-drag-region>
    <span class="tb-title" aria-hidden={!scrolled}>{title}</span>
  </div>
  {#if actions}
    <div class="tb-actions">{@render actions()}</div>
  {/if}
  <div class="window-controls" aria-label="窗口控制">
    <button type="button" disabled={!available} aria-label="最小化" title="最小化" onclick={() => act('minimize')}>
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
    </button>
    <button type="button" disabled={!available} aria-label={maximized ? '向下还原' : '最大化'} title={maximized ? '向下还原' : '最大化'} onclick={() => act('toggleMaximize')}>
      <svg viewBox="0 0 10 10" aria-hidden="true">
        {#if maximized}<path d="M2.5 2.5V.5h7v7h-2M.5 2.5h7v7h-7z" />
        {:else}<rect x=".5" y=".5" width="9" height="9" />{/if}
      </svg>
    </button>
    <button class="close" type="button" disabled={!available} aria-label="关闭窗口" title="关闭窗口" onclick={() => act('close')}>
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0 0 10" /></svg>
    </button>
  </div>
  <span class="window-error" role="status">{error}</span>
</header>

<style>
  .titlebar {
    box-sizing: border-box;
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 44px;
    height: 44px;
    min-height: 44px;
    width: 100%;
    background: transparent;
    color: var(--label);
    user-select: none;
    -webkit-user-select: none;
    transition: background-color var(--d-base, 240ms) var(--ease-out, ease-out), box-shadow var(--d-base, 240ms) var(--ease-out, ease-out);
  }
  .titlebar.scrolled {
    background: var(--toolbar);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    box-shadow: 0 .5px 0 var(--sep);
  }
  .drag-region { display: flex; align-items: center; align-self: stretch; flex: 1; min-width: 0; padding-left: 28px; }
  /* Only the blank/native drag surface receives pointer events; its content stays draggable. */
  .tb-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 600;
    pointer-events: none;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity var(--d-fast, 160ms) var(--ease-out, ease-out), transform var(--d-snappy, 370ms) var(--spring-snappy, ease-out);
  }
  .scrolled .tb-title { opacity: 1; transform: none; }
  .tb-actions { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .window-controls { display: flex; align-self: stretch; flex-shrink: 0; }
  .window-controls button {
    display: grid;
    place-items: center;
    width: 46px;
    height: 100%;
    min-height: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--label-2);
    box-shadow: none;
    transform: none;
    transition: background-color var(--d-fast, 160ms) var(--ease-out, ease-out), color var(--d-fast, 160ms);
  }
  .window-controls button:hover:not(:disabled) { background: var(--fill-2); color: var(--label); }
  .window-controls button.close:hover:not(:disabled) { background: #e81123; color: #fff; }
  .window-controls button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; box-shadow: none !important; border-radius: 0; }
  .window-controls button:disabled { cursor: default; opacity: .38 !important; }
  .window-controls svg { width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1; }
  .window-error { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) {
    .tb-title { transform: none; transition: opacity 140ms linear; }
  }
</style>
