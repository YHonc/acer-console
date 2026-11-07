/**
 * 覆盖式滚动条（仿 macOS 悬浮细条），移植自 design/apple-redesign-20261105/preview.html。
 *
 * 原生滚动条由 tokens.css 全局隐藏，这里为任意可滚动元素按需创建一个 position:fixed 的 .sb 浮层：
 * 滚动或指针进入时出现，停止 1 秒后淡出；悬停/拖动时加宽并显示轨道；支持滚轮、拖动、点击轨道跳转。
 * 浮层只是装饰，aria-hidden；键盘与辅助技术仍走元素自身滚动。
 * 元素可用 data-sb-top / data-sb-bottom 指定上下内缩（例如被悬浮工具栏遮住的部分）。
 */

type Bar = {
  el: HTMLElement;
  sb: HTMLDivElement;
  thumb: HTMLDivElement;
  hover: boolean;
  drag: { y: number; st: number } | null;
  timer: number;
};

const bars = new WeakMap<Element, Bar>();
const muted = new WeakMap<Element, number>();

function isScroller(el: Element | null): el is HTMLElement {
  if (!(el instanceof HTMLElement) || el === document.documentElement || el === document.body || el.closest('.sb')) return false;
  const o = getComputedStyle(el).overflowY;
  return (o === 'auto' || o === 'scroll') && el.scrollHeight > el.clientHeight + 1;
}

function geo(o: Bar) {
  const el = o.el;
  if (!el.isConnected) return null;
  const max = el.scrollHeight - el.clientHeight;
  if (max <= 1) return null;
  const r = el.getBoundingClientRect();
  const top = r.top + (Number(el.dataset.sbTop) || 3);
  const h = r.bottom - (Number(el.dataset.sbBottom) || 3) - top;
  if (h < 24) return null;
  return { top, h, th: Math.max(28, (h * el.clientHeight) / el.scrollHeight), max, right: r.right };
}

function place(o: Bar) {
  const g = geo(o);
  if (!g) { o.sb.classList.remove('on'); return false; }
  Object.assign(o.sb.style, { top: g.top + 'px', left: g.right - 15 + 'px', height: g.h + 'px' });
  o.thumb.style.height = g.th + 'px';
  o.thumb.style.transform = `translateY(${(g.h - g.th) * Math.min(1, o.el.scrollTop / g.max)}px)`;
  return true;
}

function later(o: Bar) {
  clearTimeout(o.timer);
  o.timer = window.setTimeout(() => {
    if (o.hover || o.drag) return;
    o.sb.classList.remove('on');
    if (!o.el.isConnected) { o.sb.remove(); bars.delete(o.el); }
  }, 1000);
}

function show(o: Bar) {
  if (place(o)) o.sb.classList.add('on');
  later(o);
}

function barOf(el: HTMLElement): Bar {
  const existing = bars.get(el);
  if (existing?.sb.isConnected) return existing;
  const sb = document.createElement('div');
  sb.className = 'sb';
  sb.setAttribute('aria-hidden', 'true');
  const thumb = document.createElement('div');
  thumb.className = 'sb-thumb';
  sb.append(thumb);
  document.body.append(sb);
  const o: Bar = { el, sb, thumb, hover: false, drag: null, timer: 0 };
  bars.set(el, o);
  const to = (top: number) => el.scrollTo({ top, behavior: 'instant' });
  sb.addEventListener('pointerenter', () => { o.hover = true; show(o); });
  sb.addEventListener('pointerleave', () => { o.hover = false; later(o); });
  sb.addEventListener('wheel', e => { e.preventDefault(); el.scrollBy({ top: e.deltaY, behavior: 'instant' }); }, { passive: false });
  sb.addEventListener('pointerdown', e => {
    e.preventDefault();
    const g = geo(o);
    if (!g) return;
    if (e.target !== thumb) to(((e.clientY - g.top - g.th / 2) / (g.h - g.th)) * g.max);
    o.drag = { y: e.clientY, st: el.scrollTop };
    sb.classList.add('drag');
    sb.setPointerCapture(e.pointerId);
  });
  sb.addEventListener('pointermove', e => {
    if (!o.drag) return;
    const g = geo(o);
    if (g) to(o.drag.st + ((e.clientY - o.drag.y) / (g.h - g.th)) * g.max);
  });
  const end = () => { if (!o.drag) return; o.drag = null; sb.classList.remove('drag'); later(o); };
  sb.addEventListener('pointerup', end);
  sb.addEventListener('pointercancel', end);
  return o;
}

/** 程序化滚动（如切页回到顶部）时临时不显示滚动条。 */
export function muteScrollbar(el: Element, ms = 700) {
  muted.set(el, performance.now() + ms);
}

/** 全局安装；返回卸载函数。 */
export function installOverlayScrollbars(): () => void {
  const onScroll = (e: Event) => {
    const el = e.target;
    if (!(el instanceof HTMLElement) || (muted.get(el) ?? 0) > performance.now() || !isScroller(el)) return;
    show(barOf(el));
  };
  let lastTarget: EventTarget | null = null;
  let hovered: HTMLElement | null = null;
  const onMove = (e: PointerEvent) => {
    if (e.target === lastTarget) return;
    lastTarget = e.target;
    if (!(e.target instanceof Element) || e.target.closest('.sb')) return;
    let el: Element | null = e.target;
    while (el && !isScroller(el)) el = el.parentElement;
    if (el === hovered) return;
    hovered = el as HTMLElement | null;
    if (hovered) show(barOf(hovered));
  };
  const onResize = () => { if (hovered && bars.get(hovered)?.sb.classList.contains('on')) place(bars.get(hovered)!); };
  document.addEventListener('scroll', onScroll, { capture: true, passive: true });
  document.addEventListener('pointermove', onMove, { passive: true });
  window.addEventListener('resize', onResize, { passive: true });
  return () => {
    document.removeEventListener('scroll', onScroll, { capture: true });
    document.removeEventListener('pointermove', onMove);
    window.removeEventListener('resize', onResize);
    document.querySelectorAll('body > .sb').forEach(n => n.remove());
  };
}
