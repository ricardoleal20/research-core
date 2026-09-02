import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";
import type { Agent, Chat, Message } from "../types";

let currentChat: Chat | null = null;
let agents: Agent[] = [];

const AGENT_ICONS: Record<string, string> = {
  rigor: ico.check, novelty: ico.spark, clarity: ico.pen, repro: ico.refresh, cite: ico.list, orchestrator: ico.brain,
};

export async function renderReview(view: HTMLElement) {
  const p = state.active!;
  view.innerHTML = `<div class="review-chat-body">
    <div class="review-chat-main">
      <div class="review-composer">
        <div class="review-composer-head"><h3>Revisión con el orquestador</h3>
          <span class="sub">describe qué quieres revisar o pega un review externo</span></div>
        <div class="composer-box">
          <textarea class="composer-input" id="review-input" rows="3" placeholder="Ej: «Revisa la §4.2, los ablations necesitan intervalos de confianza» o pega aquí el review de tu comité…"></textarea>
          <div class="composer-bar">
            <button class="prev-reviews-btn" id="prev-reviews">${ico.refresh}<span>Revisiones anteriores</span><span class="pr-count" id="prev-count">0</span></button>
            <button class="composer-action" id="agents-btn">${ico.brain}Agentes</button>
            <div class="composer-spacer"></div>
            <button class="composer-send" id="review-send">Enviar ${ico.send}</button>
          </div>
        </div>
      </div>
      <div class="review-thread pane" id="review-thread"></div>
    </div>
    <aside class="agents-panel" id="agents-panel"></aside>
  </div>`;

  agents = await api.listAgents();
  await renderAgents();
  await loadOrCreateChat(p.id);

  $("#review-send")!.addEventListener("click", sendReview);
  $("#review-input")!.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) sendReview();
  });
  $("#agents-btn")!.addEventListener("click", () => toast("Edita los agentes desde el panel →"));
  $("#prev-reviews")!.addEventListener("click", openPrevReviews);
}

async function renderAgents() {
  const panel = $("#agents-panel")!;
  const judges = agents.filter((a) => a.kind === "judge");
  panel.innerHTML = `<div class="agents-head"><h3>Agentes de revisión</h3><div class="sub">disponibles y habilitados</div></div>
    <div class="agents-list">${judges.map((a) => agentHtml(a)).join("")}</div>`;
  $$("#agents-panel .agent-toggle").forEach((b) =>
    b.addEventListener("click", async () => {
      const id = b.dataset.id!;
      const enabled = !b.classList.contains("is-on");
      await api.toggleAgent(id, enabled);
      b.classList.toggle("is-on", enabled);
      b.setAttribute("aria-checked", String(enabled));
      b.closest(".agent-item")!.classList.toggle("is-enabled", enabled);
      agents = await api.listAgents();
    }));
}

function agentHtml(a: Agent) {
  return `<div class="agent-item ${a.enabled ? "is-enabled" : ""}">
    <div class="agent-icon">${AGENT_ICONS[a.key] ?? ico.check}</div>
    <div class="agent-info"><div class="an">${esc(a.name)}</div><div class="ad">${esc(a.description)}</div></div>
    <button class="agent-toggle ${a.enabled ? "is-on" : ""}" data-id="${a.id}" role="switch" aria-checked="${!!a.enabled}"><span class="knob"></span></button>
  </div>`;
}

async function loadOrCreateChat(pid: string) {
  const chats = await api.listChats(pid, "review");
  $("#prev-count")!.textContent = String(chats.length);
  if (chats.length) {
    currentChat = await api.getChat(chats[0].id);
  } else {
    currentChat = await api.createChat(pid, "review", "Revisión de proyecto");
  }
  renderThread();
}

function renderThread() {
  const t = $("#review-thread")!;
  const msgs = currentChat?.messages ?? [];
  if (!msgs.length) {
    t.innerHTML = `<div style="margin:auto;text-align:center;color:var(--muted);max-width:380px;padding:40px">
      <div style="font-size:15px;font-weight:600;color:var(--fg);margin-bottom:6px">Revisión con el orquestador</div>
      Describe qué revisar arriba. El orquestador activará los jueces habilitados, procesará el manuscrito y creará actions para los hallazgos.</div>`;
    return;
  }
  t.innerHTML = msgs.map(msgHtml).join("");
  t.scrollTop = t.scrollHeight;
}

