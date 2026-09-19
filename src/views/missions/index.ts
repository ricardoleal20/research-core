// Svelte 5 mount point for the missions surface (AD-8): the first Svelte
// view, mounted inside the existing tab shell as an additional surface. The
// vanilla TS views stay untouched — migration proceeds view-by-view.

import { mount, unmount } from "svelte";
import Missions from "./Missions.svelte";

let app: Record<string, unknown> | null = null;

/** Mount the missions view into the given container. */
export function renderMissions(view: HTMLElement) {
  view.innerHTML = "";
  const target = document.createElement("div");
  target.style.cssText = "flex:1;min-height:0;display:flex;overflow-y:auto;";
  view.appendChild(target);
  app = mount(Missions, { target });
}

/** Unmount the missions view when its tab is left (no-op when not mounted). */
export function unmountMissions() {
  if (app) {
    unmount(app);
    app = null;
  }
}
