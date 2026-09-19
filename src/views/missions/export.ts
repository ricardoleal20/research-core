// Svelte 5 mount point for the export composer (Story 3.1, FR-7.1/7.2):
// the header's Export action mounts this overlay — there is no nav tab and
// no other surface that opens it (EXPERIENCE.md: a header action).

import { mount, unmount } from "svelte";
import ExportComposer from "./ExportComposer.svelte";

let app: Record<string, unknown> | null = null;

/** Mount the export composer overlay (no-op when already open). */
export function openExport() {
  if (app) return;
  const target = document.createElement("div");
  target.dataset.rcExport = "";
  document.body.appendChild(target);
  app = mount(ExportComposer, { target, props: { onclose: closeExport } });
}

/** Unmount the export composer (no-op when not mounted). */
export function closeExport() {
  if (!app) return;
  const target = document.querySelector("[data-rc-export]");
  unmount(app);
  app = null;
  target?.remove();
}
