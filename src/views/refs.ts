import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";
import { showSplitSkeleton } from "../skeleton";
import type { Ref } from "../types";

let filter = "all";
let selectedId: string | null = null;
let query = "";

export async function renderRefs(view: HTMLElement) {
  const p = state.active!;
  // Split-pane skeleton while refs + collections load.
  const wait = showSplitSkeleton(view);
  await wait();

  view.innerHTML = `<div class="refs-body">
    <aside class="pane pane-sidebar" id="refs-sidebar"></aside>
    <section class="pane pane-list" id="refs-list"></section>
    <section class="pane pane-detail" id="refs-detail"><div class="detail-scroll"><p style="color:var(--muted)">Selecciona una referencia.</p></div></section>
  </div>`;
  await renderSidebar(p.id);
  await renderList(p.id);
}

async function renderSidebar(pid: string) {
  const sidebar = $("#refs-sidebar")!;
  const refs = await api.listRefs(pid);
  const counts = {
    all: refs.length,
    used: refs.filter((r) => r.used).length,
    unused: refs.filter((r) => !r.used).length,
    unread: refs.filter((r) => r.status === "unread").length,
    reading: refs.filter((r) => r.status === "reading").length,
    read: refs.filter((r) => r.status === "read").length,
    reviewed: refs.filter((r) => r.status === "reviewed").length,
  };
  const collections = await api.listCollections(pid);
  sidebar.innerHTML = `
    <div class="sb-section"><div class="sb-label">Filtrar por uso</div><div class="chip-row">
      ${chip("all", "Todas", counts.all)}${chip("used", "Usadas en el paper", counts.used)}${chip("unused", "Sin usar", counts.unused)}
    </div></div>
    <div class="sb-section"><div class="sb-label">Estado</div><div class="chip-row">
      ${chip("status:unread", "Por leer", counts.unread, "var(--st-unread)")}${chip("status:reading", "Leyendo", counts.reading, "var(--st-reading)")}${chip("status:read", "Leído", counts.read, "var(--st-read)")}${chip("status:reviewed", "Revisado", counts.reviewed, "var(--st-reviewed)")}
    </div></div>
    <div class="sb-section"><div class="sb-label">Colecciones</div><div class="col-list">
      ${collections.map((c: any) => `<div class="col-item"><span class="dot" style="background:${c.color}"></span><span class="name">${esc(c.name)}</span><span class="count">${c.count}</span></div>`).join("")}
    </div></div>
    <div class="sb-section"><div class="sb-label">Etiquetas</div><div class="tag-cloud">
      ${uniqueTags(refs).map((t) => `<span class="tag">${esc(t)}</span>`).join("")}
    </div></div>`;
  $$("#refs-sidebar .filter-chip").forEach((b) =>
    b.addEventListener("click", async () => {
      filter = b.dataset.filter!;
      $$("#refs-sidebar .filter-chip").forEach((x) => x.classList.toggle("is-active", x === b));
      await renderList(pid);
    }));
}

function chip(f: string, name: string, count: number, color?: string) {
  const dot = color ? `<span class="dot" style="background:${color};width:8px;height:8px;border-radius:50%"></span>` : "";
  return `<button class="filter-chip ${filter === f ? "is-active" : ""}" data-filter="${f}">${dot}<span class="name">${name}</span><span class="count">${count}</span></button>`;
}

function uniqueTags(refs: Ref[]) {
  const set = new Set<string>();
  refs.forEach((r) => (r.tags ?? "").split(",").forEach((t) => t.trim() && set.add(t.trim())));
  return Array.from(set);
}

