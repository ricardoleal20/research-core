// Design-bible helpers — carried byte-exact from prototype/index.html: the
// inline SVG icon set, the badge/btn/card component idioms, and the
// rcSelect (shadcn-style) trigger + popover family. Inline onclick handlers
// are routed through the global RC namespace (ES modules don't create
// globals for top-level functions).
import { t } from "../i18n";

export const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// ========== ICONS (inline SVG — the bible's exact set) ==========
const iconMap = {
  home: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"/></svg>`,
  book: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"/></svg>`,
  sparkle: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 00-2.456 2.456z"/></svg>`,
  message: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M8.625 12a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375M21 12c0 4.556-4.03 8.25-9 8.25a9.764 9.764 0 01-2.555-.337L5.25 21l.587-2.288A8.982 8.982 0 013 12c0-4.556 4.03-8.25 9-8.25s9 3.694 9 8.25z"/></svg>`,
  check: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>`,
  activity: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z"/></svg>`,
  settings: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.751.43.991l1.004.827c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.212 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.751-.43-.991l-1.004-.827a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>`,
  lock: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z"/></svg>`,
  search: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z"/></svg>`,
  plus: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15"/></svg>`,
  folder: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z"/></svg>`,
  chevron: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5"/></svg>`,
  chevronDown: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5"/></svg>`,
  trash: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0"/></svg>`,
  eye: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>`,
  eyeOff: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>`,
  image: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.25 15.75l5.159-5.159a2.25 2.25 0 013.182 0l5.159 5.159m-1.5-1.5l1.409-1.409a2.25 2.25 0 013.182 0l2.909 2.909m-18 3.75h16.5a1.5 1.5 0 001.5-1.5V6a1.5 1.5 0 00-1.5-1.5H3.75A1.5 1.5 0 002.25 6v12a1.5 1.5 0 001.5 1.5zm10.5-11.25h.008v.008h-.008V8.25zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z"/></svg>`,
  link: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m13.35-.622l1.757-1.757a4.5 4.5 0 00-6.364-6.364l-4.5 4.5a4.5 4.5 0 001.242 7.244"/></svg>`,
  fileText: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z"/></svg>`,
  code: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5"/></svg>`,
  history: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>`,
  pencil: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L10.582 16.07a4.5 4.5 0 01-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 011.13-1.897l8.932-8.931zm0 0L19.5 7.125M18 14v4.75A2.25 2.25 0 0115.75 21H5.25A2.25 2.25 0 013 18.75V8.25A2.25 2.25 0 015.25 6H10"/></svg>`,
  close: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12"/></svg>`,
  command: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5"/></svg>`,
  more: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.75a.75.75 0 110-1.5.75.75 0 010 1.5zM12 12.75a.75.75 0 110-1.5.75.75 0 010 1.5zM12 18.75a.75.75 0 110-1.5.75.75 0 010 1.5z"/></svg>`,
  arrowRight: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3"/></svg>`,
  paperclip: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M18.375 12.739l-7.693 7.693a4.5 4.5 0 01-6.364-6.364l10.94-10.94A3 3 0 1119.5 7.372L8.552 18.32m.009-.01l-.01.01m5.699-9.941l-7.81 7.81a1.5 1.5 0 002.122 2.122l7.81-7.81"/></svg>`,
  bolt: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z"/></svg>`,
  danger: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z"/></svg>`,
  circle: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/></svg>`,
  x: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12"/></svg>`,
  openai: `<svg class="{cls}" viewBox="0 0 24 24" fill="currentColor"><path d="M22.282 9.821a5.985 5.985 0 0 0-.516-4.91 6.046 6.046 0 0 0-6.51-2.9A6.065 6.065 0 0 0 4.981 4.18a5.985 5.985 0 0 0-3.998 2.9 6.046 6.046 0 0 0 .743 7.097 5.98 5.98 0 0 0 .51 4.911 6.051 6.051 0 0 0 6.515 2.9A5.985 5.985 0 0 0 13.26 24a6.056 6.056 0 0 0 5.772-4.206 5.99 5.99 0 0 0 3.997-2.9 6.056 6.056 0 0 0-.747-7.073zM13.26 22.43a4.476 4.476 0 0 1-2.876-1.04l.141-.081 4.779-2.758a.795.795 0 0 0 .392-.681v-6.737l2.02 1.168a.071.071 0 0 1 .038.052v5.583a4.504 4.504 0 0 1-4.494 4.494zM3.6 18.304a4.47 4.47 0 0 1-.535-3.014l.142.085 4.783 2.759a.771.771 0 0 0 .78 0l5.843-3.369v2.332a.08.08 0 0 1-.033.062L9.74 19.95a4.5 4.5 0 0 1-6.14-1.646zM2.34 7.896a4.485 4.485 0 0 1 2.366-1.973V11.6a.766.766 0 0 0 .388.676l5.815 3.355-2.02 1.168a.076.076 0 0 1-.071 0l-4.83-2.786A4.504 4.504 0 0 1 2.34 7.896zm16.597 3.855-5.833-3.387L15.119 7.2a.076.076 0 0 1 .071 0l4.83 2.791a4.494 4.494 0 0 1-.676 8.105v-5.678a.79.79 0 0 0-.407-.667zm2.01-3.246-.142-.085-4.774-2.782a.776.776 0 0 0-.785 0L9.409 9.23V6.897a.066.066 0 0 1 .028-.061l4.83-2.787a4.5 4.5 0 0 1 6.68 4.66zm-12.64 4.135-2.02-1.164a.08.08 0 0 1-.038-.057V6.075a4.5 4.5 0 0 1 7.375-3.453l-.142.08L8.704 5.46a.795.795 0 0 0-.393.681zm1.097-2.365 2.602-1.5 2.607 1.5v2.999l-2.597 1.5-2.607-1.5z"/></svg>`,
  anthropic: `<svg class="{cls}" viewBox="0 0 24 24" fill="currentColor"><path d="M17.304 3.541h-3.671l6.696 16.918h3.67zm-10.608 0L0 20.459h3.744l1.368-3.6h6.776l1.368 3.6h3.744L9.696 3.541zm-.264 10.459 2.04-5.35 2.04 5.35z"/></svg>`,
  ollama: `<svg class="{cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3c4.97 0 9 4.03 9 9v6a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-6c0-4.97 4.03-9 9-9z"/><circle cx="9" cy="10" r="1.5" fill="currentColor" stroke="none"/><circle cx="15" cy="10" r="1.5" fill="currentColor" stroke="none"/><path d="M8 16c1.5 2 4.5 2 6 0"/></svg>`,
  personal: `<svg class="{cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22v-7"/><path d="M9 8V6a3 3 0 0 1 6 0v2"/><path d="M9 8h6"/><path d="M9 12h6"/><rect x="7" y="12" width="10" height="3" rx="1"/></svg>`,
  target: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5.5"/><circle cx="12" cy="12" r="2"/></svg>`,
  sun: `<svg class="{cls}" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6l1.4 1.4m10 10 1.4 1.4m0-12.8-1.4 1.4m-10 10L5.6 18.4"/><circle cx="12" cy="12" r="4"/></svg>`,
};

