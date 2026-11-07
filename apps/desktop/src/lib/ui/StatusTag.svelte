<script lang="ts">
  interface Props {
    text: string;
    kind?: 'ok' | 'draft' | 'er' | '';
  }

  let { text, kind = '' }: Props = $props();

  let el = $state<HTMLSpanElement>();
  let last: string | null = null;

  $effect(() => {
    const key = `${kind}\u0000${text}`;
    const node = el;
    if (!node) return;
    if (last !== null && last !== key) {
      node.classList.remove('pop');
      void node.offsetWidth;
      node.classList.add('pop');
    }
    last = key;
  });
</script>

<span bind:this={el} class="tag" class:ok={kind === 'ok'} class:draft={kind === 'draft'} class:er={kind === 'er'}
  onanimationend={() => el?.classList.remove('pop')}>{text}</span>