async function renderList(pid: string) {
  const list = $("#refs-list")!;
  list.innerHTML = `<div class="list-head">
    <div class="search">${ico.search}<input type="text" id="ref-search" placeholder="Buscar por título, autor, DOI…" value="${esc(query)}"></div>
    <div class="list-meta" id="ref-meta">cargando…</div>
  </div><div class="ref-list" id="ref-list-items"></div>`;
  const search = $("#ref-search") as HTMLInputElement;
  search.addEventListener("input", () => { query = search.value; debouncedSearch(pid); });

  const items = $("#ref-list-items")!;
  let refs: Ref[];
  if (query.trim()) refs = await api.searchRefs(pid, query);
  else refs = await api.listRefs(pid, filter);
  $("#ref-meta")!.innerHTML = `<b>${refs.length} referencias</b> · ${refs.filter((r) => r.used).length} usadas en el paper`;
  if (!refs.length) { items.innerHTML = `<p style="color:var(--muted);padding:20px">Sin resultados.</p>`; return; }
  items.innerHTML = refs.map((r) => refItemHtml(r)).join("");
  $$("#ref-list-items .ref-item").forEach((it) =>
    it.addEventListener("click", () => selectRef(it.dataset.id!)));
  if (selectedId && refs.some((r) => r.id === selectedId)) selectRef(selectedId);
  else if (refs[0]) selectRef(refs[0].id);
}

function refItemHtml(r: Ref) {
  const used = r.used
    ? `<span class="used-tag"><span class="d"></span>Usada · ${r.citation_count} ${r.citation_count === 1 ? "cita" : "citas"}</span>`
    : `<span class="unused-tag">Sin usar</span>`;
  return `<article class="ref-item ${selectedId === r.id ? "is-active" : ""}" data-id="${r.id}">
    <div class="ref-title">${esc(r.title)}</div>
    <div class="ref-by">${esc(r.authors)} — ${r.year} · ${esc(r.venue)}</div>
    <div class="ref-meta">${used}<span class="mono">${r.year}</span></div>
  </article>`;
}

let debounceT: number | undefined;
function debouncedSearch(pid: string) {
  window.clearTimeout(debounceT);
  debounceT = window.setTimeout(() => renderList(pid), 220);
}

async function selectRef(id: string) {
  selectedId = id;
  $$("#ref-list-items .ref-item").forEach((it) => it.classList.toggle("is-active", it.dataset.id === id));
  const detail = $("#refs-detail")!;
  detail.innerHTML = `<div class="detail-scroll"><p style="color:var(--muted)">cargando…</p></div>`;
  const r = await api.getRef(id);
  const usages = r.usages ?? [];
  const tags = (r.tags ?? "").split(",").map((t) => t.trim()).filter(Boolean);
  detail.innerHTML = `<div class="detail-scroll">
    <div class="detail-actions">
      <button class="btn btn-primary" data-action="open-reader">${ico.book} Abrir lector</button>
      <button class="btn btn-secondary" data-action="add-ref">${ico.plus} Editar</button>
      <button class="btn btn-ghost" data-action="delete-ref">Eliminar</button>
    </div>
    <h2 class="detail-title">${esc(r.title)}</h2>
    <p class="detail-authors"><b>${esc(r.authors)}</b></p>
    <p class="detail-doi">DOI: ${esc(r.doi)} · ${esc(r.venue)} ${r.year}</p>
    <div class="detail-section">
      <div class="ds-label">Usada en el paper</div>
      ${usages.length ? `<div class="usage-list">${usages.map((u) =>
        `<div class="usage-item"><span class="usage-loc">${esc(u.location)}</span><span class="usage-ctx">${esc(u.context)}</span><span class="usage-count">cita</span></div>`).join("")}</div>`
        : `<p style="color:var(--muted);font-size:13px">Aún no citada en el manuscrito.</p>`}
    </div>
    <div class="detail-section">
      <div class="ds-label">Etiquetas</div>
      <div class="tag-cloud">${tags.map((t) => `<span class="tag-edit">${esc(t)}<button aria-label="Quitar"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg></button></span>`).join("")}<button class="tag-add">+ añadir etiqueta</button></div>
    </div>
    <div class="detail-section">
      <div class="ds-label">Metadatos</div>
      <div class="meta-grid">
        <div class="meta-cell"><span class="k">DOI</span><span class="v mono">${esc(r.doi)}</span></div>
        <div class="meta-cell"><span class="k">Año</span><span class="v">${r.year}</span></div>
        <div class="meta-cell"><span class="k">Publicación</span><span class="v">${esc(r.venue)}</span></div>
        <div class="meta-cell"><span class="k">ISBN</span><span class="v mono">—</span></div>
        <div class="meta-cell"><span class="k">URL</span><span class="v link" data-url="${esc(r.url)}">${esc(r.url)}</span></div>
        <div class="meta-cell"><span class="k">Estado</span><span class="v">${statusLabel(r.status)}</span></div>
      </div>
    </div>
  </div>`;
  $("#refs-detail [data-action='open-reader']")?.addEventListener("click", () => {
    if (r.url) window.open(r.url, "_blank");
  });
  $("#refs-detail [data-url]")?.addEventListener("click", () => window.open(r.url, "_blank"));
  $("#refs-detail [data-action='delete-ref']")?.addEventListener("click", async () => {
    await api.deleteRef(id);
    toast("Referencia eliminada");
    selectedId = null;
    await renderList(state.active!.id);
  });
  $("#refs-detail [data-action='add-ref']")?.addEventListener("click", () => openRefForm(r));
}

