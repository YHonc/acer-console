<script lang="ts">
  /* Visual reference: design/apple-redesign-20261105/preview.html (approved 2026-11-05).
     Frosted sidebar with colored nav squares and a spring-sliding highlight, a 44px native titlebar
     that turns into a frosted toolbar once the large title scrolls away, staggered page entry,
     directional sub-page slides and a circular theme reveal.
     All underlying device states, handlers, drafts, APIs, and safety fallbacks are preserved. */
  import { onMount, flushSync, untrack, tick } from 'svelte';
  import { slide } from 'svelte/transition';
  import { cubicIn, quintOut } from 'svelte/easing';
  import { listen } from '@tauri-apps/api/event';
  import { isTauri } from '@tauri-apps/api/core';
  import ConfirmDialog from './lib/ui/ConfirmDialog.svelte';
  import TitleBar from './lib/TitleBar.svelte';
  import Icon from './lib/Icon.svelte';
  import FanControls from './lib/FanControls.svelte';
  import ThermalControls from './lib/ThermalControls.svelte';
  let thermalOwnsFans = $state(false);
  import PowerRules from './lib/PowerRules.svelte';
  import OfficialStartup from './lib/OfficialStartup.svelte';
  import LightingEffects from './lib/LightingEffects.svelte';
  import VendorControls from './lib/VendorControls.svelte';
  import UsbCharge from './lib/UsbCharge.svelte';
  import AudioControls from './lib/AudioControls.svelte';
  import CentralPlugin from './lib/CentralPlugin.svelte';
  import DeviceCenterPeripherals from './lib/DeviceCenterPeripherals.svelte';
  import { dc, connectDc, refreshDc } from './lib/dcState.svelte';
  import { dcApi } from './lib/dcApi';
  import PopupSelect from './lib/ui/PopupSelect.svelte';
  import TemperatureGauge from './lib/ui/TemperatureGauge.svelte';
  import RollingNumber from './lib/ui/RollingNumber.svelte';
  import AnimatedNumber from './lib/ui/AnimatedNumber.svelte';
  import SegmentedControl from './lib/ui/SegmentedControl.svelte';
  import Collapse from './lib/ui/Collapse.svelte';
  import ReadConflict from './lib/ui/ReadConflict.svelte';
  import { beginWrite, anyWriting, isWriting, writeBlocked } from './lib/writeResources.svelte';
  import StatusTag from './lib/ui/StatusTag.svelte';
  import { installOverlayScrollbars, muteScrollbar } from './lib/ui/overlayScrollbar';
  import TopToast from './lib/ui/TopToast.svelte';
  import LogPopover from './lib/ui/LogPopover.svelte';
  import Switch from './lib/ui/Switch.svelte';
  import { sideLayout, setSideLayout, saveSideLayout, applySideLayout, isSideDefault, SIDE_DEFAULTS, SIDE_LIMITS } from './lib/sideLayout.svelte';
  import {
    api,
    errorText,
    type Capability,
    type Capabilities,
    type Snapshot,
    type Settings,
    type StartupState,
    type RulesState,
    type RulesConfig,
    type Profile,
    type LightingAnimation,
    type OperationResult
  } from './lib/api';

  type Page = 'overview' | 'fan' | 'settings' | 'plugin' | 'acer' | 'dc';
  type SettingsTab = 'light' | 'aux' | 'app';
  const pages: { id: Page; label: string; description: string; icon: string; tone: string }[] = [
    { id: 'overview', label: '设备总览', description: '查看硬件状态与常用设置。', icon: 'overview', tone: 'blue' },
    { id: 'fan', label: '性能与风扇', description: '选择性能模式，调整双风扇控制。', icon: 'fan', tone: 'orange' },
    { id: 'settings', label: '设备设置', description: '灯光、电源与控制台偏好。', icon: 'settings', tone: 'gray' },
    { id: 'plugin', label: '服务插件', description: '管理句柄保护，查看日志与资源用量。', icon: 'plugin', tone: 'green' },
  ];
  const settingsOrder: SettingsTab[] = ['light', 'aux', 'app'];

  const profileNames: Record<Profile, string> = {
    quiet: '安静',
    balanced: '平衡',
    performance: '性能',
    turbo: '极速',
    eco: '节能'
  };

  const profileDescriptions: Record<Profile, string> = {
    quiet: '轻负载与安静环境',
    balanced: '日常使用，兼顾性能与噪声',
    performance: '为持续负载提供稳定性能',
    turbo: '设备允许的最高性能模式',
    eco: '降低设备功耗'
  };

  let page = $state<Page>('overview');
  // `page` drives navigation immediately; `shownPage` swaps after the leave animation.
  let shownPage = $state<Page>('overview');
  let pageLeaving = $state(false);
  let leaveTimer: ReturnType<typeof setTimeout> | undefined;
  let isWindowMaximized = $state(false);
  let settingsTab = $state<SettingsTab>('light');
  let settingsDirection = $state(1);
  onMount(() => {
    let disposed = false; let stop: (() => void) | undefined;
    void connectDc().then(cleanup => { if (disposed) cleanup(); else stop=cleanup; });
    return () => { disposed=true; stop?.(); };
  });
  $effect(() => { if (dc.module?.state === 'absent' && page === 'dc') go('acer'); });
  let mainElement = $state<HTMLElement>();
  let titleScrolled = $state(false);
  let capsOpen = $state(false);
  let diagOpen = $state(false);
  let themeSegment = $state<HTMLElement>();

  const pageIndex = $derived(page === 'acer' ? 4 : page === 'dc' ? 5 : pages.findIndex(p => p.id === page));
  const pageInfo = (id: Page) => id === 'acer' ? {label:'Acer 服务保护',description:'Central 句柄修补、Handle Guard 看门狗与日志。'} : id === 'dc' ? {label:'Device Center 外设',description:'鼠标、键盘与屏幕挂灯。'} : pages.find(p=>p.id===id)!;

  /* 侧栏：按钮或 Ctrl+B 切换并记住；窗口窄于 900px 时临时收起（不改记住的偏好）。 */
  const SIDE_KEY = '{{SECRET_ptuqvxuu}}';
  const NARROW = 900;
  function savedSide(): 'expanded' | 'collapsed' {
    try { return localStorage.getItem(SIDE_KEY) === 'collapsed' ? 'collapsed' : 'expanded'; } catch { return 'expanded'; }
  }
  let sidePref = $state<'expanded' | 'collapsed'>(savedSide());
  let narrow = $state(typeof window !== 'undefined' && window.innerWidth < NARROW);
  let sideOverride = $state<'expanded' | 'collapsed' | null>(null);
  const sideCollapsed = $derived((sideOverride ?? (narrow ? 'collapsed' : sidePref)) === 'collapsed');
  $effect(() => {
    document.documentElement.dataset.side = sideCollapsed ? 'collapsed' : 'expanded';
    untrack(hideSideTip);
  });
  /* 导航高亮：按当前项的实际位置滑动，不依赖固定行高。 */
  let navEl = $state<HTMLElement>();
  let navHl = $state({ y: 0, h: 0 });
  function placeNavHl() {
    const on = navEl?.querySelector<HTMLElement>('button.on');
    navHl = on ? { y: on.offsetTop, h: on.offsetHeight } : { y: navHl.y, h: 0 };
  }
  $effect(() => { void page; void dc.module?.state; void tick().then(placeNavHl); });
  $effect(() => {
    if (!navEl) return;
    const observer = new ResizeObserver(placeNavHl);
    observer.observe(navEl);
    return () => observer.disconnect();
  });

  /* 侧栏宽度与边距：设置页可调，也可拖动侧栏右边缘（双击恢复默认宽度）。 */
  applySideLayout();
  let sideResizing = $state(false);
  function startSideResize(event: PointerEvent) {
    if (event.button !== 0 || sideCollapsed) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX, startW = sideLayout.width;
    sideResizing = true;
    const move = (e: PointerEvent) => setSideLayout({ width: startW + e.clientX - startX }, false);
    const end = () => {
      sideResizing = false; saveSideLayout();
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  }
  function sideResizeKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 16 : 4;
    const next = event.key === 'ArrowLeft' ? sideLayout.width - step : event.key === 'ArrowRight' ? sideLayout.width + step
      : event.key === 'Home' ? SIDE_LIMITS.width[0] : event.key === 'End' ? SIDE_LIMITS.width[1] : null;
    if (next == null) return;
    event.preventDefault();
    setSideLayout({ width: next });
  }

  function toggleSide() {
    const next = sideCollapsed ? 'expanded' : 'collapsed';
    if (narrow) { sideOverride = next; return; }
    sidePref = next;
    try { localStorage.setItem(SIDE_KEY, next); } catch { /* 存储不可用时只在本次生效 */ }
  }

  /* 收起时的悬停提示（展开时只给切换按钮提示）。 */
  let sideTip = $state<{ text: string; x: number; y: number } | null>(null);
  let sideTipTimer = 0;
  let sideTipWarm = 0;
  function showSideTip(event: Event, text: string, always = false) {
    const target = event.currentTarget as HTMLElement;
    clearTimeout(sideTipTimer);
    if (!sideCollapsed && !always) { sideTip = null; return; }
    const quick = event.type === 'focus' || performance.now() - sideTipWarm < 600;
    sideTipTimer = window.setTimeout(() => {
      const r = target.getBoundingClientRect();
      sideTip = { text, x: r.right + 10, y: r.top + r.height / 2 };
      sideTipWarm = performance.now();
    }, quick ? 0 : 380);
  }
  function hideSideTip() {
    clearTimeout(sideTipTimer);
    if (sideTip) sideTipWarm = performance.now();
    sideTip = null;
  }

  function reduceMotion() {
    return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  function go(target: Page, tab?: SettingsTab) {
    if (target === 'plugin') target = 'acer';
    if (tab) {
      if (target === page && page === 'settings') selectSettingsTab(tab);
      else settingsTab = tab;
    }
    if (target === page) return;
    page = target;
    clearTimeout(leaveTimer);
    const swap = () => {
      pageLeaving = false;
      shownPage = page;
      titleScrolled = false;
      if (mainElement) muteScrollbar(mainElement);
      mainElement?.scrollTo({ top: 0, behavior: 'instant' });
    };
    if (reduceMotion()) { swap(); return; }
    pageLeaving = true;
    leaveTimer = setTimeout(swap, 130);
  }

  function selectSettingsTab(target: SettingsTab) {
    if (target === settingsTab) return;
    settingsDirection = settingsOrder.indexOf(target) > settingsOrder.indexOf(settingsTab) ? 1 : -1;
    settingsTab = target;
  }

  // Entry stagger: each direct child gets an index; CSS turns it into a 26ms step.
  function stagger(node: HTMLElement) {
    Array.from(node.children).forEach((child, index) => {
      (child as HTMLElement).style.setProperty('--i', String(Math.min(index, 10)));
    });
  }

  // Once the large title scrolls under the 44px titlebar, the toolbar becomes frosted.
  function watchTitle(node: HTMLElement) {
    const root = node.closest('main');
    if (!root || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([entry]) => {
      titleScrolled = !entry.isIntersecting;
    }, { root, rootMargin: '-44px 0px 0px 0px' });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  function bannerIn(node: Element) {
    return slide(node, { duration: reduceMotion() ? 0 : 420, easing: quintOut });
  }

  function bannerOut(node: Element) {
    return slide(node, { duration: reduceMotion() ? 0 : 200, easing: cubicIn });
  }

  function fanFraction(percent: number | null | undefined) {
    return percent != null && Number.isFinite(percent) ? Math.min(1, Math.max(0, percent / 100)) : 0;
  }

  let snapshot = $state<Snapshot | null>(null);
  let capabilities = $state<Capabilities | null>(null);
  let settings = $state<Settings | null>(null);
  let settingsDraft = $state<Settings>({
    theme: 'system',
    poll_interval_ms: 2000,
    close_to_tray: false,
    restore_on_exit: false
  });
  let settingsThemeTouched = $state(false);
  let systemDark = $state(typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  let selectedProfile = $state<Profile | null>(null);
  let profileDirty = $state(false);
  let batteryDraft = $state<boolean | null>(null);
  let batteryEdited = $state(false);
  $effect(() => {
    const current = snapshot?.battery_care;
    if (typeof current === 'boolean' && (!batteryEdited || batteryDraft === current)) {
      batteryDraft = current; batteryEdited = false;
    }
  });
  let loading = $state(true);
  let refreshing = $state(false);
  const performanceLocked = $derived(writeBlocked('performance'));
  let readError = $state('');
  let capabilityError = $state('');
  let settingsError = $state('');
  let startup = $state<StartupState | null>(null);
  let startupError = $state('');
  let rules = $state<RulesState | null>(null);
  let rulesError = $state('');
  let vendorRefreshToken = $state(0);
  let exitRestoreError = $state('');
  let dcExitError = $state(false);
  let dcExitMessage = $state('');
  function receiveExitError(message: string) {
    if (dc.module?.keep && /detach|Device ?Center|模块|后台/i.test(message)) { dcExitError=true;dcExitMessage=''; }
    else exitRestoreError=message;
  }
  async function stopModuleAndExit() {
    const release=beginWrite('exit');if(!release)return;
    try { await dcApi.keep(false);await api.exitWithoutRestore();dcExitError=false; }
    catch(e) {dcExitMessage=errorText(e);}
    finally {release();}
  }
  let exitActionError = $state('');

  let feedback = $state<{ status: string; message: string } | null>(null);
  $effect(() => {
    if (feedback?.status !== 'applied' && feedback?.status !== 'authorized') return;
    const current = feedback;
    const timer = setTimeout(() => {
      if (feedback === current) feedback = null;
    }, 5000);
    return () => clearTimeout(timer);
  });

  let log = $state<{ time: string; message: string; status: string }[]>([]);
  let lastRead = $state('');
  let disposed = false;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let startupReadVersion = 0;
  let snapshotReadVersion = 0;
  let rulesReadVersion = 0;
  let settingsWriteVersion = 0;
  let refreshPromise: Promise<void> | null = null;

  const currentPage = $derived(pageInfo(page));
  const isSyncing = $derived(refreshing || loading);
  const settingsDirty = $derived(!!settings && JSON.stringify(settings) !== JSON.stringify(settingsDraft));
  const theme = $derived(settingsDraft.theme);
  const resolvedTheme = $derived(theme === 'system' ? (systemDark ? 'dark' : 'light') : theme);

  const brokerDisconnected = $derived(
    /broker|\bpipe\b|管道|transport|读取组件.*(?:退出|断开|超时)/i.test(
      [readError, capabilityError, ...(snapshot?.errors ?? [])].filter(Boolean).join(' '),
    ),
  );

  const snapshotStale = $derived(!!readError || brokerDisconnected);

  const batteryAllowed = $derived(
    capabilities?.battery_control?.supported === true &&
    typeof snapshot?.battery_care === 'boolean' &&
    !snapshotStale
  );

  const permissionNeeded = $derived(
    brokerDisconnected ||
    capabilities?.telemetry.status === 'permission_blocked' ||
    /拒绝访问|访问被拒绝|权限不足|权限受阻|未授权|access.?denied|permission.?denied|0x80041003|0x80070005/i.test(
      [readError, capabilityError, capabilities?.telemetry.reason, ...(snapshot?.errors ?? [])].filter(Boolean).join(' '),
    ),
  );

  const connTone = $derived(!!snapshot && !snapshotStale && !permissionNeeded ? 'ok' : loading ? '' : permissionNeeded ? 'wn' : 'er');
  const connText = $derived(loading ? '正在读取…' : permissionNeeded ? '设备待授权' : snapshot && !snapshotStale ? '硬件已连接' : '连接不可用');

  const deviceControlsReady = $derived(!!snapshot && !snapshotStale && !permissionNeeded && !capabilityError);

  const profileAllowed = $derived(
    !!selectedProfile &&
    deviceControlsReady &&
    !!capabilities?.profile_control.supported &&
    capabilities.profiles.includes(selectedProfile)
  );

  const fanAllowed = $derived(deviceControlsReady && !!capabilities?.fan_control.supported);

  const lightingAllowed = $derived(
    capabilities?.lighting_control?.supported === true &&
    !!snapshot &&
    !snapshotStale &&
    !permissionNeeded
  );

  const dynamicLightingAllowed = $derived(
    capabilities?.lighting_dynamic_control?.supported === true &&
    !!snapshot &&
    !snapshotStale &&
    !permissionNeeded
  );

  const lightingRestoreAllowed = $derived(
    capabilities?.lighting_restore_control?.supported === true &&
    !!snapshot &&
    !snapshotStale &&
    !permissionNeeded
  );

  const vendorAuthorized = $derived(
    capabilities?.telemetry?.supported === true &&
    !!snapshot &&
    !snapshotStale &&
    !permissionNeeded
  );

  $effect(() => {
    document.documentElement.dataset.theme = resolvedTheme;
    if (isTauri()) {
      void api.previewTheme(theme).catch(error =>
        addLog('failed', '窗口主题同步失败：' + errorText(error))
      );
    }
  });

  function addLog(status: string, message: string) {
    log = [
      { time: new Date().toLocaleTimeString('zh-CN', { hour12: false }), status, message },
      ...log
    ].slice(0, 20);
  }

  function statusLabel(status: string) {
    const labels: Record<string, string> = {
      applied: '已应用',
      authorized: '授权完成',
      failed: '操作失败',
      unconfirmed: '未确认',
      pending: '处理中'
    };
    return labels[status] ?? status;
  }

  function value(n: number | null | undefined, unit: string) {
    return n != null && Number.isFinite(n) ? `${Math.round(n)}${unit}` : '—';
  }

  function capabilityState(capability?: Capability) {
    if (!capability) return '未知';
    if (capability.status) {
      return ({
        verified: '已验收',
        implemented_unverified: '已实现待实测',
        protocol_pending: '协议待逆向',
        permission_blocked: '权限受阻',
        hardware_unsupported: '硬件明确不支持'
      })[capability.status] ?? '未知';
    }
    return capability.supported ? '可用' : '暂未开放';
  }

  function timestamp(t: string) {
    const date = new Date(/^\d+$/.test(t) ? Number(t) : t);
    return Number.isNaN(date.getTime()) ? t : date.toLocaleTimeString('zh-CN', { hour12: false });
  }

  function resolveTheme(value: Settings['theme']) {
    return value === 'system' ? (systemDark ? 'dark' : 'light') : value;
  }

  // Circular reveal from the control that triggered the change; plain switch otherwise.
  function setThemeDraft(value: Settings['theme'], origin?: { x: number; y: number }) {
    const commit = () => {
      settingsThemeTouched = true;
      settingsDraft.theme = value;
    };
    const doc = document as Document & { startViewTransition?: (update: () => void) => { ready: Promise<void> } };
    if (!origin || !doc.startViewTransition || reduceMotion() || resolveTheme(value) === resolvedTheme) {
      commit();
      return;
    }
    const transition = doc.startViewTransition(() => {
      flushSync(commit);
      document.documentElement.dataset.theme = resolveTheme(value);
    });
    transition.ready.then(() => {
      const radius = Math.hypot(Math.max(origin.x, innerWidth - origin.x), Math.max(origin.y, innerHeight - origin.y));
      document.documentElement.animate(
        { clipPath: [`circle(0px at ${origin.x}px ${origin.y}px)`, `circle(${radius}px at ${origin.x}px ${origin.y}px)`] },
        { duration: 640, easing: 'cubic-bezier(.22,1,.36,1)', pseudoElement: '::view-transition-new(root)' }
      );
    }).catch(() => {});
  }

  function centerOf(element: Element | null | undefined) {
    if (!element) return undefined;
    const rect = element.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  }

  function changeTheme(value: string) {
    if (value !== 'system' && value !== 'light' && value !== 'dark') return;
    const index = (['system', 'light', 'dark'] as const).indexOf(value);
    setThemeDraft(value, centerOf(themeSegment?.querySelectorAll('button')[index] ?? themeSegment));
  }

  function toggleThemeQuick(event: MouseEvent & { currentTarget: EventTarget & HTMLButtonElement }) {
    const nextTheme = resolvedTheme === 'dark' ? 'light' : 'dark';
    setThemeDraft(nextTheme, centerOf(event.currentTarget));
  }

  function manualRefresh() {
    if (page === 'dc') return refreshDc();
    return refresh(true, true);
  }

  async function readSnapshot() {
    const version = ++snapshotReadVersion;
    try {
      const result = await api.getSnapshot();
      if (disposed || version !== snapshotReadVersion) return;
      snapshot = result;
      if (!profileDirty || selectedProfile === result.profile) {
        selectedProfile = result.profile;
        profileDirty = false;
      }
      readError = '';
      lastRead = timestamp(result.timestamp);
    } catch (error) {
      if (!disposed && version === snapshotReadVersion) readError = errorText(error);
    }
  }

  function selectProfile(profile: Profile) {
    if (isWriting('performance') || thermalOwnsFans || !deviceControlsReady || !capabilities?.profile_control.supported || !capabilities.profiles.includes(profile)) return;
    selectedProfile = profile;
    profileDirty = profile !== snapshot?.profile;
  }

  async function applyProfile() {
    if (!profileAllowed || thermalOwnsFans) return;
    await apply('performance', '应用性能模式', () => api.setProfile(selectedProfile!));
  }

  async function readCapabilities() {
    try {
      const result = await api.getCapabilities();
      if (!disposed) {
        capabilities = result;
        capabilityError = '';
      }
    } catch (error) {
      if (!disposed) capabilityError = errorText(error);
    }
  }

  async function readStartup(afterWrite = false) {
    const version = ++startupReadVersion;
    try {
      const result = await api.getStartup();
      if (!disposed && version === startupReadVersion && (!isWriting('startup') || afterWrite)) {
        startup = result;
        startupError = '';
      }
    } catch (error) {
      if (!disposed && version === startupReadVersion && (!isWriting('startup') || afterWrite)) startupError = errorText(error);
    }
  }

  async function readRules(afterWrite = false): Promise<boolean> {
    if (isWriting('rules') && !afterWrite) return true;
    const version = ++rulesReadVersion;
    try {
      const result = await api.getRules();
      if (
        !result?.config ||
        typeof result.config.enabled !== 'boolean' ||
        !(result.config.ac_profile === null || typeof result.config.ac_profile === 'string') ||
        !(result.config.dc_profile === null || typeof result.config.dc_profile === 'string') ||
        !Array.isArray(result.ac_profiles) ||
        !Array.isArray(result.dc_profiles)
      ) {
        throw new Error('原生接口未提供完整规则配置及已验证模式名单，规则编辑暂不可用。');
      }
      if (disposed || version !== rulesReadVersion) return false;
      rules = result;
      rulesError = '';
      return true;
    } catch (error) {
      if (!disposed && version === rulesReadVersion) rulesError = errorText(error);
      return false;
    }
  }

  async function saveRules(config: RulesConfig) {
    if (!rules || rulesError) throw new Error('规则配置尚未就绪，请刷新后重试。');
    const release = beginWrite('rules');
    if (!release) throw new Error('规则正在保存，请稍后重试。');
    feedback = { status: 'pending', message: '正在保存规则配置并读取确认…' };
    let saved = false;
    try {
      await api.saveRules(config);
      saved = true;
      if (!await readRules(true)) throw new Error('保存请求已返回，但配置读回失败：' + rulesError);
      const actual = rules.config;
      if (
        actual.enabled !== config.enabled ||
        actual.ac_profile !== config.ac_profile ||
        actual.dc_profile !== config.dc_profile
      ) {
        throw new Error('规则配置读回与请求不一致，请查看当前已保存配置。');
      }
      feedback = { status: 'applied', message: '规则配置已保存，仅在后续电源状态变化时执行。' };
      addLog('applied', feedback.message);
    } catch (error) {
      feedback = { status: saved ? 'unconfirmed' : 'failed', message: errorText(error) };
      addLog(feedback.status, feedback.message);
      throw error;
    } finally {
      release();
    }
  }

  async function setStartup(enabled: boolean) {
    if (!startup || startupError) return;
    const release = beginWrite('startup');
    if (!release) return;
    ++startupReadVersion;
    feedback = { status: 'pending', message: '正在更新登录启动项并读取系统状态…' };
    try {
      const result = await api.setStartup(enabled);
      if (disposed) return;
      ++startupReadVersion;
      startup = result;
      startupError = '';
      const confirmed = result.enabled === enabled && !result.mismatch && result.entry_present === enabled;
      feedback = {
        status: confirmed ? 'applied' : 'unconfirmed',
        message: confirmed
          ? `系统启动项已${enabled ? '启用' : '关闭'}。实际登录启动尚待重新登录验收。`
          : '系统读回状态与请求不一致，请查看登录启动项状态。',
      };
      addLog(feedback.status, feedback.message);
    } catch (error) {
      if (disposed) return;
      feedback = { status: 'failed', message: `登录启动设置失败：${errorText(error)}` };
      addLog('failed', feedback.message);
      await readStartup(true);
    } finally {
      release();
    }
  }

  function requestStartup(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    const requested = event.currentTarget.checked;
    event.currentTarget.checked = startup?.enabled ?? false;
    void setStartup(requested);
  }

  async function authorizeDevice() {
    if (!permissionNeeded && !exitRestoreError) return;
    const release = beginWrite('auth');
    if (!release) return;
    feedback = { status: 'pending', message: '等待 Windows 授权。请在系统提示中确认；取消后可手动重试。' };
    try {
      await api.authorizeDevice();
      await Promise.all([readCapabilities(), readSnapshot()]);
      if (disposed) return;
      vendorRefreshToken += 1;
      const incomplete = !!readError || !!capabilityError || permissionNeeded;
      feedback = {
        status: incomplete ? 'unconfirmed' : 'authorized',
        message: incomplete
          ? '授权流程已完成，但设备读取仍未恢复。请查看读取错误或权限状态。'
          : '设备已连接，可用控制项已更新。',
      };
      addLog(feedback.status, feedback.message);
    } catch (error) {
      if (disposed) return;
      feedback = { status: 'failed', message: `授权未完成：${errorText(error)}` };
      addLog('failed', feedback.message);
    } finally {
      release();
    }
  }

  async function exitWithoutRestore() {
    if (!exitRestoreError) return;
    const release = beginWrite('exit');
    if (!release) return;
    exitActionError = '';
    try {
      await api.exitWithoutRestore();
    } catch (error) {
      exitActionError = '退出请求失败：' + errorText(error);
      addLog('failed', exitActionError);
    } finally {
      release();
    }
  }

  async function readExitStatus() {
    try {
      const result = await api.getExitStatus();
      if (!disposed && typeof result.error === 'string' && result.error && !exitRestoreError) {
        receiveExitError(result.error);
        addLog('failed', '退出恢复失败：' + result.error);
      }
    } catch (error) {
      if (!disposed) addLog('failed', '退出恢复状态读取失败：' + errorText(error));
    }
  }

  function refresh(all = false, refreshVendor = false): Promise<void> {
    if (refreshPromise) return refreshPromise;
    refreshing = true;
    if (refreshVendor) vendorRefreshToken += 1;
    const settingsVersion = settingsWriteVersion;
    refreshPromise = (async () => { try {
      await Promise.all([
        readSnapshot(),
        ...(all
          ? [
              readCapabilities(),
              readStartup(),
              readRules(),
              api
                .getSettings()
                .then(result => {
                  if (disposed || settingsVersion !== settingsWriteVersion || isWriting('settings')) return;
                  const hadDraft = settingsDirty;
                  const preservedTheme = settingsThemeTouched ? settingsDraft.theme : null;
                  settings = result;
                  if (!hadDraft) {
                    settingsDraft = preservedTheme === null ? { ...result } : { ...result, theme: preservedTheme };
                  }
                  settingsThemeTouched = preservedTheme !== null && preservedTheme !== result.theme;
                  settingsError = '';
                })
                .catch(error => {
                  if (disposed || settingsVersion !== settingsWriteVersion || isWriting('settings')) return;
                  settingsError = errorText(error);
                })
            ]
          : []),
      ]);
    } finally {
      refreshing = false;
      loading = false;
      refreshPromise = null;
    }
    })();
    return refreshPromise;
  }

  function schedulePoll() {
    clearTimeout(pollTimer);
    if (disposed) return;
    pollTimer = setTimeout(async () => {
      if (!document.hidden && !anyWriting() && !refreshing) await refresh();
      schedulePoll();
    }, Math.max(1000, settings?.poll_interval_ms ?? 2000));
  }

  async function apply(resource: 'performance' | 'lighting', label: string, action: () => Promise<OperationResult>, rethrow = false) {
    const release = beginWrite(resource);
    if (!release) {
      if (rethrow) throw new Error('另一项操作正在进行，请稍后重试。');
      return;
    }
    feedback = { status: 'pending', message: `正在${label}，等待设备确认…` };
    try {
      const result = await action();
      if (rethrow && result.status === 'failed') {
        await readSnapshot();
        throw new Error(result.message || '设备设置失败。');
      }
      feedback = {
        status: result.status,
        message:
          result.message ||
          ({
            applied: '设备已确认应用。',
            failed: '应用失败。',
            unconfirmed: '请求已发送，尚未得到设备确认。'
          }[result.status] ?? '状态已更新。')
      };
      addLog(result.status, `${label}：${feedback.message}`);
      await readSnapshot();
    } catch (error) {
      feedback = { status: 'failed', message: errorText(error) };
      addLog('failed', `${label}：${feedback.message}`);
      if (rethrow) throw error;
    } finally {
      release();
    }
  }

  async function applyFan(request: { mode: 'auto' | 'max' } | { mode: 'custom'; cpuPercent: number; gpuPercent: number }) {
    if (!fanAllowed || thermalOwnsFans) {
      throw new Error(
        permissionNeeded
          ? '设备读取权限未确认，不能调整风扇。'
          : snapshotStale || !snapshot
            ? '设备状态未确认，不能调整风扇。'
            : capabilityError || '风扇控制尚未开放。'
      );
    }
    if (request.mode === 'custom') {
      const range = capabilities?.fan_percent_range;
      if (!capabilities?.fan_custom_control?.supported || !range || !Number.isFinite(range.min) || !Number.isFinite(range.max)) {
        throw new Error('自定义风扇能力或百分比范围尚未提供。');
      }
      await apply('performance', '应用自定义风扇', () => api.setFanSpeeds(request.cpuPercent, request.gpuPercent), true);
    } else {
      await apply('performance', '应用风扇模式', () => api.setFanMode(request.mode), true);
    }
  }

  async function applyLighting(request: { zones: string[]; brightness: number }) {
    if (!lightingAllowed) throw new Error('四区静态灯光能力或设备状态尚未就绪。');
    await apply('lighting', '应用四区静态灯光', () => api.setLighting({
      enabled: request.brightness > 0,
      brightness: request.brightness,
      zones: request.zones.map(color => color.toLowerCase())
    }), true);
    await readCapabilities();
  }

  async function applyLightingAnimation(request: LightingAnimation) {
    if (!dynamicLightingAllowed) throw new Error('动态灯效能力或设备状态尚未就绪。');
    await apply('lighting', '应用动态灯效', () => api.setLightingAnimation({
      ...request,
      color: request.color.toLowerCase()
    }), true);
    await readCapabilities();
  }

  async function restoreLighting() {
    if (!lightingRestoreAllowed) throw new Error('当前没有可用的灯光恢复状态。');
    await apply('lighting', '恢复上一步灯光', () => api.restoreLighting(), true);
    await readCapabilities();
  }

  async function requestBatteryCare(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
    const enabled = event.currentTarget.checked;
    event.currentTarget.checked = snapshot?.battery_care ?? false;
    if (!batteryAllowed) return;
    const release = beginWrite('battery');
    if (!release) return;
    batteryDraft = enabled;
    batteryEdited = true;
    feedback = { status: 'pending', message: '正在请求电池保护设置，等待设备读回…' };
    try {
      const result = await api.setBatteryCare(enabled);
      await readSnapshot();
      const confirmed = result.status === 'applied' && !snapshotStale && snapshot?.battery_care === enabled;
      feedback = {
        status: confirmed ? 'applied' : 'unconfirmed',
        message: confirmed
          ? `电池保护状态已读回：${enabled ? '开启' : '关闭'}。实际充电行为尚待实机验证。`
          : `${result.message || '未确认电池保护设置。'} 请以当前设备读回为准。`,
      };
      addLog(feedback.status, feedback.message);
    } catch (error) {
      await readSnapshot();
      feedback = { status: 'unconfirmed', message: `电池保护请求失败，设置结果未确认：${errorText(error)}` };
      addLog('unconfirmed', feedback.message);
    } finally {
      release();
    }
  }

  async function saveSettings() {
    if (!settings || !settingsDirty) return;
    const release = beginWrite('settings');
    if (!release) return;
    ++settingsWriteVersion;
    const submitted = { ...settingsDraft };
    feedback = { status: 'pending', message: '正在保存设置…' };
    try {
      const result = await api.saveSettings(submitted);
      ++settingsWriteVersion;
      settings = result;
      // Keep edits made after the request (including the immediate theme preview).
      for (const key of Object.keys(submitted) as (keyof Settings)[]) {
        if (settingsDraft[key] === submitted[key]) Object.assign(settingsDraft, { [key]: result[key] });
      }
      settingsThemeTouched = settingsDraft.theme !== result.theme;
      feedback = { status: 'applied', message: '设置已保存。' };
      addLog('applied', '设置已保存。');
      schedulePoll();
    } catch (error) {
      feedback = { status: 'failed', message: errorText(error) };
      addLog('failed', `保存设置：${feedback.message}`);
    } finally {
      release();
    }
  }

  onMount(() => {
    const removeScrollbars = installOverlayScrollbars();
    const onSideKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key?.toLowerCase() === 'b') {
        event.preventDefault();
        toggleSide();
      }
    };
    const onSideResize = () => {
      const next = window.innerWidth < NARROW;
      if (next !== narrow) { narrow = next; sideOverride = null; }
    };
    window.addEventListener('keydown', onSideKey);
    window.addEventListener('resize', onSideResize, { passive: true });
    let unlisten: (() => void) | undefined;
    let unlistenExit: (() => void) | undefined;
    if (isTauri()) {
      void listen('device-refresh', () => {
        if (!document.hidden) void refresh(true);
      }).then(stop => {
        if (disposed) stop();
        else unlisten = stop;
      }).catch(error => addLog('failed', '设备事件订阅失败：' + errorText(error)));
    }
    if (isTauri()) {
      void listen<string>('exit-restore-error', event => {
        if (disposed) return;
        receiveExitError(event.payload || '风扇恢复未完成，应用未退出。');
        exitActionError = '';
        addLog('failed', '退出恢复失败：' + exitRestoreError);
      }).then(stop => {
        if (disposed) { stop(); return; }
        unlistenExit = stop;
        void readExitStatus();
      }).catch(error => addLog('failed', '退出恢复事件订阅失败：' + errorText(error)));
    }
    void refresh(true).then(schedulePoll);
    const systemTheme = window.matchMedia('(prefers-color-scheme: dark)');
    const systemThemeChanged = () => { systemDark = systemTheme.matches; };
    systemTheme.addEventListener('change', systemThemeChanged);
    const visible = () => { if (!document.hidden) void refresh(); };
    document.addEventListener('visibilitychange', visible);
    return () => {
      disposed = true;
      clearTimeout(pollTimer);
      clearTimeout(leaveTimer);
      unlisten?.();
      unlistenExit?.();
      document.removeEventListener('visibilitychange', visible);
      systemTheme.removeEventListener('change', systemThemeChanged);
      window.removeEventListener('keydown', onSideKey);
      window.removeEventListener('resize', onSideResize);
      clearTimeout(sideTipTimer);
      removeScrollbars();
    };
  });
</script>
{#snippet toolbarActions()}
  <span
    class="chip"
    class:ok={!!snapshot && !snapshotStale && !permissionNeeded}
    class:er={brokerDisconnected || (!snapshot && !!readError)}
    class:wn={permissionNeeded || (!!snapshot && snapshotStale)}
  >
    <i class="dot"></i>
    <span>
      {loading && !snapshot
        ? '正在连接'
        : permissionNeeded
          ? '待授权'
          : brokerDisconnected
            ? '已断线'
            : snapshot && snapshotStale
              ? '读数过期'
              : snapshot
                ? '已连接'
                : '未连接'}
    </span>
  </span>
  <span class="chip">
    <Icon name="bolt" size={13} />
    <span>{snapshot?.ac_power === true ? '已接电' : snapshot?.ac_power === false ? '电池供电' : loading ? '正在检测电源…' : '电源未知'}</span>
  </span>
  {#if snapshot?.battery_percent != null && Number.isFinite(snapshot.battery_percent)}
    <span class="chip">电量 <AnimatedNumber value={Math.round(snapshot.battery_percent)} useGrouping={false} />%</span>
  {/if}
  <button
    type="button"
    class="btn icon"
    onclick={toggleThemeQuick}
    aria-label="切换浅色/深色主题"
    title={resolvedTheme === 'dark' ? '切换为浅色外观' : '切换为深色外观'}
  >
    <Icon name={resolvedTheme === 'dark' ? 'sun' : 'moon'} />
  </button>
  <button type="button" class="btn refresh-btn" onclick={manualRefresh} aria-busy={isSyncing} aria-label="刷新设备状态">
    <span class="refresh-ico">{#if isSyncing}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="refresh" size={15} />{/if}</span>
    <span class="label-swap">
      <span class="swap-text" class:shown={!isSyncing} aria-hidden={isSyncing}>刷新</span>
      <span class="swap-text" class:shown={isSyncing} aria-hidden={!isSyncing}>同步中</span>
    </span>
  </button>
{/snippet}

{#snippet chevron()}<span class="chev"><Icon name="chev" size={14} /></span>{/snippet}

<a class="skip" href="#content">跳转到主要内容</a>

<div class="app" class:window-maximized={isWindowMaximized} class:side-resizing={sideResizing}>
  <!-- Blank sidebar surface also drags the frameless window; its buttons do not. -->
  <aside class="side" id="sidebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="brand-mark" aria-hidden="true"><Icon name="bolt" size={16} /></span>
      <div class="brand-text">
        <b>Acer Console</b>
        <small>Predator 控制台</small>
      </div>
      <button
        type="button"
        class="side-toggle"
        aria-controls="sidebar"
        aria-expanded={!sideCollapsed}
        aria-label={sideCollapsed ? '展开侧栏' : '收起侧栏'}
        onclick={() => { toggleSide(); hideSideTip(); }}
        onpointerenter={e => showSideTip(e, (sideCollapsed ? '展开侧栏' : '收起侧栏') + ' · Ctrl+B', true)}
        onpointerleave={hideSideTip}
        onfocus={e => showSideTip(e, (sideCollapsed ? '展开侧栏' : '收起侧栏') + ' · Ctrl+B', true)}
        onblur={hideSideTip}
      ><Icon name="sidebar" size={17} /></button>
    </div>

    <nav class="nav" bind:this={navEl} aria-label="主导航">
      <i class="nav-hl" class:shown={navHl.h > 0} aria-hidden="true" style="transform: translateY({navHl.y}px); height: {navHl.h || 36}px"></i>
      {#each pages.filter(p => p.id !== 'plugin') as item}
        <button
          type="button"
          class:on={page === item.id}
          aria-current={page === item.id ? 'page' : undefined}
          onclick={() => go(item.id)}
          onpointerenter={e => showSideTip(e, item.label)}
          onpointerleave={hideSideTip}
          onfocus={e => showSideTip(e, item.label)}
          onblur={hideSideTip}
        >
          <span class="sq {item.tone}"><Icon name={item.icon} size={15} strokeWidth={2} /></span>
          <span class="nav-label">{item.label}</span>
        </button>
      {/each}
      <div class="nav-sec" id="nav-sec-plugins"><span>服务插件</span></div>
      {#each [{id:'acer' as const,label:'Acer 服务保护',icon:'shield',tone:'teal'}, {id:'dc' as const,label:'Device Center 外设',icon:'mouse',tone:'indigo'}] as item}
        <div class="nav-sub-wrap" class:absent={item.id === 'dc' && dc.module?.state === 'absent'} inert={item.id === 'dc' && dc.module?.state === 'absent'}>
          <button type="button" class="nav-sub" class:on={page===item.id} aria-current={page===item.id?'page':undefined} aria-describedby="nav-sec-plugins"
            onclick={()=>go(item.id)} onpointerenter={e=>showSideTip(e,item.label)} onpointerleave={hideSideTip} onfocus={e=>showSideTip(e,item.label)} onblur={hideSideTip}>
            <span class="sq {item.tone}"><Icon name={item.icon} size={15} strokeWidth={2}/></span><span class="nav-label">{item.label}</span>
          </button>
        </div>
      {/each}
    </nav>

    <div class="side-foot">
      <div class="foot-full">
        <i class="dot" class:ok={connTone === 'ok'} class:wn={connTone === 'wn'} class:er={connTone === 'er'}></i>
        <span class="foot-text">
          <b>{capabilities?.model || '等待设备连接'}</b>
          <span>{capabilities?.bios_version ? `BIOS ${capabilities.bios_version}` : 'BIOS 版本未知'} · {connText}</span>
        </span>
      </div>
      <!-- 收起时只留状态点；文字由悬停提示给出 -->
      <span
        class="foot-mini conn"
        class:ok={connTone === 'ok'} class:wn={connTone === 'wn'} class:er={connTone === 'er'}
        role="img"
        aria-label={(capabilities?.model || '等待设备连接') + ' · ' + connText}
        onpointerenter={e => showSideTip(e, (capabilities?.model || '等待设备连接') + ' · ' + connText)}
        onpointerleave={hideSideTip}
      ><i class="dot"></i></span>
    </div>
    {#if !sideCollapsed}
      <!-- 可聚焦的 separator 是 ARIA 窗口分隔条模式（方向键调整宽度），svelte 将其视为非交互元素 -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        class="side-resize"
        role="separator"
        tabindex="0"
        aria-orientation="vertical"
        aria-controls="sidebar"
        aria-label="调整侧栏宽度，双击恢复默认"
        aria-valuenow={sideLayout.width}
        aria-valuemin={SIDE_LIMITS.width[0]}
        aria-valuemax={SIDE_LIMITS.width[1]}
        onpointerdown={startSideResize}
        onkeydown={sideResizeKey}
        ondblclick={() => setSideLayout({ width: SIDE_DEFAULTS.width })}
      ></div>
    {/if}
  </aside>
  {#if sideTip}
    <div class="side-tip" role="tooltip" style="left: {sideTip.x}px; top: {sideTip.y}px;">{sideTip.text}</div>
  {/if}

  <div class="ws">
    <div class="tb-slot">
      <TitleBar
        title={currentPage.label}
        scrolled={titleScrolled}
        actions={toolbarActions}
        onMaximizedChange={(val) => { isWindowMaximized = val; }}
      />
    </div>

    <main id="content" class="main" tabindex="-1" data-sb-top="54" bind:this={mainElement}>
      <div class="content">
        {#key shownPage}
          <header class="head stage" class:leaving={pageLeaving} use:stagger>
            <h1 use:watchTitle>{pageInfo(shownPage).label}</h1>
            <p>{pageInfo(shownPage).description}</p>
          </header>
        {/key}

        <!-- Important notices expand in height and stay above the page body. -->
        {#if exitRestoreError}
          <div class="banner-wrap" in:bannerIn out:bannerOut>
            <div class="banner" role="alert">
              <span class="sq pink"><Icon name="warn" /></span>
              <div class="grow">
                <b>退出前风扇恢复未完成</b>
                <p>{exitRestoreError}</p>
                <p>重新授权后，可从托盘再次选择退出以重试恢复；也可明确选择跳过本次恢复并退出。</p>
                {#if exitActionError}<p class="err-text">{exitActionError}</p>{/if}
              </div>
              <div class="banner-actions">
                <button type="button" class="btn" disabled={writeBlocked('auth')} onclick={authorizeDevice}>
                  {#if isWriting('auth')}<span class="spinner" aria-hidden="true"></span>等待授权…{:else}重新授权设备访问{/if}
                </button>
                <button type="button" class="btn danger" disabled={writeBlocked('exit')} onclick={exitWithoutRestore}>
                  {#if isWriting('exit')}<span class="spinner" aria-hidden="true"></span>正在退出…{:else}跳过恢复并退出{/if}
                </button>
              </div>
            </div>
          </div>
        {/if}

        {#if readError || capabilityError || brokerDisconnected}
          <div class="banner-wrap" in:bannerIn out:bannerOut>
            <div class="banner" role="status">
              <span class="sq orange"><Icon name="warn" /></span>
              <div class="grow">
                <b>{snapshot ? '设备状态读取失败' : '尚未读取到设备状态'}</b>
                <p>{readError || capabilityError || '硬件读取连接已中断，请在概览或设置中重新授权。'}</p>
                {#if snapshot}<p>下方为上次读数，不代表当前设备状态。</p>{/if}
              </div>
            </div>
          </div>
        {/if}

        {#if permissionNeeded}
          <div class="banner-wrap" in:bannerIn out:bannerOut>
            <div class="banner" role="alert">
              <span class="sq orange"><Icon name="lock" /></span>
              <div class="grow">
                <b>{brokerDisconnected ? '设备读取连接已中断' : '需要授权设备访问'}</b>
                <p>连接设备以查看状态，并使用已验证的控制功能。此次授权不会修改硬件设置，取消后不会自动重试。</p>
              </div>
              <button type="button" class="btn primary" disabled={writeBlocked('auth')} onclick={authorizeDevice}>
                {#if isWriting('auth')}<span class="spinner" aria-hidden="true"></span>等待授权…{:else}{brokerDisconnected ? '重新授权设备访问' : '授权设备访问'}{/if}
              </button>
            </div>
          </div>
        {/if}

        {#if feedback}
          <TopToast message={feedback.message} status={feedback.status} onclose={() => (feedback = null)} />
        {/if}

        {#key shownPage}
          <div class="stage" class:leaving={pageLeaving} use:stagger>
            {#if shownPage === 'overview'}
              <div class="group hero">
                <div class="gauge-cell">
                  <TemperatureGauge
                    label="CPU"
                    value={snapshot?.cpu_temperature ?? null}
                    stale={!!snapshot && snapshotStale}
                    subtitle={snapshot?.cpu_temperature == null ? '等待读取温度' : snapshotStale ? '上次设备读数' : '当前设备温度'}
                  />
                  <div class="g-meta">风扇 <b><RollingNumber value={snapshot?.cpu_fan_rpm ?? null} suffix=" RPM" /></b></div>
                </div>
                <div class="hero-sep" aria-hidden="true"></div>
                <div class="gauge-cell">
                  <TemperatureGauge
                    label="GPU"
                    value={snapshot?.gpu_temperature ?? null}
                    stale={!!snapshot && snapshotStale}
                    subtitle={snapshot?.gpu_temperature == null ? '等待读取温度' : snapshotStale ? '上次设备读数' : '当前设备温度'}
                  />
                  <div class="g-meta">风扇 <b><RollingNumber value={snapshot?.gpu_fan_rpm ?? null} suffix=" RPM" /></b></div>
                </div>
              </div>

              <div class="sh"><span>运行状态</span><span>{snapshot && snapshotStale ? '上次读数 · 已过期' : snapshot ? '设备读回' : '等待读取'}</span></div>
              <div class="group">
                <button type="button" class="row" onclick={() => go('fan')}>
                  <span class="sq orange"><Icon name="bolt" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>性能模式</b></span>
                  <span class="row-v">{snapshot?.profile ? profileNames[snapshot.profile] : '—'}</span>
                  {@render chevron()}
                </button>
                <button type="button" class="row" onclick={() => go('fan')}>
                  <span class="sq blue"><Icon name="fan" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>散热模式</b></span>
                  <span class="row-v">{snapshot?.fan_mode === 'auto' ? '自动调节' : snapshot?.fan_mode === 'max' ? '最高转速' : snapshot?.fan_mode === 'custom' ? '自定义转速' : '—'}</span>
                  {@render chevron()}
                </button>
                <button type="button" class="row" onclick={() => go('settings', 'light')}>
                  <span class="sq pink"><Icon name="key" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>键盘灯光</b></span>
                  <span class="row-v">{snapshot?.lighting ? `亮度 ${snapshot.lighting.brightness}%` : '—'}</span>
                  {@render chevron()}
                </button>
                <button type="button" class="row" onclick={() => go('settings', 'aux')}>
                  <span class="sq green"><Icon name="shield" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>电池保护</b><small>限制充电上限</small></span>
                  <span class="row-v">{snapshot?.battery_care === true ? '已开启' : snapshot?.battery_care === false ? '已关闭' : '—'}</span>
                  {@render chevron()}
                </button>
                <div class="row">
                  <span class="sq indigo"><Icon name="bolt" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>供电方式</b><small>{snapshot?.ac_power === true ? '电源适配器供电' : snapshot?.ac_power === false ? '纯电池供电' : '状态未知'}</small></span>
                  <span class="row-v">{snapshot?.ac_power === true ? '已接电' : snapshot?.ac_power === false ? '电池' : '—'}</span>
                </div>
                <div class="row">
                  <span class="sq gray"><Icon name="list" size={15} strokeWidth={2} /></span>
                  <span class="row-l"><b>最近读取</b><small>{lastRead ? `记录时间 ${lastRead}` : '尚无记录'}</small></span>
                  <span class="row-v">{snapshot && snapshotStale ? '读数已过期' : lastRead ? '刚刚更新' : '—'}</span>
                </div>
              </div>

              <div class="sh"><span>快捷操作</span></div>
              <div class="tiles">
                <button type="button" class="tile" onclick={() => go('fan')}>
                  <span class="sq orange"><Icon name="fan" strokeWidth={2} /></span>
                  <b>性能与风扇</b>
                  <small>切换性能模式，调整双风扇与温控曲线</small>
                </button>
                <button type="button" class="tile" onclick={() => go('settings', 'light')}>
                  <span class="sq pink"><Icon name="key" strokeWidth={2} /></span>
                  <b>键盘灯光</b>
                  <small>颜色、亮度与动态效果</small>
                </button>
                <button type="button" class="tile" onclick={() => go('plugin')}>
                  <span class="sq green"><Icon name="plugin" strokeWidth={2} /></span>
                  <b>服务插件</b>
                  <small>Acer 服务保护与 Device Center</small>
                </button>
              </div>

            {:else if shownPage === 'fan'}
              <div class="sh">
                <span>接电性能模式</span>
                <StatusTag
                  text={profileDirty && selectedProfile ? '待应用 · ' + profileNames[selectedProfile] : snapshot?.profile ? '当前 · ' + profileNames[snapshot.profile] : '尚无设备读数'}
                  kind={profileDirty && selectedProfile ? 'draft' : snapshot?.profile ? 'ok' : ''}
                />
              </div>
              <div class="group">
                <div class="pad">
                  <SegmentedControl
                    value={selectedProfile ?? ''}
                    current={snapshot?.profile ?? undefined}
                    block
                    label="性能模式草稿"
                    options={(['quiet','balanced','performance','turbo','eco'] as Profile[])
                      .filter(p => p !== 'eco' || capabilities?.profiles.includes('eco'))
                      .map(p => ({
                        value: p,
                        label: profileNames[p],
                        disabled: !deviceControlsReady || !capabilities?.profile_control.supported || !capabilities.profiles.includes(p)
                      }))}
                    disabled={isWriting('performance') || !deviceControlsReady || thermalOwnsFans}
                    onchange={(profile) => selectProfile(profile as Profile)}
                  />
                  <p class="hint profile-desc">
                    {selectedProfile ? profileDescriptions[selectedProfile] : '选择一个设备支持的模式'}{profileDirty ? ' · 尚未应用' : ''}
                  </p>
                </div>
                <ReadConflict readKey={snapshot?.profile ?? null} edited={profileDirty} matches={selectedProfile === snapshot?.profile} label={snapshot?.profile ? profileNames[snapshot.profile] : '—'} onadopt={() => { selectedProfile = snapshot?.profile ?? null; profileDirty = false; }}/>
                <div class="row foot">
                  <span class="hint">
                    {!profileAllowed
                      ? permissionNeeded
                        ? '设备读取权限未确认，请先授权。'
                        : snapshotStale || !snapshot
                          ? '设备状态未确认，暂不能选择性能模式。'
                          : capabilities?.profile_control.reason || '尚未确认设备支持性能模式切换。'
                      : '设备读回确认后，当前模式才会真正更新。'}
                  </span>
                  <button type="button" class="btn primary" disabled={performanceLocked || !profileAllowed || thermalOwnsFans} onclick={applyProfile}>
                    {#if isWriting('performance')}<span class="spinner" aria-hidden="true"></span>应用中…{:else}应用模式{/if}
                  </button>
                </div>
              </div>

              <div class="sh">
                <span>风扇实时状态</span>
                <span>{snapshot?.fan_mode === 'auto' ? '自动' : snapshot?.fan_mode === 'max' ? '最高转速' : snapshot?.fan_mode === 'custom' ? '自定义' : '模式未知'}</span>
              </div>
              <div class="group">
                {#each [
                  { label: 'CPU 风扇', temperature: snapshot?.cpu_temperature, rpm: snapshot?.cpu_fan_rpm, percent: snapshot?.cpu_fan_percent },
                  { label: 'GPU 风扇', temperature: snapshot?.gpu_temperature, rpm: snapshot?.gpu_fan_rpm, percent: snapshot?.gpu_fan_percent }
                ] as fan}
                  <div class="row">
                    <span class="row-l"><b>{fan.label}</b><small>核心温度 {value(fan.temperature, '°C')}</small></span>
                    <span class="fan-val">
                      <span class="meter" aria-hidden="true"><i style="--v: {fanFraction(fan.percent)}"></i></span>
                      <span class="row-v strong"><AnimatedNumber value={fan.rpm ?? null} mode="rpm" useGrouping={false} /> <small>RPM</small></span>
                    </span>
                  </div>
                {/each}
              </div>

              <FanControls
                mode={snapshot?.fan_mode ?? null}
                cpuPercent={snapshot?.cpu_fan_percent ?? null}
                gpuPercent={snapshot?.gpu_fan_percent ?? null}
                minPercent={capabilities?.fan_percent_range?.min ?? Number.NaN}
                maxPercent={capabilities?.fan_percent_range?.max ?? Number.NaN}
                canControl={fanAllowed && !thermalOwnsFans}
                canCustom={!!capabilities?.fan_custom_control?.supported && !thermalOwnsFans}
                busy={isWriting('performance')}
                onapply={applyFan}
              />
              {#if !fanAllowed || !capabilities?.fan_custom_control?.supported}
                <p class="hint below">
                  {!fanAllowed
                    ? capabilities?.fan_control?.reason || '风扇控制尚未开放。'
                    : capabilities?.fan_custom_control?.reason || '自定义风扇能力尚未提供。'}
                </p>
              {/if}

              <ThermalControls allowed={deviceControlsReady && !!capabilities?.fan_custom_control?.supported && snapshot?.ac_power === true} onownership={(owned)=>thermalOwnsFans=owned}/>

            {:else if shownPage === 'acer' || shownPage === 'plugin'}
              <CentralPlugin authorized={vendorAuthorized} onauthorize={authorizeDevice} />
            {:else if shownPage === 'dc'}
              <div class="dc-page"><DeviceCenterPeripherals /></div>

            {:else if shownPage === 'settings'}
              <div class="seg-wrap">
                <SegmentedControl
                  value={settingsTab}
                  label="设置类别"
                  options={[
                    { value: 'light', label: '键盘灯光' },
                    { value: 'aux', label: '硬件辅助与电源' },
                    { value: 'app', label: '应用与系统' }
                  ]}
                  onchange={(val) => {
                    if (val === 'light' || val === 'aux' || val === 'app') selectSettingsTab(val);
                  }}
                />
              </div>

              <div class="panes">
                <!-- Panes stay mounted; hidden panes are inert and the shown one slides in from its side. -->
                <div class="pane" class:active={settingsTab === 'light'} style="--dir: {settingsDirection}" inert={settingsTab !== 'light'}>
                  <LightingEffects
                    config={snapshot?.lighting_config ?? null}
                    canStatic={lightingAllowed}
                    canDynamic={dynamicLightingAllowed}
                    canRestore={lightingRestoreAllowed}
                    restoreReason={capabilities?.lighting_restore_control?.reason ?? null}
                    busy={isWriting('lighting')}
                    onstatic={applyLighting}
                    onanimation={applyLightingAnimation}
                    onrestore={restoreLighting}
                  />
                  {#if !lightingAllowed && !dynamicLightingAllowed}
                    <p class="hint below">
                      {capabilities?.lighting_control?.reason || capabilities?.lighting_dynamic_control?.reason || '连接设备并获取灯光控制权限后可调整。'}
                    </p>
                  {/if}
                </div>

                <div class="pane" class:active={settingsTab === 'aux'} style="--dir: {settingsDirection}" inert={settingsTab !== 'aux'}>
                  <div class="sh"><span>电池保护</span></div>
                  <div class="group">
                    <div class="row">
                      <span class="row-l">
                        <b>限制充电上限</b>
                        <small>电量 {value(snapshot?.battery_percent, '%')} · 当前读回：{snapshot?.battery_care === true ? '开启' : snapshot?.battery_care === false ? '关闭' : '未知'}</small>
                      </span>
                      <Switch
                        role="switch"
                        aria-label="限制充电上限"
                        checked={batteryDraft ?? false}
                        disabled={writeBlocked('battery') || !batteryAllowed}
                        pending={isWriting('battery')}
                        onchange={requestBatteryCare}
                      />
                    </div>
                    <ReadConflict readKey={typeof snapshot?.battery_care === 'boolean' ? String(snapshot.battery_care) : null} edited={batteryEdited} matches={batteryDraft === snapshot?.battery_care} label={snapshot?.battery_care ? '开启' : '关闭'} onadopt={() => { batteryDraft = snapshot?.battery_care ?? null; batteryEdited = false; }}/>
                    {#if !batteryAllowed}
                      <div class="row">
                        <span class="hint">{capabilities?.battery_control?.reason || '电池保护能力或当前状态尚未读取，控制暂不可用。'}</span>
                      </div>
                    {/if}
                  </div>

                  <UsbCharge authorized={vendorAuthorized} refreshToken={vendorRefreshToken} />

                  {#if rules && !rulesError}
                    <PowerRules
                      config={rules.config}
                      status={rules.status ?? ''}
                      acProfiles={rules.ac_profiles}
                      dcProfiles={rules.dc_profiles}
                      busy={writeBlocked('rules')}
                      onsave={saveRules}
                    />
                  {:else}
                    <div class="sh"><span>电源切换规则</span></div>
                    <div class="group">
                      <div class="row">
                        <span class="row-l">
                          <b>{rulesError ? '规则配置读取失败' : '正在读取原生规则配置…'}</b>
                          {#if rulesError}<small class="err-text">{rulesError}</small>{/if}
                          <small>规则编辑暂不可用，请使用刷新状态重新读取。</small>
                        </span>
                      </div>
                    </div>
                  {/if}

                  <VendorControls authorized={vendorAuthorized} refreshToken={vendorRefreshToken} />
                  <AudioControls authorized={vendorAuthorized} refreshToken={vendorRefreshToken} />
                </div>

                <div class="pane" class:active={settingsTab === 'app'} style="--dir: {settingsDirection}" inert={settingsTab !== 'app'}>
                  {#if settingsError && settingsError !== readError && settingsError !== capabilityError}
                    <div class="banner inline" role="status">
                      <span class="sq pink"><Icon name="warn" /></span>
                      <div class="grow"><b>设置读取失败</b><p>{settingsError}</p></div>
                    </div>
                  {/if}

                  <div class="sh"><span>外观与采样</span></div>
                  <div class="group">
                    <div class="row">
                      <span class="row-l"><b>主题外观</b><small>选择浅色、深色或跟随系统设置</small></span>
                      <div class="theme-seg" bind:this={themeSegment}>
                        <SegmentedControl
                          value={settingsDraft.theme}
                          size="small"
                          label="主题外观"
                          options={[
                            { value: 'system', label: '跟随系统' },
                            { value: 'light', label: '浅色' },
                            { value: 'dark', label: '深色' }
                          ]}
                          onchange={changeTheme}
                        />
                      </div>
                    </div>
                    <div class="row">
                      <span class="row-l"><b>采样更新频率</b><small>窗口最小化或后台时自动暂停</small></span>
                      <PopupSelect aria-label="采样更新频率" bind:value={settingsDraft.poll_interval_ms} disabled={!settings || isWriting('settings')} options={[{value:1000,label:'1 秒（实时）'},{value:2000,label:'2 秒（标准）'},{value:5000,label:'5 秒（节能）'}]}/>
                    </div>
                  </div>

                  <div class="sh"><span>侧栏</span><span>本机界面偏好 · 立即生效</span></div>
                  <div class="group" data-g="side">
                    <div class="slider-row">
                      <label for="side-width">宽度</label>
                      <input id="side-width" class="slider" type="range" min={SIDE_LIMITS.width[0]} max={SIDE_LIMITS.width[1]} step="4" value={sideLayout.width} style:--p={(sideLayout.width - SIDE_LIMITS.width[0]) / (SIDE_LIMITS.width[1] - SIDE_LIMITS.width[0]) * 100 + '%'} oninput={e => setSideLayout({ width: +e.currentTarget.value }, false)} onchange={saveSideLayout}/>
                      <output for="side-width">{sideLayout.width} px</output>
                    </div>
                    <div class="slider-row">
                      <label for="side-pad">边距</label>
                      <input id="side-pad" class="slider" type="range" min={SIDE_LIMITS.pad[0]} max={SIDE_LIMITS.pad[1]} step="1" value={sideLayout.pad} style:--p={(sideLayout.pad - SIDE_LIMITS.pad[0]) / (SIDE_LIMITS.pad[1] - SIDE_LIMITS.pad[0]) * 100 + '%'} oninput={e => setSideLayout({ pad: +e.currentTarget.value }, false)} onchange={saveSideLayout}/>
                      <output for="side-pad">{sideLayout.pad} px</output>
                    </div>
                    <div class="row foot">
                      <span class="hint">边距同时决定收起后的宽度（{40 + 2 * sideLayout.pad} px）。也可以拖动侧栏右边缘调整宽度，双击恢复默认。</span>
                      <button type="button" class="btn" disabled={isSideDefault()} onclick={() => setSideLayout({ ...SIDE_DEFAULTS })}>恢复默认</button>
                    </div>
                  </div>

                  <div class="sh"><span>登录启动项</span></div>
                  <div class="group">
                    <div class="row">
                      <span class="row-l">
                        <b>登录后启动</b>
                        <small id="startup-description">读取系统启动项；点击后立即保存。实际登录启动尚待重新登录验收。</small>
                        <small id="startup-state" aria-live="polite">{startupError ? '读取失败，启动状态未知' : !startup ? '正在读取系统启动项…' : startup.mismatch ? '系统存在启动项，但目标与当前应用不一致' : startup.enabled ? '系统读回：已启用' : startup.entry_present ? '系统读回：存在启动项，但未启用' : '系统读回：已关闭'}</small>
                      </span>
                      <Switch
                        role="switch"
                        aria-label="登录后启动控制台"
                        aria-describedby="startup-description startup-state"
                        checked={startup?.enabled ?? false}
                        disabled={writeBlocked('startup') || !startup || !!startupError}
                        pending={isWriting('startup')}
                        onchange={requestStartup}
                      />
                    </div>
                  </div>

                  <div class="sh"><span>窗口与退出偏好</span></div>
                  <div class="group">
                    <div class="row">
                      <span class="row-l"><b>关闭窗口时保留托盘</b><small>关闭窗口后释放界面并保留后台；再次启动时重新打开。</small></span>
                      <Switch role="switch" aria-label="关闭窗口时保留托盘" bind:checked={settingsDraft.close_to_tray} disabled={!settings || isWriting('settings')} />
                    </div>
                    <div class="row">
                      <span class="row-l"><b>退出时恢复临时风扇设置</b><small>退出时恢复本次会话首次风扇改动前的状态；启用需先授权并读取原风扇状态。</small></span>
                      <Switch role="switch" aria-label="退出时恢复临时风扇设置" bind:checked={settingsDraft.restore_on_exit} disabled={!settings || isWriting('settings')} />
                    </div>
                    <ReadConflict readKey={settings ? JSON.stringify(settings) : null} edited={settingsDirty} matches={!settingsDirty} label={settings ? ({system:'跟随系统',light:'浅色',dark:'深色'}[settings.theme] + ` · ${settings.poll_interval_ms / 1000} 秒 · 托盘${settings.close_to_tray ? '开' : '关'} · 退出恢复${settings.restore_on_exit ? '开' : '关'}`) : '—'} onadopt={() => { if (settings) settingsDraft = { ...settings }; settingsThemeTouched = false; }}/>
                    <div class="row foot">
                      <span class="hint">{settingsDirty ? '有未保存的设置' : settings ? '设置已同步' : '读取设置后可保存更改'}</span>
                      <button type="button" class="btn primary" disabled={writeBlocked('settings') || !settingsDirty} onclick={saveSettings}>
                        {#if isWriting('settings')}<span class="spinner" aria-hidden="true"></span>保存中…{:else}保存设置{/if}
                      </button>
                    </div>
                  </div>

                  <OfficialStartup />

                  <div class="group spaced">
                    <button type="button" class="disc" aria-expanded={capsOpen} aria-controls="capabilities-panel" onclick={() => (capsOpen = !capsOpen)}>
                      设备能力与说明{@render chevron()}
                    </button>
                    <Collapse open={capsOpen} id="capabilities-panel">
                      {#each [
                        { label: '状态读取', capability: capabilities?.telemetry },
                        { label: '性能模式', capability: capabilities?.profile_control },
                        { label: '风扇控制', capability: capabilities?.fan_control },
                        { label: '自定义风扇', capability: capabilities?.fan_custom_control },
                        { label: '静态灯光', capability: capabilities?.lighting_control },
                        { label: '动态灯效', capability: capabilities?.lighting_dynamic_control },
                        { label: '灯光恢复', capability: capabilities?.lighting_restore_control }
                      ] as item}
                        <div class="row">
                          <span class="row-l">
                            <b>{item.label}</b>
                            {#if item.capability?.reason}<small>{item.capability.reason}</small>{/if}
                          </span>
                          <span class="row-v">{capabilityState(item.capability)}</span>
                        </div>
                      {/each}
                    </Collapse>
                  </div>
                </div>
              </div>
            {/if}

            {#if snapshot?.errors?.length}
              <div class="group spaced">
                <button type="button" class="disc" aria-expanded={diagOpen} aria-controls="diagnostics-panel" onclick={() => (diagOpen = !diagOpen)}>
                  <span class="sq orange small"><Icon name="warn" size={13} strokeWidth={2} /></span>
                  部分数据尚未读取 · 查看诊断信息 ({[...new Set(snapshot.errors)].length})
                  {@render chevron()}
                </button>
                <Collapse open={diagOpen} id="diagnostics-panel">
                  <div class="pad hint diag-list">
                    {#each [...new Set(snapshot.errors)] as error}<p>{error}</p>{/each}
                  </div>
                </Collapse>
              </div>
            {/if}

            <footer class="foot-bar">
              <span>{loading ? '正在读取设备…' : lastRead ? `上次读取 ${lastRead}${snapshot && snapshotStale ? '（已过期）' : ''}` : '尚无设备读数'}</span>
              <LogPopover {log} {statusLabel} />
            </footer>
          </div>
        {/key}
      </div>
    </main>
  </div>
</div>

{#if dcExitError}
  <ConfirmDialog title="未能让模块留在后台" oncancel={()=>dcExitError=false}>
    <p>已开启“控制台退出时保持后台运行”，但没能通知 Device Center 模块继续运行。为避免模块随控制台一起停止，控制台没有退出。</p>
    <p>可以稍后再试，或者停止模块后退出。</p>
    {#if dcExitMessage}<p role="status">{dcExitMessage}</p>{/if}
    <div class="actions"><button class="btn" data-dialog-cancel disabled={isWriting('exit')} onclick={()=>dcExitError=false}>暂不退出</button><button class="btn danger" disabled={isWriting('exit')} onclick={stopModuleAndExit}>停止模块并退出</button></div>
  </ConfirmDialog>
{/if}

<style>
  /* ---------- Frame: sidebar + workspace ---------- */
  .app {
    display: grid;
    grid-template-columns: var(--side-w) minmax(0, 1fr);
    height: 100dvh;
    transition: grid-template-columns var(--d-smooth) var(--spring-smooth);
    width: 100vw;
    overflow: hidden;
    background: var(--bg);
  }
  .side {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    padding: 14px var(--side-pad) 12px;
    background: var(--sidebar);
    backdrop-filter: blur(30px) saturate(180%);
    -webkit-backdrop-filter: blur(30px) saturate(180%);
    border-right: .5px solid var(--sep);
    user-select: none;
  }
  .brand { position: relative; display: flex; align-items: center; gap: 10px; min-height: 44px; margin-bottom: 14px; padding: 0 36px 0 8px; white-space: nowrap; }
  .brand-mark {
    width: 30px; height: 30px; border-radius: 8px; flex: none;
    display: grid; place-items: center;
    background: linear-gradient(145deg, #2b2b2f, #0d0d0f);
    color: #7ee0ff;
    box-shadow: inset 0 0 0 .5px rgba(255, 255, 255, .15);
    pointer-events: none;
  }
  .brand-text { min-width: 0; pointer-events: none; }
  .brand-text b { display: block; font-size: 14px; font-weight: 600; letter-spacing: -.01em; }
  .brand-text small { display: block; font-size: 11px; color: var(--label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .nav { position: relative; display: grid; gap: 2px; }
  .nav-hl {
    position: absolute; left: 0; right: 0; top: 0;
    border-radius: 9px;
    background: var(--surface);
    box-shadow: var(--shadow-thumb);
    opacity: 0;
    transition: transform var(--d-snappy) var(--spring-snappy), height var(--d-snappy) var(--spring-snappy), opacity var(--d-fast);
  }
  .nav-hl.shown { opacity: 1; }
  /* 分节标题：展开时是小字，收起时退化成一条细线，占位高度不变，避免导航跳动 */
  .nav-sec { position: relative; height: 30px; padding: 12px 8px 0; font-size: 11px; font-weight: 600; color: var(--label-3); white-space: nowrap; overflow: hidden; }
  .nav-sec span { transition: opacity 220ms var(--ease-out) 110ms; }
  .nav-sec::after { content: ''; position: absolute; left: 10px; right: 10px; top: 18px; height: .5px; background: var(--sep); opacity: 0; transition: opacity 120ms var(--ease-in); }
  :global(:root[data-side="collapsed"]) .nav-sec span { opacity: 0; transition: opacity 120ms var(--ease-in); }
  :global(:root[data-side="collapsed"]) .nav-sec::after { opacity: 1; transition: opacity var(--d-fast) var(--ease-out) 110ms; }
  :global(:root[data-theme="dark"]) .nav-hl { background: rgba(255, 255, 255, .1); }
  .nav button {
    position: relative; z-index: 1;
    display: flex; align-items: center; gap: 10px;
    height: 36px; padding: 0 8px;
    border: 0; border-radius: 9px;
    background: none; color: var(--label);
    font-size: 13px; font-weight: 500; text-align: left;
    transition: background-color var(--d-fast) var(--ease-out), transform var(--d-snappy) var(--spring-snappy);
  }
  .nav button:hover:not(.on) { background: var(--fill-2); }
  .nav button:active { transform: scale(.98); transition-duration: var(--d-press); }
  .nav button { white-space: nowrap; overflow: hidden; }
  .nav-label { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .side-toggle {
    position: absolute; top: 8px; right: 6px; z-index: 2;
    width: 28px; height: 28px; padding: 0;
    border: 0; border-radius: 7px;
    background: none; color: var(--label-2);
    display: grid; place-items: center;
    transition: background-color var(--d-fast) var(--ease-out), color var(--d-fast), transform var(--d-snappy) var(--spring-snappy);
  }
  .side-toggle:hover { background: var(--fill-2); color: var(--label); }
  .side-toggle:active { transform: scale(.92); transition-duration: var(--d-press); }

  /* 收起：宽度用 smooth 弹簧，文字只淡出、不换行 */
  .brand-mark, .brand-text, .nav-label, .foot-full {
    transition: opacity 220ms var(--ease-out) 110ms;
  }
  :global(:root[data-side="collapsed"]) :is(.brand-mark, .brand-text, .nav-label, .foot-full) {
    opacity: 0; pointer-events: none; transition: opacity 120ms var(--ease-in);
  }
  .foot-mini {
    display: grid; place-items: center; align-self: center;
    width: 24px; height: 22px; margin: 0; border-radius: 6px;
    opacity: 0; pointer-events: none;
    transition: opacity 120ms var(--ease-in);
  }
  :global(:root[data-side="collapsed"]) .foot-mini {
    opacity: 1; pointer-events: auto; transition: opacity var(--d-fast) var(--ease-out) 110ms;
  }
  .side-tip {
    position: fixed; z-index: 9500;
    padding: 5px 10px; border-radius: 8px;
    background: var(--elevated);
    backdrop-filter: blur(24px) saturate(180%); -webkit-backdrop-filter: blur(24px) saturate(180%);
    box-shadow: var(--shadow-float);
    font-size: 12px; font-weight: 500; white-space: nowrap;
    pointer-events: none;
    transform: translate(0, -50%);
    transform-origin: left center;
    animation: side-tip-in var(--d-snappy) var(--spring-snappy) both;
  }
  @keyframes side-tip-in { from { opacity: 0; transform: translate(-4px, -50%) scale(.96); } }

  .side-foot { display: grid; margin-top: auto; padding: 10px 0 0; border-top: .5px solid var(--sep); white-space: nowrap; }
  .side-foot > * { grid-area: 1 / 1; min-width: 0; }
  .foot-full { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 9px; }
  .foot-full .dot { margin: 0 4px 0 2px; }
  .foot-text { display: grid; min-width: 0; }
  .foot-text b { font-size: 12px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; }
  .foot-text span { font-size: 11px; color: var(--label-2); overflow: hidden; text-overflow: ellipsis; }
  .foot-mini.conn { margin: 0; justify-self: center; }

  /* 拖动侧栏右边缘调整宽度；拖动时关闭宽度过渡，跟手 */
  .side-resize { position: absolute; top: 0; right: 0; bottom: 0; width: 6px; cursor: col-resize; z-index: 3; touch-action: none; }
  .side-resize::after { content: ''; position: absolute; top: 0; bottom: 0; right: 0; width: 2px; background: var(--accent); opacity: 0; transition: opacity var(--d-fast); }
  .side-resize:hover::after, .side-resize:focus-visible::after, .side-resizing .side-resize::after { opacity: .6; }
  .side-resize:focus-visible { outline: none; }
  .app.side-resizing { transition: none; cursor: col-resize; user-select: none; }

  .ws { position: relative; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .tb-slot { position: absolute; top: 0; left: 0; right: 0; z-index: 20; }
  .main {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
  }
  .content { max-width: 800px; margin: 0 auto; padding: 60px 40px 40px; }

  /* ---------- Toolbar content (rendered inside TitleBar) ---------- */
  .refresh-ico { display: grid; place-items: center; }
  .label-swap { display: grid; }
  .swap-text {
    grid-area: 1 / 1;
    opacity: 0;
    transform: translateY(6px);
    transition: opacity var(--d-fast) var(--ease-out), transform var(--d-snappy) var(--spring-snappy);
  }
  .swap-text.shown { opacity: 1; transform: none; }
  .swap-text:first-child:not(.shown) { transform: translateY(-6px); }

  /* ---------- Page head and transitions ---------- */
  .head { margin: 0 0 22px; }
  .head h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 28px; font-weight: 700;
    letter-spacing: -.022em; line-height: 1.15;
  }
  .head p { margin: 6px 0 0; font-size: 13px; color: var(--label-2); }

  .stage > :global(*) {
    animation:
      app-fade-in 300ms var(--ease-out) both,
      app-rise var(--d-gentle) var(--spring-gentle) both;
    animation-delay: calc(var(--i, 0) * 26ms);
  }
  .stage.leaving {
    opacity: 0;
    transform: translateY(-6px);
    transition: opacity 120ms var(--ease-in), transform 130ms var(--ease-in);
  }
  .head.leaving { transform: none; }
  @keyframes app-fade-in { from { opacity: 0; } }
  @keyframes app-rise { from { transform: translateY(12px); } }
  @keyframes app-pane-in { from { transform: translateX(calc(var(--dir, 1) * 18px)); } }

  .seg-wrap { display: flex; justify-content: center; margin: 0 0 22px; }
  .seg-wrap > :global(*) { min-width: 420px; }
  .pane { display: none; }
  .pane.active { display: block; animation: app-fade-in 260ms var(--ease-out) both, app-pane-in var(--d-smooth) var(--spring-smooth) both; }

  /* ---------- Banners ---------- */
  .banner-wrap { padding-bottom: 20px; }
  .banner {
    display: flex; align-items: center; gap: 14px;
    padding: 14px 16px;
    border-radius: var(--r-card);
    background: var(--surface);
    box-shadow: var(--shadow-card);
  }
  .banner.inline { margin-bottom: 20px; }
  .banner .sq { width: 30px; height: 30px; border-radius: 8px; }
  .banner b { display: block; font-size: 13px; }
  .banner p { margin: 2px 0 0; font-size: 12px; color: var(--label-2); }
  .banner .err-text { color: var(--red-text); }
  .grow { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .banner-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }

  /* ---------- Overview ---------- */
  .hero { display: grid; grid-template-columns: 1fr 1px 1fr; align-items: center; padding: 26px 12px 22px; }
  .hero-sep { align-self: stretch; background: var(--sep); margin: 12px 0; }
  .gauge-cell { display: flex; flex-direction: column; align-items: center; gap: 10px; min-width: 0; }
  .g-meta { font-size: 12px; color: var(--label-2); }
  .g-meta b { color: var(--label); font-weight: 600; }
  .chev { display: grid; place-items: center; color: var(--label-3); flex: none; }
  .disc :global(.chev) { margin-left: auto; transition: transform var(--d-smooth) var(--spring-smooth); }
  .disc[aria-expanded="true"] :global(.chev) { transform: rotate(90deg); }

  .tiles { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .tile {
    display: flex; flex-direction: column; align-items: flex-start; gap: 10px;
    padding: 16px;
    border: 0; border-radius: var(--r-card);
    background: var(--surface); color: var(--label);
    box-shadow: var(--shadow-card);
    text-align: left;
    transition: background-color var(--d-fast) var(--ease-out), transform var(--d-snappy) var(--spring-snappy);
  }
  .tile:hover { background: var(--surface-hover); }
  .tile:active { transform: scale(.975); transition-duration: var(--d-press); }
  .tile b { font-size: 13px; font-weight: 600; }
  .tile small { font-size: 12px; color: var(--label-2); line-height: 1.5; }
  .tile .sq { width: 30px; height: 30px; border-radius: 8px; }

  /* ---------- Performance ---------- */
  .profile-desc { margin: 12px 2px 0; }
  .fan-val { display: flex; align-items: center; gap: 14px; }
  .fan-val .row-v { min-width: 86px; }
  .fan-val small { font-size: 11px; font-weight: 400; color: var(--label-2); }
  .meter { position: relative; width: 120px; height: 6px; border-radius: 3px; background: var(--fill); overflow: hidden; }
  .meter i {
    position: absolute; inset: 0;
    border-radius: 3px;
    background: var(--accent);
    transform-origin: left;
    transform: scaleX(var(--v, 0));
    transition: transform var(--d-smooth) var(--spring-smooth);
  }
  .hint.below { margin: 8px 6px 0; }

  /* ---------- Settings / shared ---------- */
  .theme-seg { flex: none; }
  .spaced { margin-top: 20px; }
  .disc {
    display: flex; align-items: center; gap: 8px;
    width: 100%; min-height: 48px; padding: 0 18px;
    border: 0; border-radius: 0;
    background: none; color: var(--label);
    font-size: 13px; font-weight: 500; text-align: left;
    transition: background-color var(--d-fast);
  }
  .disc:hover { background: var(--surface-hover); }
  .sq.small { width: 22px; height: 22px; border-radius: 6px; }
  .diag-list { display: flex; flex-direction: column; gap: 6px; padding-top: 0; overflow-wrap: anywhere; }
  .err-text { color: var(--red-text); }

  .foot-bar {
    position: relative;
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    margin-top: 36px; padding-top: 16px;
    border-top: .5px solid var(--sep);
    font-size: 12px; color: var(--label-2);
  }

  /* ---------- Narrow window: icon rail ---------- */
  @media (max-width: 900px) {
    .content { padding: 60px 24px 32px; }
    .tiles { grid-template-columns: 1fr 1fr; }
    .seg-wrap > :global(*) { min-width: 0; width: 100%; }
    .meter { width: 72px; }
  }

  /* ---------- Reduced motion: keep a short fade only ---------- */
  @media (prefers-reduced-motion: reduce) {
    .stage > :global(*), .pane.active { animation: app-fade-in 140ms linear both; }
    .stage.leaving { transform: none; transition: opacity 140ms linear; }
    .app, .side-tip, .nav-hl, .nav-sec span, .nav-sec::after, .side-resize::after, .nav button, .tile, .meter i, .swap-text, .disc :global(.chev) { transition-duration: 0s; }
    .swap-text, .swap-text:first-child:not(.shown) { transform: none; }
    .nav button:active, .tile:active { transform: none; }
  }
</style>
