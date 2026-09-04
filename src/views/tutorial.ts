import { el } from "../main";
import { api } from "../api";
import { ico, appLogo } from "../icons";
import { BG, WIN_CONTROLS } from "./welcome";

const CARDS = [
  { icon: ico.book, title: "Investigación", body: "Tu dashboard. Métricas del proyecto, último avance y siguientes pasos de un vistazo." },
  { icon: ico.doc, title: "Refs", body: "Centraliza papers y referencias. Busca, filtra y enlaza citas con tus colecciones." },
  { icon: ico.brain, title: "AI Review", body: "Agentes de revisión (rigor, novedad, claridad…) evalúan tu manuscrito y generan hallazgos accionables." },
  { icon: ico.chat, title: "Asistente", body: "Conversa con un asistente que conoce tus referencias. Resume, redacta y responde sobre tu proyecto." },
  { icon: ico.list, title: "Acciones", body: "Los hallazgos se convierten en tareas. Marca, prioriza y despacha lo que sigue." },
];

/**
 * Overview tutorial page shown once after the setup wizard. A short tour of
 * the five core areas, then `onDone` enters the shell. Markup mirrors OpenDesign
 * frame 15: `.tut-body` ambient stage → `.tut-hero` (logo + title + subtitle)
 * → responsive `.tut-grid` of `.tut-card`s → "Empezar a investigar" button.
 * Contextual coachmarks on the Inicio dashboard are a follow-up (Phase D).
 */
export function renderTutorial(app: HTMLElement, onDone: () => void) {
  const wrap = el(`<div class="app-window">${WIN_CONTROLS}
      <div class="tut-body">${BG}
        <div class="tut-hero">
          <div class="tut-logo">${appLogo()}</div>
          <h1 class="tut-title">Bienvenido a Research Core</h1>
          <p class="tut-subtitle">Un tour rápido por lo que puedes hacer. Después podrás empezar a investigar.</p>
          <div class="tut-grid" id="tut-cards"></div>
          <button class="btn btn-primary tut-btn" id="tut-start" type="button">Empezar a investigar</button>
        </div>
      </div></div>`);
  app.innerHTML = "";
  app.appendChild(wrap);
  $("#tut-cards", wrap)!.innerHTML = CARDS.map((c) => `<div class="tut-card">
    <div class="tut-card-icon">${c.icon}</div>
    <div class="tut-card-title">${c.title}</div>
    <div class="tut-card-body">${c.body}</div></div>`).join("");
  $("#tut-start", wrap)!.addEventListener("click", async () => {
    await api.updateSetting("tutorial_seen", "true");
    onDone();
  });
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
