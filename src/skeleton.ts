// Reusable skeleton loading primitives. Used to show a lightweight placeholder
// while a tab's content is loading, even when data resolves fast — a minimum
// visible delay guarantees the loading state is perceivable instead of a flash.

/** Minimum time a skeleton stays visible, in ms. Guarantees the loading state
 *  is perceivable instead of a sub-frame flash on fast loads. */
export const SKELETON_MIN = 500;

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
  const start = performance.now();
  return async () => {
    const elapsed = performance.now() - start;
    if (elapsed < SKELETON_MIN) {
      await new Promise<void>((r) => setTimeout(r, SKELETON_MIN - elapsed));
    }
  };
}