function statusLabel(s: string) {
  return { unread: "Por leer", reading: "Leyendo", read: "Leído", reviewed: "Revisado" }[s] ?? s;
}

export function openRefForm(r?: Ref) {
  const pid = state.active!.id;
  const overlay = el(`<div class="modal-overlay" id="ref-modal-overlay"><div class="modal" style="width:520px">
    <div class="modal-head"><div class="modal-title">${r ? "Editar referencia" : "Nueva referencia"}</div></div>
    <div class="modal-body">
      <div class="field"><label>Título</label><input id="rf-title" type="text" value="${r ? esc(r.title) : ""}"></div>
      <div class="field"><label>Autores</label><input id="rf-authors" type="text" value="${r ? esc(r.authors) : ""}"></div>
      <div class="field-row">
        <div class="field"><label>Año</label><input id="rf-year" type="number" value="${r ? r.year : 2025}"></div>
        <div class="field"><label>Publicación</label><input id="rf-venue" type="text" value="${r ? esc(r.venue) : ""}"></div>
      </div>
      <div class="field"><label>DOI</label><input id="rf-doi" type="text" class="mono" value="${r ? esc(r.doi) : ""}"></div>
      <div class="field"><label>URL</label><input id="rf-url" type="text" class="mono" value="${r ? esc(r.url) : ""}"></div>
      <div class="field"><label>Etiquetas (coma)</label><input id="rf-tags" type="text" value="${r ? esc(r.tags) : ""}"></div>
      <div class="field" style="margin-bottom:0"><label>Estado</label>
        <select id="rf-status">
          ${["unread|Por leer","reading|Leyendo","read|Leído","reviewed|Revisado"].map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${r&&r.status===v?"selected":""}>${l}</option>`}).join("")}
        </select>
      </div>
    </div>
    <div class="modal-foot">
      <button class="btn btn-secondary" data-action="close">Cancelar</button>
      <button class="btn btn-primary" id="rf-save">Guardar</button>
    </div>
  </div></div>`);
  $(".app-window")!.appendChild(overlay);
  const close = () => $("#ref-modal-overlay")?.remove();
  $("[data-action='close']", overlay)?.addEventListener("click", close);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  $("#rf-save")!.addEventListener("click", async () => {
    const data = {
      title: val("rf-title"), authors: val("rf-authors"), year: +val("rf-year") || 2025,
      venue: val("rf-venue"), doi: val("rf-doi"), url: val("rf-url"), tags: val("rf-tags"), status: (val("rf-status")),
    };
    try {
      if (r) { await api.updateRef({ id: r.id, ...data }); toast("Referencia actualizada"); }
      else { await api.createRef({ projectId: pid, ...data }); toast("Referencia creada"); }
      close();
      await renderList(pid);
      if (r) selectRef(r.id);
    } catch (e) { toast("Error: " + e); }
  });
}

function val(id: string) { return ($("#" + id) as HTMLInputElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
