<script lang="ts">
  interface Props {
    checked?: boolean;
    disabled?: boolean;
    pending?: boolean;
    id?: string;
    name?: string;
    value?: string;
    role?: string;
    'aria-label'?: string;
    'aria-labelledby'?: string;
    'aria-describedby'?: string;
    onchange?: (event: Event & { currentTarget: EventTarget & HTMLInputElement }) => void;
  }

  let {
    checked = $bindable(false),
    disabled = false,
    pending = false,
    id,
    name,
    value,
    role = 'switch',
    'aria-label': ariaLabel,
    'aria-labelledby': ariaLabelledby,
    'aria-describedby': ariaDescribedby,
    onchange
  }: Props = $props();

  function handleChange(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    checked = event.currentTarget.checked;
    onchange?.(event);
    if (event.currentTarget.checked !== checked) {
      checked = event.currentTarget.checked;
    }
  }
</script>

<span class="sw" class:pending>
  <input
    type="checkbox"
    {id}
    {name}
    {value}
    {role}
    aria-label={ariaLabel}
    aria-labelledby={ariaLabelledby}
    aria-describedby={ariaDescribedby}
    aria-busy={pending || undefined}
    {checked}
    disabled={disabled || pending}
    onchange={handleChange}
  />
  <span class="sw-knob" aria-hidden="true"></span>
</span>

<style>
  .sw {
    position: relative;
    display: inline-block;
    width: 44px;
    height: 26px;
    border-radius: 13px;
    background: var(--fill-strong);
    flex: none;
    vertical-align: middle;
    transition: background-color var(--d-base) var(--ease-out), opacity var(--d-fast);
  }
  .sw:has(input:checked) { background: var(--green); }
  .sw input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
    z-index: 1;
    appearance: none;
  }
  .sw-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 22px;
    height: 22px;
    border-radius: 11px;
    background: #fff;
    box-shadow: 0 2px 4px rgba(0, 0, 0, .18), 0 0 0 .5px rgba(0, 0, 0, .04);
    pointer-events: none;
    transition: transform var(--d-bouncy) var(--spring-bouncy), width var(--d-snappy) var(--spring-snappy);
  }
  .sw input:checked + .sw-knob { transform: translateX(18px); }
  .sw input:active:not(:disabled) + .sw-knob { width: 28px; }
  .sw input:checked:active:not(:disabled) + .sw-knob { transform: translateX(12px); }
  .sw:has(input:disabled):not(.pending) { opacity: .4; }
  .sw:has(input:disabled) input { cursor: default; }
  .sw:has(input:focus-visible) { box-shadow: 0 0 0 3px var(--focus); }
  .sw.pending { opacity: .75; }
  .sw.pending .sw-knob::after {
    content: "";
    position: absolute;
    inset: 5px;
    border-radius: 50%;
    border: 2px solid rgba(0, 0, 0, .1);
    border-top-color: var(--accent);
    animation: spin .8s linear infinite;
  }
  @media (forced-colors: active) {
    .sw { border: 1px solid CanvasText; }
    .sw:has(input:checked) { background: Highlight; }
    .sw-knob { border: 1px solid CanvasText; background: Canvas; }
  }
</style>