export const icon = (name, cls = "w-5 h-5") => (iconMap[name] || "").split(`{cls}`).join(cls);

// ========== COMPONENT IDIOMS (the bible's exact markup) ==========
export const badge = (text, color = "primary") => {
  const map = {
    primary: "bg-primary/10 text-primary ring-primary/20",
    success: "bg-emerald-50 text-emerald-700 ring-emerald-600/20",
    warning: "bg-amber-50 text-amber-700 ring-amber-600/20",
    destructive: "bg-rose-50 text-rose-700 ring-rose-600/20",
    muted: "bg-gray-100 text-muted ring-gray-500/10",
    urgent: "bg-rose-50 text-rose-700 ring-rose-600/20",
    high: "bg-orange-50 text-orange-700 ring-orange-600/20",
    medium: "bg-sky-50 text-sky-700 ring-sky-600/20",
    low: "bg-slate-100 text-slate-700 ring-slate-600/20",
  };
  return `<span class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ring-1 ring-inset hover-scale ${map[color] || map.primary}">${esc(text)}</span>`;
};

export const btn = ({ label, variant = "default", size = "md", iconName, cls = "", onClick = "", type = "button", id = "", disabled = false }) => {
  const base = "inline-flex items-center justify-center gap-2 rounded-md font-medium transition-all duration-150 ring-focus";
  const sizes = { sm: "px-3 py-1.5 text-xs", md: "px-4 py-2 text-sm", lg: "px-5 py-2.5 text-sm" };
  const variants = {
    default: "bg-primary text-primaryForeground shadow hover:shadow-md hover:-translate-y-0.5 active:scale-[0.98]",
    secondary: "bg-white text-foreground border border-border shadow-sm hover:bg-gray-50 hover:-translate-y-0.5 active:scale-[0.98]",
    ghost: "bg-transparent text-foreground hover:bg-gray-100",
    outline: "bg-transparent text-foreground border border-border hover:bg-gray-50",
    destructive: "bg-destructive text-white shadow-sm hover:bg-red-600 hover:-translate-y-0.5 active:scale-[0.98]",
  };
  return `<button type="${type}" ${id ? `id="${id}"` : ""} class="${base} ${sizes[size]} ${variants[variant]} ${cls}"${disabled ? " disabled" : ""} ${onClick ? `onclick="${onClick}"` : ""}>${iconName ? icon(iconName, "w-4 h-4") : ""}${label}</button>`;
};