function msgHtml(m: Message) {
  if (m.role === "user") {
    return `<div class="msg msg-user"><span class="msg-avatar">RC</span><div class="msg-bubble">${esc(m.content)}</div></div>`;
  }
  let extra = "";
  if (m.meta) {
    try {
      const r = JSON.parse(m.meta);
      const dims = (r.dims ?? []).map((d: any) =>
        `<div class="ri-dim"><span class="ri-dim-name">${esc(d.name)}</span><span class="ri-dim-score">${d.score}</span><div class="ri-dim-track"><div class="ri-dim-fill" style="width:${d.score * 10}%;background:${d.score >= 8 ? "var(--st-read)" : "var(--accent)"}"></div></div></div>`).join("");
      const findings = (r.findings ?? []).map((f: any) =>
        `<div class="action-item ${f.severity === "low" ? "done" : ""}"><span class="action-check">${ico.check}</span><div class="action-body"><div class="at-title">${esc(f.text)}</div><div class="at-meta"><span class="action-loc">${esc(f.location)}</span> · ${esc(f.severity)}</div></div></div>`).join("");
      extra = `<div class="review-inline"><div class="review-inline-head"><span class="ri-title">Revisión #${r.number} — ${esc(r.findings?.length ?? 0)} hallazgos</span><span class="ri-score">${r.score}</span></div><div class="review-inline-body"><div class="review-inline-dims">${dims}</div><div class="actions-label">Hallazgos</div>${findings}</div></div>`;
    } catch {}
  }
  const tag = m.classify_tag ? `<span class="classify-tag ct-review">Revisión guardada</span>` : "";
  return `<div class="msg msg-agent"><span class="msg-avatar">${ico.brain}</span><div class="msg-bubble">${tag}<p>${esc(m.content)}</p>${extra}</div></div>`;
}

async function sendReview() {
  const input = $("#review-input") as HTMLTextAreaElement;
  const content = input.value.trim();
  if (!content || !currentChat) return;
  input.value = "";
  // optimistic user msg
  const t = $("#review-thread")!;
  const optimistic = el(`<div class="msg msg-user"><span class="msg-avatar">RC</span><div class="msg-bubble">${esc(content)}</div></div>`);
  t.appendChild(optimistic);
  const thinking = el(`<div class="msg msg-agent" id="thinking"><span class="msg-avatar">${ico.brain}</span><div class="msg-bubble"><span class="classify-tag ct-intention">Procesando…</span><p>Activando jueces y revisando el manuscrito. Esto toma unos segundos.</p></div></div>`);
  t.appendChild(thinking);
  t.scrollTop = t.scrollHeight;
  $("#review-send")!.setAttribute("disabled", "true");
  try {
    await api.sendMessage(currentChat.id, content);
    currentChat = await api.getChat(currentChat.id);
    $("#thinking")?.remove();
    renderThread();
    const chats = await api.listChats(state.active!.id, "review");
    $("#prev-count")!.textContent = String(chats.length);
    toast(`Revisión completada · puntaje ${currentChat.messages?.slice(-1)[0]?.meta ? JSON.parse(currentChat.messages.slice(-1)[0].meta!).score : ""}`);
  } catch (e) {
    $("#thinking")?.remove();
    toast("Error en la revisión: " + e);
  } finally {
    $("#review-send")!.removeAttribute("disabled");
  }
}

function openPrevReviews() {
  api.listReviews(state.active!.id).then((reviews) => {
    if (!reviews.length) { toast("Aún no hay revisiones guardadas"); return; }
    const overlay = el(`<div class="modal-overlay" id="prev-overlay"><div class="modal" style="width:560px">
      <div class="modal-head"><div class="modal-title">Revisiones anteriores</div></div>
      <div class="modal-body"><div class="proj-list">
        ${reviews.map((r) => `<button class="proj-card" data-rid="${r.id}">
          <span class="pdot" style="background:var(--accent)"></span>
          <span class="pinfo"><span class="pname">Revisión #${r.number} · ${r.score}/10</span><span class="pmeta">${esc(r.focus || "general")} · ${new Date(r.created_at).toLocaleDateString()}</span></span>
          <span class="pcount">${r.findings?.length ?? 0} hallazgos</span></button>`).join("")}
      </div></div>
      <div class="modal-foot"><button class="btn btn-secondary" data-action="close">Cerrar</button></div>
    </div></div>`);
    $(".app-window")!.appendChild(overlay);
    $("[data-action='close']", overlay)!.addEventListener("click", () => overlay.remove());
    overlay.addEventListener("click", (e) => { if (e.target === overlay) overlay.remove(); });
    $$(".proj-card", overlay).forEach((c) =>
      c.addEventListener("click", () => showReviewDetail(reviews.find((r) => r.id === c.dataset.rid)!!, overlay)));
  });
}

function showReviewDetail(r: any, overlay: HTMLElement) {
  const dims = (r.dims ?? []).map((d: any) =>
    `<div class="dim-row"><span class="dim-name">${esc(d.name)}</span><span class="dim-score">${d.score}</span><div class="dim-track"><div class="dim-fill" style="width:${d.score * 10}%;background:var(--accent)"></div></div></div>`).join("");
  const findings = (r.findings ?? []).map((f: any) =>
    `<div class="finding"><div class="head"><span class="sev sev-${f.severity}">${f.severity.toUpperCase()}</span><span class="loc">${esc(f.location)}</span></div>${esc(f.text)}</div>`).join("");
  const body = $(".modal-body", overlay)!;
  body.innerHTML = `<div>
    <h3 class="rd-title">Revisión #${r.number}</h3>
    <p class="rd-sub">Score ${r.score}/10 · ${new Date(r.created_at).toLocaleString()}</p>
    <div class="dim-list">${dims}</div>
    <div class="ds-label">Veredicto</div><p style="font-size:13.5px;color:var(--fg);margin-bottom:18px">${esc(r.verdict)}</p>
    <div class="ds-label">Hallazgos</div>${findings || "<p style='color:var(--muted)'>Sin hallazgos.</p>"}
  </div>`;
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
