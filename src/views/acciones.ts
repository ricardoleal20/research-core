import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";
import type { Action } from "../types";

let showDone = false;

export async function renderAcciones(view: HTMLElement) {
  const p = state.active!;
  view.innerHTML = `<div class="acciones-body pane">
    <div class="acciones-head">
      <div class="ah-left"><h3>Acciones</h3><span class="ah-sub" id="ah-sub">cargando…</span></div>
      <div class="ah-actions">
        <div class="seg-toggle" id="done-toggle">
          <button class="seg is-active" data-done="false">Pendientes</button>
          <button class="seg" data-done="true">Completadas</button>
        </div>
        <button class="btn btn-primary" id="new-action">${ico.plus} Nueva acción</button>
      </div>
    </div>
    <div class="acciones-list" id="acciones-list"></div>
  </div>`;
  $$("#done-toggle .seg").forEach((b) =>
    b.addEventListener("click", async () => {
      $$("#done-toggle .seg").forEach((x) => x.classList.toggle("is-active", x === b));
      showDone = b.dataset.done === "true";
      await load(p.id);
    }));
  $("#new-action")!.addEventListener("click", () => openActionForm());
  await load(p.id);
}

async function load(pid: string) {
  const list = $("#acciones-list")!;
  const actions = await api.listActions(pid, showDone);
  const sub = $("#ah-sub")!;
  if (showDone) sub.textContent = `${actions.length} completadas`;
  else sub.textContent = `${actions.length} pendientes · ${actions.filter((a) => a.priority === "high").length} alta prioridad`;
  if (!actions.length) {
    list.innerHTML = `<div class="empty-row">${showDone ? "Sin acciones completadas." : "Sin acciones pendientes. Crea una con «Nueva acción»."}</div>`;
    return;
  }
  list.innerHTML = actions.map((a) => actionHtml(a)).join("");
  $$("#acciones-list .action-row").forEach((row) => {
    const id = row.dataset.id!;
    $(".action-check", row)!.addEventListener("click", async (e) => {
      e.stopPropagation();
      await api.toggleAction(id, !showDone);
      toast(showDone ? "Acción reabierta" : "Acción completada");
      await load(pid);
    });
    row.addEventListener("click", () => openActionForm(actions.find((a) => a.id === id)));
  });
}

function actionHtml(a: Action) {
  const prioColor = a.priority === "high" ? "var(--st-unread)" : a.priority === "med" ? "var(--accent)" : "var(--muted)";
  const originLbl = { manual: "manual", asistente: "asistente", review: "review" }[a.origin] ?? a.origin;
  const due = a.due ? `<span class="a-due">${ico.clock} ${esc(a.due)}</span>` : "";
  return `<article class="action-row ${a.done ? "is-done" : ""}" data-id="${a.id}">
    <button class="action-check ${a.done ? "is-on" : ""}">${a.done ? ico.check : ""}</button>
    <div class="action-content">
      <div class="a-top">
        <span class="a-code mono" style="color:${prioColor}">${esc(a.code)}</span>
        <span class="a-title">${esc(a.title)}</span>
      </div>
      <div class="a-desc">${esc(a.description)}</div>
      <div class="a-meta">
        <span class="a-prio" style="color:${prioColor}">${a.priority.toUpperCase()}</span>
        <span class="a-origin">${originLbl}</span>
        ${a.location ? `<span class="a-loc">${esc(a.location)}</span>` : ""}
        ${due}
      </div>
    </div>
  </article>`;
}

function openActionForm(a?: Action) {
  const pid = state.active!.id;
  const overlay = el(`<div class="modal-overlay" id="act-overlay"><div class="modal" style="width:520px">
    <div class="modal-head"><div class="modal-title">${a ? "Editar acción" : "Nueva acción"}</div></div>
    <div class="modal-body">
      <div class="field-row">
        <div class="field"><label>Código</label><input id="ac-code" type="text" class="mono" value="${a ? esc(a.code) : "ACT-" + Math.floor(Math.random()*900+100)}"></div>
        <div class="field"><label>Prioridad</label><select id="ac-prio">
          ${["high|Alta","med|Media","low|Baja"].map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${a&&a.priority===v?"selected":""}>${l}</option>`}).join("")}
        </select></div>
      </div>
      <div class="field"><label>Título</label><input id="ac-title" type="text" value="${a ? esc(a.title) : ""}"></div>
      <div class="field"><label>Descripción</label><textarea id="ac-desc" rows="3">${a ? esc(a.description) : ""}</textarea></div>
      <div class="field-row">
        <div class="field"><label>Ubicación (sección)</label><input id="ac-loc" type="text" value="${a ? esc(a.location) : ""}"></div>
        <div class="field"><label>Vence (fecha)</label><input id="ac-due" type="text" value="${a ? esc(a.due) : ""}"></div>
      </div>
      <div class="field" style="margin-bottom:0"><label>Notas</label><textarea id="ac-notes" rows="2">${a ? esc(a.notes) : ""}</textarea></div>
    </div>
    <div class="modal-foot">
      <button class="btn btn-secondary" data-action="close">Cancelar</button>
      ${a ? `<button class="btn btn-ghost" id="ac-del" style="margin-right:auto">Eliminar</button>` : ""}
      <button class="btn btn-primary" id="ac-save">Guardar</button>
    </div>
  </div></div>`);
  $(".app-window")!.appendChild(overlay);
  const close = () => overlay.remove();
  $("[data-action='close']", overlay)!.addEventListener("click", close);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  $("#ac-save")!.addEventListener("click", async () => {
    const data = {
      project_id: pid,
      code: val("ac-code"), title: val("ac-title"), description: val("ac-desc"),
      priority: val("ac-prio"), origin: a?.origin ?? "manual",
      due: val("ac-due"), location: val("ac-loc"), notes: val("ac-notes"),
    };
    try {
      if (a) { await api.updateAction({ id: a.id, ...data }); toast("Acción actualizada"); }
      else { await api.createAction(data); toast("Acción creada"); }
      close();
      await load(pid);
    } catch (e) { toast("Error: " + e); }
  });
  $("#ac-del")?.addEventListener("click", async () => {
    await api.deleteAction(a!.id);
    toast("Acción eliminada");
    close();
    await load(pid);
  });
}

function val(id: string) { return ($("#" + id) as HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
