<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    message: string;
    status?: string;
    onclose: () => void;
  }

  let { message, status, onclose }: Props = $props();

  let pulseTrigger = $state(0);
  let prevKey: string | null = null;

  const isFailure = $derived(status === 'failed' || status === 'failure' || status === 'error');
  const isSuccess = $derived(status === 'applied' || status === 'authorized' || status === 'success');
  const isUnconfirmed = $derived(status === 'unconfirmed');
  const isPending = $derived(status === 'pending' || status === 'busy');
  const tone = $derived(isSuccess ? 'green' : isFailure ? 'pink' : isUnconfirmed ? 'orange' : 'blue');

  $effect(() => {
    const key = `${status}\u0000${message}`;
    if (prevKey !== null && key !== prevKey) pulseTrigger += 1;
    prevKey = key;
  });

  onMount(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onclose();
    }
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  });
</script>

<div class="toast-host top-toast-host">
  <aside class="toast top-toast" role={isFailure ? 'alert' : 'status'} aria-live={isFailure ? 'assertive' : 'polite'}>
    {#key pulseTrigger}
      <span class="sq {tone} toast-icon" aria-hidden="true">
        {#if isPending}
          <span class="spinner"></span>
        {:else if isSuccess}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3.5 8.5 6.5 11.5 12.5 4.5" /></svg>
        {:else if isFailure || isUnconfirmed}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="8" y1="3.5" x2="8" y2="9" /><circle cx="8" cy="12" r=".6" fill="currentColor" /></svg>
        {:else}
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="8" y1="7" x2="8" y2="12" /><circle cx="8" cy="4" r=".6" fill="currentColor" /></svg>
        {/if}
      </span>
      <span class="toast-body">
        {#if isUnconfirmed}<span class="tag draft">未确认</span>{/if}
        <span class="toast-message">{message}</span>
      </span>
    {/key}
    <button type="button" class="toast-close" aria-label="关闭提示" onclick={onclose}>
      <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="4" y1="4" x2="12" y2="12" /><line x1="12" y1="4" x2="4" y2="12" /></svg>
    </button>
  </aside>
</div>

<style>
  .top-toast-host { z-index: 9999; max-width: calc(100vw - 32px); }
  .top-toast { max-width: min(520px, calc(100vw - 32px)); padding-right: 8px; }
  .toast-icon { animation: tagPop var(--d-bouncy) var(--spring-bouncy); }
  .toast-icon svg { width: 13px; height: 13px; }
  .toast-icon .spinner { width: 11px; height: 11px; }
  .toast-body { display: flex; align-items: baseline; flex-wrap: wrap; gap: 6px; min-width: 0; flex: 1; max-height: 40vh; overflow-y: auto; }
  .toast-message { overflow-wrap: anywhere; text-wrap: pretty; animation: fadeIn var(--d-base) var(--ease-out); }
  .toast-close { flex: none; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--label-3); display: grid; place-items: center; transition: background-color var(--d-fast) var(--ease-out), color var(--d-fast); }
  .toast-close:hover { background: var(--fill); color: var(--label); }
</style>
