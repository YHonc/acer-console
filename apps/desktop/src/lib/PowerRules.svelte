<script lang="ts">
  import PopupSelect from './ui/PopupSelect.svelte';
  // Primary: saved rule state; secondary: explicit configuration draft and one save action.
  // Compact settings rows using project tokens; no API calls or power-change execution here.
  import type { Profile } from './api';
  import Switch from './ui/Switch.svelte';
  import StatusTag from './ui/StatusTag.svelte';
  import ReadConflict from './ui/ReadConflict.svelte';
  type Config = { enabled: boolean; ac_profile: Profile | null; dc_profile: Profile | null };
  interface Props {
    config: Config;
    status: string;
    acProfiles: Profile[];
    dcProfiles: Profile[];
    busy: boolean;
    onsave: (config: Config) => Promise<void>;
  }
  let { config, status, acProfiles, dcProfiles, busy, onsave }: Props = $props();
  const uid = $props.id();
  const labels: Record<Profile, string> = { quiet: '安静', balanced: '平衡', performance: '性能', turbo: '极速', eco: '节能' };
  let draft = $state<Config>({ enabled: false, ac_profile: null, dc_profile: null });
  let editing = $state(false);
  let saving = $state(false);
  let returned = $state(false);
  let error = $state('');
  const locked = $derived(busy || saving);
  const dirty = $derived(!same(draft, config));
  const invalidAc = $derived(draft.ac_profile !== null && !acProfiles.includes(draft.ac_profile));
  const invalidDc = $derived(draft.dc_profile !== null && !dcProfiles.includes(draft.dc_profile));
  // An unverified historical selection cannot be saved as an active rule.
  const valid = $derived(!draft.enabled || (!invalidAc && !invalidDc));
  function same(a: Config, b: Config) {
    return a.enabled === b.enabled && a.ac_profile === b.ac_profile && a.dc_profile === b.dc_profile;
  }
  function profileLabel(profile: Profile | null) { return profile === null ? '不切换' : labels[profile] ?? profile; }
  $effect(() => { if (!editing) draft = { ...config }; });
  $effect(() => { if (editing && !dirty) editing = false; });

  function changeEnabled(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    if (locked) return;
    editing = true;
    draft.enabled = event.currentTarget.checked;
    returned = false;
    error = '';
  }
  function changeProfile(target: 'ac_profile' | 'dc_profile', event: Event & { currentTarget: EventTarget & HTMLSelectElement }) {
    if (locked || !draft.enabled) return;
    const value = event.currentTarget.value;
    const allowed = target === 'ac_profile' ? acProfiles : dcProfiles;
    if (value && !allowed.includes(value as Profile)) return;
    editing = true;
    draft[target] = value ? value as Profile : null;
    returned = false;
    error = '';
  }
  async function save() {
    if (locked || !dirty || !valid) return;
    saving = true;
    returned = false;
    error = '';
    try {
      await onsave({ ...draft });
      returned = true;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '规则保存失败，请查看操作记录。';
    } finally { saving = false; }
  }
</script>

<div class="sh"><span>电源切换规则</span><span><StatusTag text={saving ? '保存中' : dirty ? '待保存草稿' : '与已保存配置一致'} kind={dirty || saving ? 'draft' : 'ok'}/></span></div>
<div class="group" role="group" aria-label="电源切换规则" aria-busy={saving}>
  <div class="row">
    <span class="row-l" aria-live="polite"><b>当前规则：{config.enabled ? '开启' : '关闭'}</b><small role="status">{status || '规则运行状态未知'}</small></span>
    <span class="row-v">接通电源 · {profileLabel(config.ac_profile)}<br>使用电池 · {profileLabel(config.dc_profile)}</span>
  </div>
  <div class="row">
    <label class="row-l" for={uid + '-enabled'}><b>电源变化时切换模式</b><small id={uid + '-behavior'}>开启并保存后，仅在电源连接状态变化时执行，不立即切换当前模式。</small></label>
    <Switch id={uid + '-enabled'} checked={draft.enabled} disabled={locked} pending={saving} aria-describedby={uid + '-behavior'} onchange={changeEnabled}/>
  </div>
  <div class="row">
    <label class="row-l" for={uid + '-ac'}><b>接通电源</b><small id={uid + '-ac-note'}>{acProfiles.length ? '仅列出已验证模式' : '接电模式尚待验证'}</small>{#if invalidAc}<small class="er">已保存的接电模式不在当前已验证名单，请重新选择。</small>{/if}</label>
    <PopupSelect id={uid + '-ac'} aria-label="接通电源" value={draft.ac_profile ?? ''} disabled={locked || !draft.enabled || !acProfiles.length} aria-describedby={uid + '-ac-note'} aria-invalid={draft.enabled && invalidAc} onchange={event => changeProfile('ac_profile', event)} options={[{value:'',label:'不切换'},...acProfiles.map(profile=>({value:profile,label:profileLabel(profile)}))]}/>
  </div>
  <div class="row">
    <label class="row-l" for={uid + '-dc'}><b>使用电池</b><small id={uid + '-dc-note'}>{dcProfiles.length ? '仅列出已验证模式' : '待电池验证，尚无可用模式'}</small>{#if invalidDc}<small class="er">已保存的电池模式不在当前已验证名单，不能作为已验证规则保存。</small>{/if}</label>
    <PopupSelect id={uid + '-dc'} aria-label="使用电池" value={draft.dc_profile ?? ''} disabled={locked || !draft.enabled || !dcProfiles.length} aria-describedby={uid + '-dc-note'} aria-invalid={draft.enabled && invalidDc} onchange={event => changeProfile('dc_profile', event)} options={[{value:'',label:'不切换'},...dcProfiles.map(profile=>({value:profile,label:profileLabel(profile)}))]}/>
  </div>
  {#if error}<div class="row"><p class="hint er" role="alert">{error}</p></div>{:else if returned}<div class="row"><p class="hint" role="status">{dirty ? '保存请求已返回，等待配置读回。' : '规则配置已读回。'}本次保存不代表硬件模式已切换。</p></div>{/if}
  <ReadConflict readKey={JSON.stringify(config)} edited={editing} matches={!dirty} label={(config.enabled ? '开启' : '关闭') + ' · 接电 ' + profileLabel(config.ac_profile) + ' / 电池 ' + profileLabel(config.dc_profile)} onadopt={() => { draft = { ...config }; editing = false; }}/>
  <div class="row foot">
    <span class="hint" aria-live="polite">{dirty ? '待保存草稿' : '与已保存配置一致'}{dirty ? ' · ' + (draft.enabled ? '开启' : '关闭') + ' / 电源 ' + profileLabel(draft.ac_profile) + ' / 电池 ' + profileLabel(draft.dc_profile) : ' · 本次保存不代表硬件模式已切换。'}</span>
    <button type="button" class="btn primary" disabled={locked || !dirty || !valid} onclick={save}>{#if saving}<span class="spinner" aria-hidden="true"></span>保存中…{:else}保存规则{/if}</button>
  </div>
</div>

<style>
  p.hint { margin:0; overflow-wrap:anywhere; }
  .er, .row-l small.er { color:var(--red-text, var(--danger)); }
  .row > .btn { flex-shrink:0; }
</style>
