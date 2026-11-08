/* 侧栏布局偏好：展开宽度与内边距。纯界面偏好，立即生效，只存本机 localStorage。
   收起宽度由边距推出（40px 图标位 + 两侧边距），默认 64px。 */
const KEY = 'acer-console.side-layout';
export const SIDE_DEFAULTS = { width: 232, pad: 12 } as const;
export const SIDE_LIMITS = { width: [200, 320], pad: [4, 24] } as const;

const clamp = (v: number, [lo, hi]: readonly [number, number]) => Math.min(hi, Math.max(lo, Math.round(v)));

function load() {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return {
      width: Number.isFinite(raw.width) ? clamp(raw.width, SIDE_LIMITS.width) : SIDE_DEFAULTS.width,
      pad: Number.isFinite(raw.pad) ? clamp(raw.pad, SIDE_LIMITS.pad) : SIDE_DEFAULTS.pad
    };
  } catch { return { ...SIDE_DEFAULTS }; }
}

export const sideLayout = $state(typeof window === 'undefined' ? { ...SIDE_DEFAULTS } : load());

export function applySideLayout() {
  const root = document.documentElement.style;
  root.setProperty('--side-w-open', sideLayout.width + 'px');
  root.setProperty('--side-pad', sideLayout.pad + 'px');
}

export function setSideLayout(patch: Partial<{ width: number; pad: number }>, persist = true) {
  if (patch.width != null) sideLayout.width = clamp(patch.width, SIDE_LIMITS.width);
  if (patch.pad != null) sideLayout.pad = clamp(patch.pad, SIDE_LIMITS.pad);
  applySideLayout();
  if (persist) saveSideLayout();
}

export function saveSideLayout() {
  try { localStorage.setItem(KEY, JSON.stringify(sideLayout)); } catch { /* 存储不可用时只在本次生效 */ }
}

export const isSideDefault = () => sideLayout.width === SIDE_DEFAULTS.width && sideLayout.pad === SIDE_DEFAULTS.pad;
