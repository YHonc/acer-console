<script lang="ts">
  // Compact task controls. Only query runs on mount; mutations require explicit clicks.
  import { onMount } from 'svelte';
  import { api, errorText, type OfficialTaskAction, type OfficialTaskState } from './api';
  import StatusTag from './ui/StatusTag.svelte';
  import { beginWrite, writeBlocked } from './writeResources.svelte';
  let task = $state<OfficialTaskState | null>(null);
  let pending = $state<OfficialTaskAction | null>(null);
  let reading = $state(false);
  let readPromise: Promise<void> | null = null;
  let serial = 0;
  let error = $state('');
  let message = $state('');
  let lastRead = $state('');
  let disposed = false;
  const locked = $derived(writeBlocked('official'));
  const backupMatches = $derived(!!task && !error && task.backup_valid === true && typeof task.backup_original_enabled === 'boolean');

  function run(action: OfficialTaskAction): Promise<void> {
    if (action === 'query' && readPromise) return readPromise;
    if (action !== 'query' && locked) return Promise.resolve();
    if ((action === 'disable' || action === 'restore') && !backupMatches) return Promise.resolve();
    if (action === 'disable' && task?.enabled !== true) return Promise.resolve();
    if (action === 'backup' && (!task || error || backupMatches)) return Promise.resolve();
    const release = action === 'query' ? null : beginWrite('official');
    if (action !== 'query' && !release) return Promise.resolve();
    const request = ++serial;
    if (action === 'query') reading = true; else pending = action;
    error = '';
    message = '';
    const promise = (async () => { try {
      const result = await api.officialTask(action);
      if (disposed || (action === 'query' && (request !== serial || pending !== null))) return;
      if (action !== 'query') ++serial;
      if (!result || typeof result.task_name !== 'string' || typeof result.enabled !== 'boolean' || !/^[a-f\d]{64}$/i.test(result.definition_sha256)) {
        throw new Error('官方启动状态返回不完整，当前状态未知。');
      }
      task = result;
      lastRead = new Date().toLocaleTimeString('zh-CN', { hour12: false });
      if (action === 'backup') {
        message = result.backup_valid === true ? '系统已核验备份与当前任务定义一致。' : '备份请求已返回，但有效性未通过核验，禁用与恢复暂不可用。';
      } else if (action === 'disable') {
        message = result.enabled ? '禁用请求已返回，但任务仍处于启用状态，结果未确认。' : '系统读回：官方界面登录启动已禁用。';
      } else if (action === 'restore') {
        message = result.backup_valid === true && result.enabled === result.backup_original_enabled
          ? `系统读回与备份时的启动状态一致：${result.enabled ? '启用' : '禁用'}。`
          : '恢复请求已返回，但读回与有效备份状态未能匹配，结果未确认。';
      }
    } catch (cause) {
      if (disposed || (action === 'query' && (request !== serial || pending !== null))) return;
      error = errorText(cause);
      task = null;
    } finally { release?.(); if (action === 'query') { reading = false; readPromise = null; } else pending = null; }
    })();
    if (action === 'query') readPromise = promise;
    return promise;
  }
  onMount(() => {
    void run('query');
    return () => { disposed = true; };
  });
</script>

{#snippet busyLabel(active: boolean, busyText: string, idleText: string)}{#if active}<span class="spinner" aria-hidden="true"></span>{busyText}{:else}{idleText}{/if}{/snippet}

<div class="sh"><span>官方控制台联动</span><span><StatusTag text={error ? '状态未知' : task ? (task.enabled ? '已启用' : '已禁用') : reading ? '读取中' : '状态未知'} kind={error ? 'er' : task ? 'ok' : ''}/></span></div>
<div class="group" role="group" aria-label="官方界面登录启动" aria-busy={reading || pending !== null}>
  <div class="row">
    <span class="row-l"><b>官方界面登录启动</b><small aria-live="polite">{error ? '当前状态未知' : task ? task.enabled ? '已启用' : '已禁用' : reading ? '正在读取…' : '当前状态未知'}{#if task} · {task.task_name}{/if}{#if lastRead} · 状态读取 {lastRead}{error ? ' · 已过期' : ''}{/if}</small></span>
    <button type="button" class="btn" aria-busy={reading} onclick={() => run('query')}>{@render busyLabel(reading, '读取中…', '刷新状态')}</button>
  </div>
  <div class="row">
    <span class="row-l"><small>{backupMatches ? '备份有效 · 原状态为' + (task?.backup_original_enabled ? '启用' : '禁用') : '先备份当前任务，再禁用或恢复登录启动。'}</small>{#if task?.backup_error}<small class="er" role="status">{task.backup_error}</small>{/if}</span>
    <button type="button" class="btn" disabled={locked || !task || !!error || backupMatches} onclick={() => run('backup')}>{@render busyLabel(pending === 'backup', '备份中…', '备份启动任务')}</button>
  </div>
  {#if error}<div class="row"><p class="hint er" role="alert">{error}</p></div>{:else if message}<div class="row"><p class="hint" role="status">{message}</p></div>{/if}
  <div class="row foot">
    <span class="hint">仅管理官方界面的登录启动任务，不卸载程序、不停止服务或当前进程。</span>
    <div class="actions">
      <button type="button" class="btn" disabled={locked || !backupMatches || task?.enabled !== true} onclick={() => run('disable')}>{@render busyLabel(pending === 'disable', '正在禁用…', '禁用官方界面登录启动')}</button>
      <button type="button" class="btn" disabled={locked || !backupMatches} onclick={() => run('restore')}>{@render busyLabel(pending === 'restore', '正在恢复…', '恢复备份时的启动状态')}</button>
    </div>
  </div>
</div>

<style>
  p.hint { margin:0; overflow-wrap:anywhere; }
  .er, .row-l small.er { color:var(--red-text, var(--danger)); }
  .row > .btn { flex-shrink:0; }
  .actions { display:flex; flex-wrap:wrap; gap:8px; flex-shrink:0; }
  @media(max-width:620px) { .row.foot { flex-wrap:wrap; } }
</style>
