import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";

export async function renderDashboard(view: HTMLElement) {
  const p = state.active!;
  view.innerHTML = `<div class="dash-body pane"><div class="dash-grid" id="dash-grid">
    <div class="dash-card" id="card-review"><h3>${ico.checkCircle}<span>Estado de la revisión</span></h3><p class="card-sub">cargando…</p></div>
    <div class="dash-card" id="card-citations"><h3>${ico.checkCircle}<span>Citas y referencias</span></h3></div>
    <div class="dash-card" id="card-venues"><h3>${ico.layers}<span>Posición y revistas objetivo</span></h3></div>
    <div class="dash-card" id="card-activity"><h3>${ico.clock}<span>Actividad reciente</span></h3></div>
  </div></div>`;

  try {
    const d = await api.getDashboard(p.id);
    renderReviewCard($("#card-review")!, d);
    renderCitationsCard($("#card-citations")!, d);
    renderVenuesCard($("#card-venues")!, p);
    renderActivityCard($("#card-activity")!, d);
  } catch (e) {
    toast("Error al cargar dashboard");
  }
}

function $ (s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }

function renderReviewCard(card: HTMLElement, d: any) {
  const lr = d.last_review;
  const score = lr?.score ?? 0;
  const history: any[] = d.review_history ?? [];
  const maxScore = Math.max(10, ...history.map((h) => h.score));
  const bars = history.length
    ? history.map((h, i) => `<div class="review-mini-bar ${i === history.length - 1 ? "last" : ""}" style="height:${(h.score / maxScore) * 100}%"></div>`).join("")
    : `<div class="review-mini-bar last" style="height:50%"></div>`;
  const prev = history.length > 1 ? history[history.length - 2].score : score;
  const delta = (score - prev).toFixed(1);
  card.innerHTML = `
    <h3>${ico.checkCircle}<span>Estado de la revisión</span></h3>
    <p class="card-sub">${lr ? `última revisión AI · #${lr.number}` : "sin revisiones aún"}</p>
    <div class="review-status-top">
      <div>
        <div class="review-score-big">${score.toFixed(1)}</div>
        <div class="review-score-label">/ 10 · ${d.active_actions} actions</div>
      </div>
      <div class="review-mini-chart">${bars}</div>
    </div>
    <div class="review-trend">${ico.trendUp} ${Number(delta) >= 0 ? "+" : ""}${delta} desde la revisión anterior</div>
    <div class="review-verdict">${lr ? esc(lr.verdict) : "Ejecuta tu primera revisión desde la pestaña <b>AI Review</b>."}</div>`;
}

function renderCitationsCard(card: HTMLElement, d: any) {
  const total = d.refs_total ?? 0;
  const used = d.refs_used ?? 0;
  const unused = Math.max(0, total - used);
  const usedPct = total ? (used / total) * 100 : 0;
  card.innerHTML = `
    <h3>${ico.checkCircle}<span>Citas y referencias</span></h3>
    <p class="card-sub">seguimiento de uso en el manuscrito</p>
    <div class="citation-bar-wrap">
      <div class="citation-bar-used" style="flex:${used}"></div>
      <div class="citation-bar-unused"></div>
    </div>
    <div class="citation-legend">
      <span class="lg"><span class="d" style="background:var(--st-read)"></span>Usadas (${used})</span>
      <span class="lg"><span class="d" style="background:var(--border-2)"></span>Sin usar (${unused})</span>
    </div>
    <div class="citation-stats">
      <div class="citation-stat"><div class="num">${total}</div><div class="lbl">Total refs</div></div>
      <div class="citation-stat"><div class="num">${used}</div><div class="lbl">Citadas</div></div>
      <div class="citation-stat"><div class="num">${d.citations_in_text ?? 0}</div><div class="lbl">Citas en texto</div></div>
    </div>`;
}

function renderVenuesCard(card: HTMLElement, p: any) {
  card.innerHTML = `
    <h3>${ico.layers}<span>Posición y revistas objetivo</span></h3>
    <p class="card-sub">Capítulo ${p.chapter_index} de ${p.chapter_count} · estado del borrador</p>
    <div class="venue-row">
      <div><div class="vname">${esc(p.name)}</div><div class="vmeta">borrador · en progreso</div></div>
      <span class="status-pill sp-reading"><span class="dot"></span>En progreso</span>
    </div>
    <div class="venue-row">
      <div><div class="vname">NeurIPS 2026</div><div class="vmeta">deadline · 22 may 2026</div></div>
      <span class="status-pill sp-unread"><span class="dot"></span>Preparando</span>
    </div>
    <div class="venue-row">
      <div><div class="vname">ICLR 2026</div><div class="vmeta">deadline · 28 sep 2025</div></div>
      <span class="status-pill sp-unread"><span class="dot"></span>Considerando</span>
    </div>`;
}

function renderActivityCard(card: HTMLElement, d: any) {
  const items: any[] = d.timeline ?? [];
  const dots = ["var(--accent)", "var(--st-read)", "var(--dot-3)", "var(--dot-4)"];
  card.innerHTML = `
    <h3>${ico.clock}<span>Actividad reciente</span></h3>
    <p class="card-sub">últimas acciones del proyecto</p>
    <div class="timeline">
      ${items.length ? items.map((t, i) =>
        `<div class="tl-item"><span class="tl-dot" style="background:${dots[i % dots.length]}"></span>
          <span class="tl-text">${t.type === "review" ? "Revisión AI completada — " : "Referencia añadida — "}<b>${esc(t.text)}</b></span>
          <span class="tl-time">${timeAgo(t.created_at)}</span></div>`).join("")
        : `<p style="color:var(--muted);font-size:13px">Sin actividad reciente.</p>`}
    </div>`;
}

export function timeAgo(iso: string): string {
  const then = new Date(iso).getTime();
  const diff = Date.now() - then;
  const m = Math.floor(diff / 60000);
  if (m < 1) return "ahora";
  if (m < 60) return `hace ${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `hace ${h}h`;
  const d = Math.floor(h / 24);
  if (d < 7) return `hace ${d}d`;
  return new Date(iso).toLocaleDateString();
}
