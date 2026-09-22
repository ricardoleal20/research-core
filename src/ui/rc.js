// The shared RC namespace — the inline onclick surface every rendered
// screen talks to, plus the live app context (state, render, loaders)
// app.js publishes at boot. Screen modules attach their handlers here;
// no module cycles.
export const RC = {};

// Populated by app.js at boot: { app, render, renderMainOnly, navigate,
// loadMissions, loadRuns, loadBoard, loadDigest, loadTrust, loadRefs }.
export const ctx = {};
