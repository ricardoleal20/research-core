// Minimal i18n layer. Translations are keyed dictionaries; `t()` resolves the
// string for the currently selected interface language. For now only Spanish
// (full) and English (mock) are provided — more languages can be added later by
// extending DICT. The active language is read from the persisted settings.

export type Lang = "es" | "en" | "pt" | "fr";

const DICT: Record<string, Record<Lang, string>> = {
  // sidebar / header
  "ajustes.title": { es: "Ajustes", en: "Settings", pt: "Definições", fr: "Réglages" },
  "ajustes.sub": {
    es: "Configuración global de Research Core — general, IA, idioma y preferencias local-first.",
    en: "Research Core global configuration — general, AI, language and local-first preferences.",
    pt: "Configuração global do Research Core — geral, IA, idioma e preferências local-first.",
    fr: "Configuration globale de Research Core — général, IA, langue et préférences local-first.",
  },
  "ajustes.save": { es: "Guardar cambios", en: "Save changes", pt: "Guardar alterações", fr: "Enregistrer" },
  "ajustes.saving": { es: "Guardando…", en: "Saving…", pt: "A guardar…", fr: "Enregistrement…" },
  "ajustes.saved": { es: "Guardado", en: "Saved", pt: "Guardado", fr: "Enregistré" },
  "ajustes.savedToast": { es: "Ajustes guardados", en: "Settings saved", pt: "Definições guardadas", fr: "Réglages enregistrés" },
  "ajustes.saveError": { es: "Error al guardar: ", en: "Error saving: ", pt: "Erro ao guardar: ", fr: "Erreur d'enregistrement : " },

  // sections
  "sec.general": { es: "General", en: "General", pt: "Geral", fr: "Général" },
  "sec.ia": { es: "IA", en: "AI", pt: "IA", fr: "IA" },
  "sec.app": { es: "App", en: "App", pt: "App", fr: "App" },

  // app section
  "app.title": { es: "Aplicación", en: "Application", pt: "Aplicação", fr: "Application" },
  "app.about": { es: "Acerca de", en: "About", pt: "Acerca de", fr: "À propos" },
  "app.version": { es: "Versión", en: "Version", pt: "Versão", fr: "Version" },
  "app.github": { es: "Código en GitHub", en: "Source on GitHub", pt: "Código no GitHub", fr: "Code sur GitHub" },
  "app.githubDesc": {
    es: "Reporta problemas, revisa el changelog y colabora con el proyecto.",
    en: "Report issues, read the changelog and contribute to the project.",
    pt: "Reporta problemas, lê o changelog e colabora com o projeto.",
    fr: "Signalez des problèmes, lisez le changelog et contribuez au projet.",
  },

  // profile
  "profile.title": { es: "Perfil", en: "Profile", pt: "Perfil", fr: "Profil" },
  "profile.name": { es: "Nombre", en: "Name", pt: "Nome", fr: "Nom" },
  "profile.namePh": { es: "Tu nombre", en: "Your name", pt: "O teu nome", fr: "Votre nom" },

  // language
  "lang.title": { es: "Idioma de la interfaz", en: "Interface language", pt: "Idioma da interface", fr: "Langue de l'interface" },
  "lang.native.es": { es: "Español", en: "Spanish", pt: "Espanhol", fr: "Espagnol" },
  "lang.native.en": { es: "Inglés", en: "English", pt: "Inglês", fr: "Anglais" },
  "lang.native.pt": { es: "Portugués", en: "Portuguese", pt: "Português", fr: "Portugais" },
  "lang.native.fr": { es: "Francés", en: "French", pt: "Francês", fr: "Français" },

  // security
  "sec.security": { es: "Seguridad y contraseña", en: "Security & password", pt: "Segurança e palavra-passe", fr: "Sécurité et mot de passe" },
  "sec.lockPolicy": { es: "Bloqueo de acceso", en: "Access lock", pt: "Bloqueio de acesso", fr: "Verrouillage d'accès" },
  "sec.policyNever": { es: "Nunca", en: "Never", pt: "Nunca", fr: "Jamais" },
  "sec.policyOnLaunch": { es: "Al abrir la app", en: "On app launch", pt: "Ao abrir a app", fr: "À l'ouverture" },
  "sec.policyIdle": { es: "Tras X minutos de inactividad", en: "After X minutes idle", pt: "Após X minutos inativo", fr: "Après X min d'inactivité" },
  "sec.policySensitive": { es: "Antes de acciones sensibles", en: "Before sensitive actions", pt: "Antes de ações sensíveis", fr: "Avant actions sensibles" },
  "sec.policyNeverDesc": { es: "La app permanece desbloqueada tras iniciar sesión.", en: "The app stays unlocked after sign-in.", pt: "A app fica desbloqueada após iniciar sessão.", fr: "L'app reste déverrouillée après connexion." },
  "sec.policyOnLaunchDesc": { es: "Pide la clave cada vez que abres Research Core.", en: "Asks for the key every time you open Research Core.", pt: "Pede a chave cada vez que abres o Research Core.", fr: "Demande la clé à chaque ouverture de Research Core." },
  "sec.policyIdleDesc": { es: "Bloquea tras un tiempo de inactividad.", en: "Locks after a period of inactivity.", pt: "Bloqueia após um período de inatividade.", fr: "Verrouille après une période d'inactivité." },
  "sec.policySensitiveDesc": { es: "Pide la clave al borrar proyectos, refs o vaciar acciones.", en: "Asks for the key when deleting projects, refs or clearing actions.", pt: "Pede a chave ao apagar projetos, refs ou limpar ações.", fr: "Demande la clé pour supprimer projets, réf. ou vider les actions." },
  "sec.idleMin": { es: "Minutos de inactividad", en: "Idle minutes", pt: "Minutos de inatividade", fr: "Minutes d'inactivité" },
  "sec.idleCustom": { es: "Personalizado", en: "Custom", pt: "Personalizado", fr: "Personnalisé" },
  "sec.idleCustomPh": { es: "Escribe los minutos", en: "Enter minutes", pt: "Escreve os minutos", fr: "Saisir les minutes" },
  "sec.changeKey": { es: "Cambiar clave de acceso", en: "Change access key", pt: "Alterar chave de acesso", fr: "Changer la clé d'accès" },
  "sec.newKey": { es: "Clave nueva", en: "New key", pt: "Nova chave", fr: "Nouvelle clé" },
  "sec.confirmKey": { es: "Confirmar clave", en: "Confirm key", pt: "Confirmar chave", fr: "Confirmer la clé" },
  "sec.updateKey": { es: "Actualizar clave", en: "Update key", pt: "Atualizar chave", fr: "Mettre à jour" },
  "sec.updating": { es: "Actualizando…", en: "Updating…", pt: "A atualizar…", fr: "Mise à jour…" },
  "sec.keyShort": { es: "La clave es demasiado corta", en: "Key is too short", pt: "A chave é demasiado curta", fr: "Clé trop courte" },
  "sec.keyMismatch": { es: "Las claves no coinciden", en: "Keys do not match", pt: "As chaves não coincidem", fr: "Les clés ne correspondent pas" },
  "sec.keyEnter": { es: "Escribe una clave nueva", en: "Enter a new key", pt: "Escreve uma nova chave", fr: "Saisissez une nouvelle clé" },
  "sec.keyUpdated": { es: "Clave actualizada", en: "Key updated", pt: "Chave atualizada", fr: "Clé mise à jour" },
  "sec.keyError": { es: "No se pudo actualizar: ", en: "Could not update: ", pt: "Não foi possível atualizar: ", fr: "Impossible de mettre à jour : " },

  // local-first
  "local.title": { es: "Local-first", en: "Local-first", pt: "Local-first", fr: "Local-first" },
  "local.store": { es: "Almacenar datos localmente", en: "Store data locally", pt: "Guardar dados localmente", fr: "Stocker localement" },
  "local.storeDesc": {
    es: "Mantiene proyectos, refs y revisiones en tu disco. No se envía nada a la nube.",
    en: "Keeps projects, refs and reviews on your disk. Nothing is sent to the cloud.",
    pt: "Mantém projetos, refs e revisões no teu disco. Nada é enviado para a nuvem.",
    fr: "Conserve projets, réf. et révisions sur votre disque. Rien n'est envoyé dans le cloud.",
  },
  "local.zotero": { es: "Sincronizar refs con Zotero", en: "Sync refs with Zotero", pt: "Sincronizar refs com Zotero", fr: "Sync réf. avec Zotero" },
  "local.zoteroDesc": {
    es: "Copia bidireccional de referencias entre Research Core y Zotero local.",
    en: "Two-way copy of references between Research Core and local Zotero.",
    pt: "Cópia bidirecional de referências entre Research Core e Zotero local.",
    fr: "Copie bidirectionnelle des références entre Research Core et Zotero local.",
  },
  "local.cache": { es: "Caché offline de PDFs", en: "Offline PDF cache", pt: "Cache offline de PDFs", fr: "Cache PDF hors ligne" },
  "local.cacheDesc": {
    es: "Descarga PDFs de refs para acceso sin conexión.",
    en: "Downloads ref PDFs for offline access.",
    pt: "Descarrega PDFs de refs para acesso sem ligação.",
    fr: "Télécharge les PDF des réf. pour un accès hors ligne.",
  },

  // danger
  "danger.title": { es: "Datos — zona de peligro", en: "Data — danger zone", pt: "Dados — zona de perigo", fr: "Données — zone de danger" },
  "danger.desc": {
    es: "Borra <b>toda</b> la base de datos —proyectos, referencias, revisiones, acciones, chats y ajustes— y la recrea desde cero. La app volverá al asistente de configuración inicial. No se puede deshacer.",
    en: "Erases the <b>entire</b> database —projects, references, reviews, actions, chats and settings— and recreates it from scratch. The app will return to the initial setup wizard. This cannot be undone.",
    pt: "Apaga <b>toda</b> a base de dados —projetos, referências, revisões, ações, chats e definições— e recria-a do zero. A app volta ao assistente de configuração inicial. Não pode ser desfeito.",
    fr: "Efface <b>toute</b> la base de données —projets, références, révisions, actions, discussions et réglages— et la recrée de zéro. L'app reviendra à l'assistant de configuration initial. Irréversible.",
  },
  "danger.reset": { es: "Borrar base de datos y reiniciar", en: "Erase database and restart", pt: "Apagar base de dados e reiniciar", fr: "Effacer la base et redémarrer" },
  "danger.confirm": { es: "¿Seguro? Clic otra vez para confirmar", en: "Sure? Click again to confirm", pt: "A certeza? Clica outra vez para confirmar", fr: "Sûr ? Cliquez encore pour confirmer" },

  // danger confirmation modal
  "danger.modalTitle": { es: "Borrar todos los datos", en: "Erase all data", pt: "Apagar todos os dados", fr: "Effacer toutes les données" },
  "danger.modalWarn": {
    es: "Estás a punto de borrar <b>toda</b> la base de datos —proyectos, referencias, revisiones, acciones, chats y ajustes— y recrearla desde cero. La app volverá al asistente de configuración inicial.",
    en: "You are about to erase the <b>entire</b> database —projects, references, reviews, actions, chats and settings— and recreate it from scratch. The app will return to the initial setup wizard.",
    pt: "Estás prestes a apagar <b>toda</b> a base de dados —projetos, referências, revisões, ações, chats e definições— e recriá-la do zero. A app voltará ao assistente de configuração inicial.",
    fr: "Vous allez effacer <b>toute</b> la base de données —projets, références, révisions, actions, discussions et réglages— et la recréer de zéro. L'app reviendra à l'assistant de configuration initial.",
  },
  "danger.modalIrreversible": { es: "Esta acción no se puede deshacer.", en: "This action cannot be undone.", pt: "Esta ação não pode ser desfeita.", fr: "Cette action est irréversible." },
  "danger.modalCancel": { es: "Cancelar", en: "Cancel", pt: "Cancelar", fr: "Annuler" },
  "danger.modalConfirm": { es: "Borrar todo", en: "Erase everything", pt: "Apagar tudo", fr: "Tout effacer" },
  "danger.modalWait": {
    es: "Podrás confirmar en {s} s",
    en: "You can confirm in {s} s",
    pt: "Poderás confirmar em {s} s",
    fr: "Vous pourrez confirmer dans {s} s",
  },
  "danger.restarting": { es: "Reiniciando…", en: "Restarting…", pt: "A reiniciar…", fr: "Redémarrage…" },
  "danger.recreatedToast": { es: "Base de datos recreada. Reiniciando…", en: "Database recreated. Restarting…", pt: "Base de dados recriada. A reiniciar…", fr: "Base recréée. Redémarrage…" },
  "danger.resetError": { es: "No se pudo reiniciar: ", en: "Could not restart: ", pt: "Não foi possível reiniciar: ", fr: "Impossible de redémarrer : " },

  // IA tabs
  "ia.provider": { es: "Proveedor de IA", en: "AI provider", pt: "Fornecedor de IA", fr: "Fournisseur d'IA" },
  "ia.mcp": { es: "MCP", en: "MCP", pt: "MCP", fr: "MCP" },
  "ia.agent": { es: "Agente", en: "Agent", pt: "Agente", fr: "Agent" },

  // provider card
  "prov.title": { es: "Proveedor de IA", en: "AI provider", pt: "Fornecedor de IA", fr: "Fournisseur d'IA" },
  "prov.provider": { es: "Proveedor", en: "Provider", pt: "Fornecedor", fr: "Fournisseur" },
  "prov.model": { es: "Modelo", en: "Model", pt: "Modelo", fr: "Modèle" },
  "prov.baseUrl": { es: "Base URL", en: "Base URL", pt: "Base URL", fr: "Base URL" },
  "prov.apiKey": { es: "API Key", en: "API Key", pt: "API Key", fr: "API Key" },
  "prov.note": {
    es: "La clave se guarda localmente en SQLite; solo se envía al proveedor configurado.",
    en: "The key is stored locally in SQLite; it is only sent to the configured provider.",
    pt: "A chave é guardada localmente no SQLite; só é enviada para o fornecedor configurado.",
    fr: "La clé est stockée localement dans SQLite ; elle n'est envoyée qu'au fournisseur configuré.",
  },

  // mcp card
  "mcp.title": { es: "Servidores MCP", en: "MCP servers", pt: "Servidores MCP", fr: "Serveurs MCP" },
  "mcp.manage": { es: "Gestionar", en: "Manage", pt: "Gerir", fr: "Gérer" },
  "mcp.empty": { es: "No hay servidores MCP configurados.", en: "No MCP servers configured.", pt: "Sem servidores MCP configurados.", fr: "Aucun serveur MCP configuré." },
  "mcp.connected": { es: "Conectado", en: "Connected", pt: "Ligado", fr: "Connecté" },
  "mcp.disconnected": { es: "Desconectado", en: "Disconnected", pt: "Desligado", fr: "Déconnecté" },

  // agent card
  "agent.title": { es: "Agente de revisión por defecto", en: "Default review agent", pt: "Agente de revisão predefinido", fr: "Agent de révision par défaut" },
  "agent.cli": { es: "CLI de agente", en: "Agent CLI", pt: "CLI do agente", fr: "CLI de l'agent" },
  "agent.path": { es: "Ruta del ejecutable", en: "Executable path", pt: "Caminho do executável", fr: "Chemin de l'exécutable" },
};

const SUPPORTED: Lang[] = ["es", "en", "pt", "fr"];

let activeLang: Lang = "es";

export function setLang(lang: string) {
  activeLang = (SUPPORTED as string[]).includes(lang) ? (lang as Lang) : "es";
}

export function getLang(): Lang {
  return activeLang;
}

/** Resolve a translation key for the active language, falling back to Spanish
 *  then to the key itself if missing. Optional `vars` replaces {name} tokens. */
export function t(key: string, vars?: Record<string, string | number>): string {
  const entry = DICT[key];
  let str: string = entry ? (entry[activeLang] ?? entry.es ?? key) : key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      str = str.replaceAll(`{${k}}`, String(v));
    }
  }
  return str;
}
