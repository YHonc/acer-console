<script lang="ts">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    children: Snippet;
    oncancel: () => void;
  }

  let { title, children, oncancel }: Props = $props();
  let dialog: HTMLDialogElement;
  const titleId = $props.id();

  function trapTab(event: KeyboardEvent) {
    if (event.key !== 'Tab') return;
    const items = Array.from(dialog.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]'))
      .filter(item => item.tabIndex >= 0 && !item.matches(':disabled') && !item.closest('[inert]') && item.getClientRects().length > 0);
    const first = items[0], last = items.at(-1);
    if (!first || !last) { event.preventDefault(); return; }
    if (event.shiftKey && (document.activeElement === first || !dialog.contains(document.activeElement))) {
      event.preventDefault(); last.focus();
    } else if (!event.shiftKey && (document.activeElement === last || !dialog.contains(document.activeElement))) {
      event.preventDefault(); first.focus();
    }
  }

  onMount(() => {
    const previous = document.activeElement;
    dialog.showModal();
    (dialog.querySelector<HTMLElement>('[data-dialog-cancel]') ?? dialog.querySelector('button'))?.focus();
    return () => {
      dialog.close();
      if (previous instanceof HTMLElement && previous.isConnected && !previous.matches(':disabled')) previous.focus();
      else {
        const main = document.querySelector<HTMLElement>('main');
        if (main) {
          const old = main.getAttribute('tabindex');
          main.setAttribute('tabindex', '-1');
          main.focus();
          if (old === null) main.removeAttribute('tabindex'); else main.setAttribute('tabindex', old);
        }
      }
    };
  });
</script>

<dialog
  bind:this={dialog}
  class="sheet confirm-dialog"
  role="alertdialog"
  aria-modal="true"
  aria-labelledby={titleId}
  onkeydown={trapTab}
  oncancel={(event) => { event.preventDefault(); oncancel(); }}
>
  <h2 id={titleId}>{title}</h2>
  {@render children()}
</dialog>

<style>
  .confirm-dialog {
    position: fixed;
    inset: 0;
    margin: auto;
    max-height: calc(100dvh - 32px);
    overflow: auto;
    border: 0;
    text-align: left;
  }
  .confirm-dialog::backdrop { background: var(--scrim); animation: fadeIn 220ms var(--ease-out) both; }
  .confirm-dialog :global(p) { overflow-wrap: anywhere; }
  .confirm-dialog :global(.action-row), .confirm-dialog :global(.actions) {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 18px;
  }
</style>
