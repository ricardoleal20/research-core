import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";
import type { Chat, Message } from "../types";

let currentChat: Chat | null = null;

export async function renderAsistente(view: HTMLElement) {
  const p = state.active!;
  view.innerHTML = `<div class="asistente-body">
    <aside class="pane pane-sidebar" id="as-sidebar"></aside>
    <section class="as-main">
      <div class="as-thread pane" id="as-thread"></div>
      <div class="as-composer">
        <div class="composer-box">
          <textarea class="composer-input" id="as-input" rows="2" placeholder="Pregúntale al asistente sobre tu investigación…"></textarea>
          <div class="composer-bar">
            <button class="composer-action" id="as-attach">${ico.attach}Adjuntar .tex</button>
            <div class="composer-spacer"></div>
            <button class="composer-send" id="as-send">Enviar ${ico.send}</button>
          </div>
        </div>
      </div>
    </section>
  </div>`;
  await renderSidebar(p.id);
  // pick first conversation or create
  const chats = await api.listChats(p.id, "asistente");
  if (chats.length) await openChat(chats[0].id);
  else await newChat();
}

async function renderSidebar(pid: string) {
  const sidebar = $("#as-sidebar")!;
  const chats = await api.listChats(pid, "asistente");
  sidebar.innerHTML = `<div class="sb-head"><h3>Conversaciones</h3>
    <button class="icon-btn" id="as-new" title="Nueva conversación">${ico.plus}</button></div>
    <div class="conv-list" id="conv-list">
      ${chats.length ? chats.map((c) => `
        <button class="conv-item ${currentChat?.id === c.id ? "is-active" : ""}" data-cid="${c.id}">
          <span class="conv-ico">${ico.chat}</span>
          <span class="conv-info"><span class="conv-title">${esc(c.title)}</span><span class="conv-prev">${esc(c.preview || "—")}</span></span>
        </button>`).join("") : `<p style="color:var(--muted);padding:14px;font-size:13px">Sin conversaciones.</p>`}
    </div>`;
  $("#as-new")!.addEventListener("click", newChat);
  $$("#conv-list .conv-item").forEach((b) =>
    b.addEventListener("click", () => openChat(b.dataset.cid!)));
}

async function newChat() {
  const pid = state.active!.id;
  const title = "Conversación " + new Date().toLocaleDateString();
  const c = await api.createChat(pid, "asistente", title);
  await openChat(c.id);
  await renderSidebar(pid);
}

async function openChat(id: string) {
  currentChat = await api.getChat(id);
  const pid = state.active!.id;
  await renderSidebar(pid);
  renderThread();
  $("#as-send")!.onclick = sendMessage;
  $("#as-input")!.onkeydown = (e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) sendMessage(); };
  $("#as-attach")!.onclick = () => toast("Adjuntar .tex: pega el contenido en el mensaje");
}

function renderThread() {
  const t = $("#as-thread")!;
  const msgs = currentChat?.messages ?? [];
  if (!msgs.length) {
    t.innerHTML = `<div style="margin:auto;text-align:center;color:var(--muted);max-width:380px;padding:40px">
      <div style="font-size:28px;margin-bottom:10px">${ico.spark}</div>
      <div style="font-size:15px;font-weight:600;color:var(--fg);margin-bottom:6px">Asistente de investigación</div>
      Pregúntame sobre tu manuscrito, resume referencias, sugiere experimentos o redacta secciones.</div>`;
    return;
  }
  t.innerHTML = msgs.map(msgHtml).join("");
  t.scrollTop = t.scrollHeight;
}

function msgHtml(m: Message) {
  if (m.role === "user") {
    return `<div class="msg msg-user"><span class="msg-avatar">RC</span><div class="msg-bubble">${esc(m.content)}</div></div>`;
  }
  const tag = m.classify_tag ? classifyTag(m.classify_tag) : "";
  return `<div class="msg msg-agent"><span class="msg-avatar">${ico.spark}</span><div class="msg-bubble">${tag}<p>${esc(m.content)}</p></div></div>`;
}

function classifyTag(tag: string) {
  const map: Record<string, string> = {
    question: "Pregunta", answer: "Respuesta", intention: "Intención",
    summary: "Resumen", draft: "Borrador", action: "Acción", other: "Otros",
  };
  return `<span class="classify-tag">${map[tag] ?? tag}</span>`;
}

async function sendMessage() {
  const input = $("#as-input") as HTMLTextAreaElement;
  const content = input.value.trim();
  if (!content || !currentChat) return;
  input.value = "";
  const t = $("#as-thread")!;
  t.appendChild(el(`<div class="msg msg-user"><span class="msg-avatar">RC</span><div class="msg-bubble">${esc(content)}</div></div>`));
  const thinking = el(`<div class="msg msg-agent" id="as-thinking"><span class="msg-avatar">${ico.spark}</span><div class="msg-bubble"><span class="classify-tag ct-intention">Pensando…</span></div></div>`);
  t.appendChild(thinking);
  t.scrollTop = t.scrollHeight;
  $("#as-send")!.setAttribute("disabled", "true");
  try {
    await api.sendMessage(currentChat.id, content);
    currentChat = await api.getChat(currentChat.id);
    $("#as-thinking")?.remove();
    renderThread();
    await renderSidebar(state.active!.id);
  } catch (e) {
    $("#as-thinking")?.remove();
    toast("Error: " + e);
  } finally {
    $("#as-send")!.removeAttribute("disabled");
  }
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