export const card = (children, cls = "") =>
  `<div class="bg-card rounded-xl border border-border shadow-sm hover-lift ${cls}">${children}</div>`;

// shadcn-style select — the bible's rcSelect verbatim; handlers on RC.
export function rcSelect({ id, options, value, onChange = "", size = "md", cls = "", dir = "up", variant = "default" }) {
  const opts = options.map((o) => (typeof o === "string" ? { value: String(o), label: String(o) } : o));
  const val = value === undefined || value === null ? (opts[0] ? opts[0].value : "") : String(value);
  const matched = opts.find((o) => o.value === val);
  const cur = matched || { value: val, label: val };
  const sizes = { xs: "px-2 py-1 text-[11px]", sm: "px-2.5 py-1.5 text-xs", md: "px-4 py-2.5 text-sm" };
  const triggerCls =
    variant === "ghost"
      ? "border-transparent bg-transparent text-muted hover:bg-gray-50 hover:text-foreground"
      : "border border-border bg-white text-foreground hover:border-primary/40";
  return `
  <div class="rc-select relative ${cls}" data-select-id="${id}">
    <input type="hidden" id="${id}" value="${esc(cur.value)}" ${onChange ? `onchange="${onChange}"` : ""}>
    <button type="button" onclick="RC.toggleRcSelect('${id}', event)" class="flex w-full items-center justify-between gap-1.5 rounded-lg ${triggerCls} ${sizes[size]} text-left focus:outline-none focus:ring-2 focus:ring-primary/30 transition">
      <span class="rc-select-label flex items-center gap-1 min-w-0"><span class="rc-select-icon">${cur.icon ? icon(cur.icon, size === "xs" ? "w-3 h-3 shrink-0" : "w-3.5 h-3.5 shrink-0") : ""}</span><span class="rc-select-txt truncate">${esc(cur.label)}</span></span>
      ${icon("chevronDown", (size === "xs" ? "w-3 h-3" : "w-4 h-4") + " text-muted shrink-0")}
    </button>
    <div class="rc-select-menu absolute left-0 right-0 ${dir === "down" ? "top-full mt-1" : "bottom-full mb-1"} z-50 hidden max-h-60 overflow-y-auto rounded-lg border border-border bg-white p-1 shadow-lg animate-fade-up">
      ${opts
        .map(
          (o) => `
        <button type="button" onclick="RC.pickRcSelect('${id}', '${esc(o.value)}', event)" class="rc-select-option flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-sm text-left hover:bg-gray-100 transition ${o.value === cur.value ? "bg-primary/10 text-primary" : "text-foreground"}" data-value="${esc(o.value)}" data-icon="${esc(o.icon || "")}">
          <span class="flex items-center gap-1.5 min-w-0"><span class="rc-opt-icon">${o.icon ? icon(o.icon, "w-3.5 h-3.5 shrink-0") : ""}</span><span class="truncate">${esc(o.label)}</span></span>
          <span class="rc-opt-check">${o.value === cur.value ? icon("check", "w-4 h-4 shrink-0") : ""}</span>
        </button>`,
        )
        .join("")}
    </div>
  </div>`;
}

