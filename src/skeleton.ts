// Reusable skeleton loading primitives. Used to show a lightweight placeholder
// while a tab's content is loading, even when data resolves fast — a minimum
// visible delay guarantees the loading state is perceivable instead of a flash.

/** Minimum time a skeleton stays visible, in ms. Guarantees the loading state
 *  is perceivable instead of a sub-frame flash on fast loads. */
export const SKELETON_MIN = 500;

/** Generic shimmering block. */
export function sk(width: string, height = "14px", extra = ""): string {
  return `<span class="sk" style="width:${width};height:${height};${extra}"></span>`;
}

/** A generic content skeleton: a header bar + a stack of card/row blocks.
 *  `blocks` controls how many placeholder blocks render. */
export function contentSkeleton(blocks = 3, opts: { header?: boolean } = {}): string {
  const header = opts.header === false ? "" :
    `<div class="sk-sec-head">${sk("160px", "20px")}${sk("220px", "13px", "margin-left:14px")}</div>`;
  const block = (i: number) => `<div class="sk-block" style="animation-delay:${i * 90}ms">
    <div class="sk-block-head">${sk("42%", "15px")}</div>
    ${sk("100%", "13px")}${sk("78%", "13px")}${sk("60%", "13px")}
  </div>`;
  return `<div class="sk-content">${header}
    ${Array.from({ length: blocks }, (_, i) => block(i)).join("")}
  </div>`;
}

/** Two-pane skeleton (sidebar + main) for split layouts like Refs / Asistente. */
export function splitSkeleton(): string {
  return `<div class="sk-split">
    <aside class="sk-side">
      ${sk("70%", "16px")}
      ${Array.from({ length: 5 }, (_, i) => sk(`${55 + (i % 3) * 18}%`, "34px")).join("")}
    </aside>
    <section class="sk-main">
      ${sk("40%", "20px")}
      ${Array.from({ length: 4 }, () => sk("100%", "13px")).join("")}
    </section>
  </div>`;
}

/** A skeleton mirroring the dashboard card grid. */
export function dashboardSkeleton(): string {
  const card = (i: number) => `<div class="dash-card sk-dash-card" style="animation-delay:${i * 90}ms">
    <div class="sk-dash-head">${sk("55%", "15px")}</div>
    ${sk("80%", "28px")}${sk("100%", "12px")}
  </div>`;
  return `<div class="dash-body pane"><div class="dash-grid">
    ${Array.from({ length: 4 }, (_, i) => card(i)).join("")}
  </div></div>`;
}

/** Build a settings-card skeleton: a card head (title bar) plus N field rows. */
export function settingsCardSkeleton(rows = 2): string {
  const fieldRow = (i: number) => `<div class="sk-field" style="animation-delay:${i * 80}ms">
    <span class="sk sk-label"></span>
    <span class="sk sk-input"></span>
  </div>`;
  return `<div class="card sk-card">
    <div class="sk-card-head"><span class="sk sk-title"></span></div>
    ${Array.from({ length: rows }, (_, i) => fieldRow(i)).join("")}
  </div>`;
}

/** A full settings-section skeleton: header + a stack of cards. */
export function settingsSectionSkeleton(cards = 2): string {
  return `<div class="settings-header">
    <div>
      <span class="sk sk-h2"></span>
      <span class="sk sk-sub"></span>
    </div>
    <span class="sk sk-btn"></span>
  </div>
  ${Array.from({ length: cards }, (_, i) =>
    settingsCardSkeleton(i % 2 === 0 ? 2 : 1)
  ).join("")}`;
}

/** A sidebar skeleton mirroring the settings nav. */
export function settingsSidebarSkeleton(): string {
  return `<div class="settings-side">
    <span class="sk sk-sb-label"></span>
    ${Array.from({ length: 4 }, (_, i) =>
      `<span class="sk sk-nav-item" style="width:${60 + (i % 3) * 25}%"></span>`
    ).join("")}
  </div>`;
}

/** Paint a skeleton into `view` and return a function that resolves only after
 *  at least SKELETON_MIN ms have elapsed. Pair with the real render so the
 *  placeholder is always shown for the minimum duration. */
export function showSettingsSkeleton(view: HTMLElement, cards = 2): () => Promise<void> {
  view.innerHTML = `<div class="settings-body">${settingsSidebarSkeleton()}
    <section class="settings-main">${settingsSectionSkeleton(cards)}</section>
  </div>`;
  return minDwell();
}

/** Paint a generic content skeleton and return the min-dwell waiter. */
export function showContentSkeleton(view: HTMLElement, blocks = 3, opts?: { header?: boolean }): () => Promise<void> {
  view.innerHTML = `<div class="pane sk-pane">${contentSkeleton(blocks, opts)}</div>`;
  return minDwell();
}

/** Paint a split-pane skeleton and return the min-dwell waiter. */
export function showSplitSkeleton(view: HTMLElement): () => Promise<void> {
  view.innerHTML = `<div class="sk-split-wrap pane">${splitSkeleton()}</div>`;
  return minDwell();
}

/** Paint a dashboard skeleton and return the min-dwell waiter. */
export function showDashboardSkeleton(view: HTMLElement): () => Promise<void> {
  view.innerHTML = dashboardSkeleton();
  return minDwell();
}

/** Returns a waiter that resolves after at least SKELETON_MIN ms since now. */
function minDwell(): () => Promise<void> {
  const start = performance.now();
  return async () => {
    const elapsed = performance.now() - start;
    if (elapsed < SKELETON_MIN) {
      await new Promise<void>((r) => setTimeout(r, SKELETON_MIN - elapsed));
    }
  };
}

/** Run `loader` while showing a skeleton, then render with `render`. The
 *  skeleton stays visible for at least SKELETON_MIN regardless of how fast the
 *  loader resolves. Generic helper for tabs that follow the load→render pattern. */
export async function withSkeleton<T>(
  view: HTMLElement,
  skeleton: string,
  loader: () => Promise<T>,
  render: (data: T) => void,
): Promise<void> {
  view.innerHTML = `<div class="pane sk-pane">${skeleton}</div>`;
  const wait = minDwell();
  const [data] = await Promise.all([loader(), wait()]);
  render(data);
}

