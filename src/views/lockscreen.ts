import { el, toast } from "../main";
import { api } from "../api";
import { ico } from "../icons";

/**
 * Full-screen local-access-key challenge. Shown when the lock policy is
 * "on_launch" (boot) or "idle" (timeout), and before sensitive actions.
 * On success calls `onUnlock`.
 */
export function showLockScreen(app: HTMLElement, onUnlock: () => void) {
  const overlay = el(`<div class="lock-overlay" id="lock-overlay"><div class="lock-card">
    <div class="lock-logo">${ico.shield}</div>
    <h1 class="lock-title">Research Core bloqueado</h1>
    <p class="lock-sub">Escribe tu clave de acceso local para continuar.</p>
    <div class="lock-field"><input id="lock-key" type="password" placeholder="••••••••" autocomplete="off"/></div>
    <div id="lock-error"></div>
    <button class="btn btn-primary lock-btn" id="lock-unlock">Desbloquear</button>
  </div></div>`);
  app.appendChild(overlay);
  const input = $("#lock-key", overlay) as HTMLInputElement;
  input.focus();

  const submit = async () => {
    const key = input.value;
    if (!key) { input.focus(); return; }
    try {
      const ok = await api.verifyKey(key);
      if (ok) {
        overlay.remove();
        onUnlock();
      } else {
        $("#lock-error", overlay)!.innerHTML = `<div class="welcome-error">Clave incorrecta.</div>`;
        input.value = "";
        input.focus();
      }
    } catch (e) {
      toast("Error: " + e);
    }
  };
  $("#lock-unlock", overlay)!.addEventListener("click", submit);
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
}

/** Guard a sensitive action behind the lock policy. If policy is "sensitive"
 *  and a lock is configured, show the lock screen first; otherwise run `fn`. */
export async function guardSensitive(app: HTMLElement, fn: () => void): Promise<void> {
  const policy = (await api.lockState().catch(() => ({ policy: "never", idle_min: "", configured: false }))).policy;
  if (policy === "sensitive") {
    showLockScreen(app, fn);
  } else {
    fn();
  }
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