export function toggleRcSelect(id, ev) {
  if (ev) ev.stopPropagation();
  const wrap = document.querySelector(`[data-select-id="${id}"]`);
  const menu = wrap?.querySelector(".rc-select-menu");
  if (!menu) return;
  const willOpen = menu.classList.contains("hidden");
  closeRcSelects();
  if (willOpen) menu.classList.remove("hidden");
}
export function closeRcSelects() {
  document.querySelectorAll(".rc-select-menu").forEach((m) => m.classList.add("hidden"));
}
export function syncRcSelect(wrap, value) {
  const input = wrap.querySelector("input");
  const iconEl = wrap.querySelector(".rc-select-icon");
  const txt = wrap.querySelector(".rc-select-txt");
  input.value = value;
  const opt = Array.from(wrap.querySelectorAll(".rc-select-option")).find((o) => o.dataset.value === value);
  if (txt) txt.textContent = opt ? opt.textContent.trim() : value;
  if (iconEl) iconEl.innerHTML = opt && opt.dataset.icon ? icon(opt.dataset.icon, "w-3.5 h-3.5 shrink-0") : "";
  wrap.querySelectorAll(".rc-select-option").forEach((o) => {
    const sel = o.dataset.value === value;
    o.classList.toggle("bg-primary/10", sel);
    o.classList.toggle("text-primary", sel);
    o.querySelectorAll(".rc-opt-check").forEach((x) => x.remove());
    if (sel) o.insertAdjacentHTML("beforeend", '<span class="rc-opt-check">' + icon("check", "w-4 h-4 shrink-0") + "</span>");
  });
}
export function pickRcSelect(id, value, ev) {
  if (ev) ev.stopPropagation();
  const wrap = document.querySelector(`[data-select-id="${id}"]`);
  if (!wrap) return;
  syncRcSelect(wrap, value);
  closeRcSelects();
  wrap.querySelector("input").dispatchEvent(new Event("change", { bubbles: true }));
}
export function setRcSelectValue(id, value) {
  const wrap = document.querySelector(`[data-select-id="${id}"]`);
  if (wrap) syncRcSelect(wrap, value);
}

// The page-header idiom every bible screen opens with: caption kicker +
// Instrument Serif italic 36px title + actions right.
export const pageHeader = (kicker, title, actions = "") => `
  <div class="flex flex-col md:flex-row md:items-end justify-between gap-4">
    <div>
      <p class="text-sm text-muted uppercase tracking-[0.06em] font-medium mb-1">${esc(kicker)}</p>
      <h1 class="font-serif text-4xl italic">${esc(title)}</h1>
    </div>
    ${actions ? `<div class="flex gap-2">${actions}</div>` : ""}
  </div>`;

// The mono receipt voice: ids, seqs, timestamps, costs — tabular figures.
export const mono = (s) => `<span class="font-mono text-xs tabular">${esc(s)}</span>`;
export const fmtCents = (c) => `$${(c / 100).toFixed(2)}`;
export const fmtTs = (iso) => (iso ? iso.replace("T", " ").replace("Z", " UTC") : "—");
