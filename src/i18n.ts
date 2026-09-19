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

  // security
  "sec.security": { es: "Seguridad y contraseña", en: "Security & password", pt: "Segurança e palavra-passe", fr: "Sécurité et mot de passe" },
  "sec.lockPolicy": { es: "Bloqueo de acceso", en: "Access lock", pt: "Bloqueio de acesso", fr: "Verrouillage d'accès" },
  "sec.policyNever": { es: "Nunca", en: "Never", pt: "Nunca", fr: "Jamais" },
  "sec.policyOnLaunch": { es: "Al abrir la app", en: "On app launch", pt: "Ao abrir a app", fr: "À l'ouverture" },
  "sec.policyIdle": { es: "Tras X minutos de inactividad", en: "After X minutes idle", pt: "Após X minutos inativo", fr: "Après X min d'inactivité" },
  "sec.policySensitive": { es: "Antes de acciones sensibles", en: "Before sensitive actions", pt: "Antes de ações sensíveis", fr: "Avant actions sensibles" },
  "sec.idleMin": { es: "Minutos de inactividad", en: "Idle minutes", pt: "Minutos de inatividade", fr: "Minutes d'inactivité" },
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

  // trust center (Story 2.4, FR-5): dials, ceilings, kill switch, providers, targets
  "sec.trust": { es: "Confianza", en: "Trust", pt: "Confiança", fr: "Confiance" },
  "trust.autonomy": { es: "Autonomía", en: "Autonomy", pt: "Autonomia", fr: "Autonomie" },
  "trust.autonomySub": { es: "observar → sugerir → actuar con recibos", en: "watch → suggest → act-with-receipts", pt: "observar → sugerir → agir com recibos", fr: "observer → suggérer → agir avec reçus" },
  "trust.global": { es: "Predeterminado global", en: "Global default", pt: "Padrão global", fr: "Prédéfini global" },
  "trust.mostRestrictive": { es: "la regla más restrictiva gana", en: "most restrictive wins", pt: "a regra mais restritiva vence", fr: "la règle la plus restrictive l'emporte" },
  "trust.targetsFootnote": { es: "Los objetivos de cómputo tienen su propio dial abajo", en: "Compute targets carry their own dial below", pt: "Os alvos de computação têm o próprio dial abaixo", fr: "Les cibles de calcul ont leur propre réglage ci-dessous" },
  "trust.watch": { es: "observar", en: "watch", pt: "observar", fr: "observer" },
  "trust.suggest": { es: "sugerir", en: "suggest", pt: "sugerir", fr: "suggérer" },
  "trust.act": { es: "con recibos", en: "act w/ receipts", pt: "com recibos", fr: "avec reçus" },
  "trust.spend": { es: "Gasto", en: "Spend", pt: "Gasto", fr: "Dépense" },
  "trust.spendSub": { es: "aplicado de forma estricta — el despacho se rechaza al llegar al techo", en: "hard-enforced, not advisory — dispatch is refused at the ceiling", pt: "aplicado de forma estrita — o despacho é recusado ao chegar ao teto", fr: "appliqué strictement — l'envoi est refusé au plafond" },
  "trust.monthlyCeiling": { es: "Techo mensual", en: "Monthly ceiling", pt: "Teto mensal", fr: "Plafond mensuel" },
  "trust.perRunCeiling": { es: "Techo por corrida", en: "Per-run ceiling", pt: "Teto por execução", fr: "Plafond par exécution" },
  "trust.hard": { es: "estricto", en: "hard", pt: "estrito", fr: "strict" },
  "trust.current": { es: "actual", en: "current", pt: "atual", fr: "actuel" },
  "trust.of": { es: "de", en: "of", pt: "de", fr: "sur" },
  "trust.lastRun": { es: "última corrida", en: "last run", pt: "última execução", fr: "dernière exécution" },
  "trust.unset": { es: "sin techo", en: "no ceiling", pt: "sem teto", fr: "sans plafond" },
  "trust.kill": { es: "Interruptor de apagado", en: "Kill switch", pt: "Interruptor de desligamento", fr: "Interrupteur d'arrêt" },
  "trust.autonomousWork": { es: "Trabajo autónomo", en: "Autonomous work", pt: "Trabalho autônomo", fr: "Travail autonome" },
  "trust.killOn": { es: "Activado — los agentes corren dentro de los límites", en: "ON — agents may run within limits", pt: "Ativado — os agentes executam dentro dos limites", fr: "Activé — les agents s'exécutent dans les limites" },
  "trust.killOff": { es: "Detenido — todo despacho se rechaza hasta reanudar", en: "Stopped — every dispatch is refused until resumed", pt: "Parado — todo despacho é recusado até retomar", fr: "Arrêté — tout envoi est refusé jusqu'à reprise" },
  "trust.running": { es: "en marcha", en: "running", pt: "em marcha", fr: "en marche" },
  "trust.killed": { es: "detenido", en: "stopped", pt: "parado", fr: "arrêté" },
  "trust.heartbeats": { es: "latidos cada 30 s", en: "heartbeats every 30 s", pt: "batimentos a cada 30 s", fr: "pulsations toutes les 30 s" },
  "trust.heartbeatsOff": { es: "latidos suspendidos", en: "heartbeats suspended", pt: "batimentos suspensos", fr: "pulsations suspendues" },
  "trust.healthy": { es: "sano", en: "healthy", pt: "saudável", fr: "sain" },
  "trust.confirmStop": { es: "Confirmar", en: "Confirm stop", pt: "Confirmar", fr: "Confirmer l'arrêt" },
  "trust.confirmStopDesc": { es: "Detiene todo el trabajo autónomo de inmediato.", en: "Stops all autonomous work immediately.", pt: "Para todo o trabalho autônomo imediatamente.", fr: "Arrête immédiatement tout travail autonome." },
  "trust.cancel": { es: "Cancelar", en: "Cancel", pt: "Cancelar", fr: "Annuler" },
  "trust.resume": { es: "Reanudar", en: "Resume", pt: "Retomar", fr: "Reprendre" },
  "trust.killedToast": { es: "Runtime detenido — todo despacho se rechaza", en: "Runtime killed — every dispatch is refused", pt: "Runtime parado — todo despacho é recusado", fr: "Runtime arrêté — tout envoi est refusé" },
  "trust.resumedToast": { es: "Runtime reanudado — el despacho está restaurado", en: "Runtime resumed — dispatch is restored", pt: "Runtime retomado — o despacho está restaurado", fr: "Runtime repris — l'envoi est restauré" },
  "trust.providers": { es: "Proveedores", en: "Providers", pt: "Provedores", fr: "Fournisseurs" },
  "trust.providersSub": { es: "Traiga su propia clave — viven en su llavero y nunca se muestran", en: "BYOK — keys live in your Keychain and are never shown", pt: "BYOK — as chaves vivem no seu chaveiro e nunca são exibidas", fr: "BYOK — les clés vivent dans votre trousseau et ne sont jamais affichées" },
  "trust.configured": { es: "configurada", en: "configured", pt: "configurada", fr: "configurée" },
  "trust.noKey": { es: "sin clave", en: "no key", pt: "sem chave", fr: "sans clé" },
  "trust.keyInKeychain": { es: "clave en el llavero, nunca se muestra", en: "key in Keychain, never shown", pt: "chave no chaveiro, nunca exibida", fr: "clé dans le trousseau, jamais affichée" },
  "trust.targets": { es: "Objetivos de cómputo", en: "Compute targets", pt: "Alvos de computação", fr: "Cibles de calcul" },
  "trust.targetsSub": { es: "hosts fuera de la lista se rechazan", en: "hosts outside the allowlist are refused", pt: "hosts fora da lista permitida são recusados", fr: "les hôtes hors liste sont refusés" },
  "trust.saveError": { es: "Error al guardar: ", en: "Error saving: ", pt: "Erro ao guardar: ", fr: "Erreur d'enregistrement : " },
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
    es: "La clave se guarda en el llavero del sistema (keychain), nunca en la base de datos; solo se envía al proveedor configurado.",
    en: "The key is stored in the OS keychain — never in the database — and is only sent to the configured provider.",
    pt: "A chave é guardada no chaveiro do sistema (keychain), nunca na base de dados; só é enviada ao fornecedor configurado.",
    fr: "La clé est conservée dans le trousseau du système (keychain), jamais dans la base de données ; elle n'est envoyée qu'au fournisseur configuré.",
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

  // missions — question box + mission composer (Story 1.3)
  "missions.title": { es: "Misiones", en: "Missions", pt: "Missões", fr: "Missions" },
  "missions.greeting": { es: "¿Qué te pregunta?", en: "What are you wondering?", pt: "O que te pergunta?", fr: "Qu'est-ce que ça te demande ?" },
  "missions.questionPh": { es: "Me pregunto si X se sostiene…", en: "I wonder if X holds up…", pt: "Será que X se sustenta…", fr: "Je me demande si X tient debout…" },
  "missions.turnIntoMission": { es: "Convertir en misión", en: "Turn into a mission", pt: "Converter em missão", fr: "Convertir en mission" },
  "missions.composer.title": { es: "Nueva misión", en: "New mission", pt: "Nova missão", fr: "Nouvelle mission" },
  "missions.composer.question": { es: "Pregunta", en: "Question", pt: "Pergunta", fr: "Question" },
  "missions.stopCondition": { es: "Condición de paro", en: "Stop condition", pt: "Condição de paragem", fr: "Condition d'arrêt" },
  "missions.stopConditionPh": { es: "Cuándo debe terminar la misión — presupuesto, rondas o fecha", en: "When the mission must end — budget, rounds, or date", pt: "Quando a missão deve terminar — orçamento, rondas ou data", fr: "Quand la mission doit s'arrêter — budget, tours ou date" },
  "missions.successCriterion": { es: "Criterio de éxito (falsable)", en: "Success criterion (falsifiable)", pt: "Critério de sucesso (falsificável)", fr: "Critère de succès (falsifiable)" },
  "missions.successCriterionPh": { es: "Qué resultado, si se cumple, da la misión por buena", en: "What result, if met, proves the mission done", pt: "Que resultado, se cumprido, dá a missão por boa", fr: "Quel résultat, s'il est atteint, prouve la mission" },
  "missions.required": { es: "Requerido para lanzar", en: "Required to launch", pt: "Necessário para lançar", fr: "Requis pour lancer" },
  "missions.autonomy": { es: "Autonomía", en: "Autonomy", pt: "Autonomia", fr: "Autonomie" },
  "missions.autonomy.watch": { es: "Observar", en: "Watch", pt: "Observar", fr: "Observer" },
  "missions.autonomy.suggest": { es: "Sugerir", en: "Suggest", pt: "Sugerir", fr: "Suggérer" },
  "missions.autonomy.act_with_receipts": { es: "Actuar con recibos", en: "Act with receipts", pt: "Agir com recibos", fr: "Agir avec reçus" },
  "missions.autonomyDesc.watch": { es: "El agente solo mira y reporta; nada se despacha.", en: "The agent only watches and reports; nothing is dispatched.", pt: "O agente só observa e relata; nada é despachado.", fr: "L'agent observe et rapporte ; rien n'est déclenché." },
  "missions.autonomyDesc.suggest": { es: "El agente propone; tú decides cada paso.", en: "The agent proposes; you decide each step.", pt: "O agente propõe; tu decides cada passo.", fr: "L'agent propose ; tu décides de chaque étape." },
  "missions.autonomyDesc.act_with_receipts": { es: "El agente actúa y deja recibo de cada acción.", en: "The agent acts and leaves a receipt for every action.", pt: "O agente age e deixa um recibo de cada ação.", fr: "L'agent agit et laisse un reçu pour chaque action." },
  "missions.autonomyNote": { es: "La autonomía gobierna el despacho; toda mutación siempre es una propuesta que tú fusionas.", en: "Autonomy governs dispatch; every mutation is still a proposal you merge.", pt: "A autonomia governa o despacho; toda mutação continua a ser uma proposta que tu fundes.", fr: "L'autonomie régit le déclenchement ; toute mutation reste une proposition que tu fusionnes." },
  "missions.spendCeiling": { es: "Techo de gasto", en: "Spend ceiling", pt: "Teto de gasto", fr: "Plafond de dépense" },
  "missions.spendCeilingHint": { es: "Límite duro: el despacho se rehúsa al pasarlo.", en: "Hard limit: dispatch is refused past it.", pt: "Limite rígido: o despacho é recusado ao ultrapassá-lo.", fr: "Limite stricte : au-delà, le déclenchement est refusé." },
  "missions.launch": { es: "Lanzar misión", en: "Launch mission", pt: "Lançar missão", fr: "Lancer la mission" },
  "missions.landing": { es: "Lanzando…", en: "Launching…", pt: "A lançar…", fr: "Lancement…" },
  "missions.cancel": { es: "Cancelar", en: "Cancel", pt: "Cancelar", fr: "Annuler" },
  "missions.active": { es: "Activa", en: "Active", pt: "Ativa", fr: "Active" },
  "missions.missionOf": { es: "misión", en: "mission", pt: "missão", fr: "mission" },
  "missions.createError": { es: "No se pudo crear la misión: ", en: "Could not create the mission: ", pt: "Não foi possível criar a missão: ", fr: "Impossible de créer la mission : " },
  "missions.loadError": { es: "No se pudieron cargar las misiones: ", en: "Could not load missions: ", pt: "Não foi possível carregar as missões: ", fr: "Impossible de charger les missions : " },

  // missions home — Story 1.4 (progressive disclosure, status, spend, runs)
  "missions.empty": {
    es: "Tu primera misión empieza con una pregunta",
    en: "Your first mission starts with a question",
    pt: "A tua primeira missão começa com uma pergunta",
    fr: "Ta première mission commence par une question",
  },
  "missions.emptyHint": {
    es: "Escríbela arriba y conviértela en misión. Nada más que hacer por ahora.",
    en: "Type it above and turn it into a mission. Nothing else to do for now.",
    pt: "Escreve-a acima e converte-a em missão. Nada mais a fazer por agora.",
    fr: "Écris-la ci-dessus et convertis-la en mission. Rien d'autre à faire pour l'instant.",
  },
  "missions.status.active": { es: "Activa", en: "Active", pt: "Ativa", fr: "Active" },
  "missions.status.awaiting_review": { es: "En revisión", en: "Awaiting review", pt: "Em revisão", fr: "En révision" },
  "missions.status.completed": { es: "Completada", en: "Completed", pt: "Concluída", fr: "Terminée" },
  "missions.status.stopped": { es: "Detenida", en: "Stopped", pt: "Parada", fr: "Arrêtée" },
  "missions.status.failed": { es: "Fallida", en: "Failed", pt: "Falhada", fr: "Échouée" },
  "missions.spend": { es: "Gasto", en: "Spend", pt: "Gasto", fr: "Dépense" },
  "missions.spendOf": { es: "de", en: "of", pt: "de", fr: "sur" },
  "missions.spendBlocked": {
    es: "Techo alcanzado — el despacho se rehúsa",
    en: "Ceiling reached — dispatch refused",
    pt: "Teto atingido — o despacho é recusado",
    fr: "Plafond atteint — déclenchement refusé",
  },
  "missions.runs": { es: "Ejecuciones", en: "Runs", pt: "Execuções", fr: "Exécutions" },
  "missions.showRuns": { es: "Ver ejecuciones", en: "Show runs", pt: "Ver execuções", fr: "Voir les exécutions" },
  "missions.hideRuns": { es: "Ocultar ejecuciones", en: "Hide runs", pt: "Ocultar execuções", fr: "Masquer les exécutions" },
  "missions.runsEmpty": {
    es: "Aún no hay ejecuciones para esta misión.",
    en: "No runs for this mission yet.",
    pt: "Ainda não há execuções para esta missão.",
    fr: "Pas encore d'exécutions pour cette mission.",
  },

  // morning digest (Story 2.3, FR-4): the Night Shift result, skimmable in
  // ninety seconds — one verdict per mission, spend vs ceiling, honest
  // failure rows, and the dead-man alert row.
  "digest.title": { es: "Resumen Matutino", en: "Morning Digest", pt: "Resumo Matinal", fr: "Résumé du Matin" },
  "digest.micro": {
    es: "Resultado del Turno Nocturno",
    en: "Night Shift result",
    pt: "Resultado do Turno Noturno",
    fr: "Résultat du poste de nuit",
  },
  "digest.nightShift": { es: "Turno Nocturno", en: "Night Shift", pt: "Turno Noturno", fr: "Poste de nuit" },
  "digest.ofCeiling": { es: "de {ceiling} de techo", en: "of {ceiling} ceiling", pt: "de {ceiling} de teto", fr: "sur un plafond de {ceiling}" },
  "digest.outcome.no_runs": { es: "sin corridas", en: "no runs", pt: "sem corridas", fr: "aucune exécution" },
  "digest.outcome.all_finished": { es: "todo terminó", en: "all finished", pt: "tudo terminado", fr: "tout est terminé" },
  "digest.outcome.partial_success": { es: "éxito parcial", en: "partial success", pt: "êxito parcial", fr: "succès partiel" },
  "digest.outcome.all_failed": { es: "todo falló", en: "all failed", pt: "tudo falhou", fr: "tout a échoué" },
  "digest.verdict.ok": {
    es: "{runs} corrida(s) · {proposals} propuesta(s) en cuarentena",
    en: "{runs} run(s) · {proposals} proposal(s) pending",
    pt: "{runs} corrida(s) · {proposals} proposta(s) pendente(s)",
    fr: "{runs} exécution(s) · {proposals} proposition(s) en attente",
  },
  "digest.verdict.ceiling": {
    es: "techo de gasto alcanzado · sin más corridas",
    en: "cost ceiling reached · no further runs",
    pt: "teto de gasto alcançado · sem mais corridas",
    fr: "plafond atteint · plus d'exécutions",
  },
  "digest.verdict.failed": {
    es: "corrida fallida: {reason} · recibo conservado",
    en: "run failed: {reason} · receipt kept",
    pt: "corrida falhou: {reason} · recibo guardado",
    fr: "exécution échouée : {reason} · reçu conservé",
  },
  "digest.verdict.criterion": {
    es: "criterio cumplido",
    en: "criterion met",
    pt: "critério cumprido",
    fr: "critère rempli",
  },
  "digest.receipts": { es: "recibos", en: "receipts", pt: "recibos", fr: "reçus" },
  "digest.alertLabel": { es: "interruptor de hombre muerto", en: "dead-man switch", pt: "interruptor de homem morto", fr: "interrupteur d'homme mort" },
  "digest.alertBody": {
    es: "La corrida {runId} murió con un latido vencido a las {time} — reiniciada",
    en: "Run {runId} died silently at {time} — restarted",
    pt: "A corrida {runId} morreu em silêncio às {time} — reiniciada",
    fr: "L'exécution {runId} est morte en silence à {time} — redémarrée",
  },
  "digest.rowsNote": {
    es: "los recibos se abren en la vista de corridas",
    en: "receipts open in the run view",
    pt: "os recibos abrem na vista de corridas",
    fr: "les reçus s'ouvrent dans la vue des exécutions",
  },
  "digest.foot": {
    es: "{rows} filas · {alerts} alerta(s) · generado por el Turno Nocturno",
    en: "{rows} rows · {alerts} alert(s) · generated by Night Shift",
    pt: "{rows} linhas · {alerts} alerta(s) · gerado pelo Turno Noturno",
    fr: "{rows} lignes · {alerts} alerte(s) · généré par le poste de nuit",
  },
  "digest.empty": {
    es: "Sin corridas anoche — nada que reportar.",
    en: "No runs last night — nothing to report.",
    pt: "Sem corridas ontem à noite — nada a reportar.",
    fr: "Aucune exécution cette nuit — rien à signaler.",
  },
  "digest.runNow": { es: "Ejecutar el Turno Nocturno", en: "Run Night Shift now", pt: "Executar o Turno Noturno", fr: "Lancer le poste de nuit" },
  "digest.running": { es: "Ejecutando…", en: "Running…", pt: "Executando…", fr: "En cours…" },
  "digest.loadError": {
    es: "No se pudo cargar el resumen: ",
    en: "Could not load the digest: ",
    pt: "Não foi possível carregar o resumo: ",
    fr: "Impossible de charger le résumé : ",
  },
  "missions.schedule": { es: "Horario del Turno Nocturno", en: "Night Shift schedule", pt: "Horário do Turno Noturno", fr: "Horaires du poste de nuit" },
  "missions.scheduleHint": {
    es: "off lo desactiva · daily-HH:MM lo ejecuta cada noche",
    en: "off disables it · daily-HH:MM runs it nightly",
    pt: "off desativa · daily-HH:MM executa toda noite",
    fr: "off le désactive · daily-HH:MM l'exécute chaque nuit",
  },
  "missions.scheduleSave": { es: "Guardar horario", en: "Save schedule", pt: "Guardar horário", fr: "Enregistrer" },
  "missions.scheduleSaved": { es: "Horario guardado", en: "Schedule saved", pt: "Horário guardado", fr: "Horaires enregistrés" },
  "missions.scheduleError": {
    es: "No se pudo guardar el horario: ",
    en: "Could not save the schedule: ",
    pt: "Não foi possível guardar o horário: ",
    fr: "Impossible d'enregistrer les horaires : ",
  },

  "missions.loadRunsError": { es: "No se pudieron cargar las ejecuciones: ", en: "Could not load runs: ", pt: "Não foi possível carregar as execuções: ", fr: "Impossible de charger les exécutions : " },

  // agent roles — Story 2.1 (drafter + critic, each with provider+model;
  // the different-model critic rule, NFR-3, surfaces inline)
  "missions.roles.title": { es: "Roles de agente", en: "Agent roles", pt: "Papéis de agente", fr: "Rôles d'agent" },
  "missions.roles.drafter": { es: "Redactor", en: "Drafter", pt: "Redator", fr: "Rédacteur" },
  "missions.roles.critic": { es: "Crítico", en: "Critic", pt: "Crítico", fr: "Critique" },
  "missions.roles.provider": { es: "Proveedor", en: "Provider", pt: "Provedor", fr: "Fournisseur" },
  "missions.roles.model": { es: "Modelo", en: "Model", pt: "Modelo", fr: "Modèle" },
  "missions.roles.hint": {
    es: "Cada rol corre con su propio proveedor y modelo. Sin clave configurada, todo corre simulado.",
    en: "Each role runs on its own provider and model. With no key configured, everything runs simulated.",
    pt: "Cada papel roda com seu próprio provedor e modelo. Sem chave configurada, tudo roda simulado.",
    fr: "Chaque rôle tourne avec son propre fournisseur et modèle. Sans clé configurée, tout tourne en simulé.",
  },
  "missions.roles.sameModel": {
    es: "El crítico no puede usar el mismo proveedor y modelo que el redactor: nunca un algoritmo calificando su propia tarea. Elige un modelo distinto.",
    en: "The critic cannot use the same provider and model as the drafter: never one algorithm grading its own homework. Pick a different model.",
    pt: "O crítico não pode usar o mesmo provedor e modelo que o redator: nunca um algoritmo avaliando o próprio trabalho. Escolha um modelo diferente.",
    fr: "Le critique ne peut pas utiliser le même fournisseur et modèle que le rédacteur : jamais un algorithme notant son propre travail. Choisissez un modèle différent.",
  },

  // hypotheses — Story 1.5 (board, lifecycle chips, relations, audit stamps)
  "hyp.show": { es: "Hipótesis", en: "Hypotheses", pt: "Hipóteses", fr: "Hypothèses" },
  "hyp.hide": { es: "Ocultar hipótesis", en: "Hide hypotheses", pt: "Ocultar hipóteses", fr: "Masquer les hypothèses" },
  "hyp.board": { es: "Tablero de hipótesis", en: "Hypothesis board", pt: "Quadro de hipóteses", fr: "Tableau des hypothèses" },
  "hyp.empty": {
    es: "Aún no hay hipótesis. La misión propondrá la primera.",
    en: "No hypotheses yet. The mission will propose its first.",
    pt: "Ainda não há hipóteses. A missão proporá a primeira.",
    fr: "Pas encore d'hypothèses. La mission proposera la première.",
  },
  "hyp.statement": { es: "Enunciado", en: "Statement", pt: "Enunciado", fr: "Énoncé" },
  "hyp.statementPh": {
    es: "Un enunciado falsable de esta misión…",
    en: "A falsifiable statement from this mission…",
    pt: "Um enunciado falsificável desta missão…",
    fr: "Un énoncé falsifiable de cette mission…",
  },
  "hyp.add": { es: "Añadir hipótesis", en: "Add hypothesis", pt: "Adicionar hipótese", fr: "Ajouter une hypothèse" },
  "hyp.createError": { es: "No se pudo crear la hipótesis: ", en: "Could not create the hypothesis: ", pt: "Não foi possível criar a hipótese: ", fr: "Impossible de créer l'hypothèse : " },
  "hyp.loadError": { es: "No se pudieron cargar las hipótesis: ", en: "Could not load hypotheses: ", pt: "Não foi possível carregar as hipóteses: ", fr: "Impossible de charger les hypothèses : " },
  "hyp.basis": { es: "Base de la transición", en: "Transition basis", pt: "Base da transição", fr: "Base de la transition" },
  "hyp.basisPh": {
    es: "Por qué este cambio — ejecución, ancla o nota",
    en: "Why this change — run, pin, or note",
    pt: "Porquê esta mudança — execução, âncora ou nota",
    fr: "Pourquoi ce changement — exécution, ancre ou note",
  },
  "hyp.basisRequired": {
    es: "Toda transición nombra su base — requerida",
    en: "Every transition names its basis — required",
    pt: "Toda transição nomeia a sua base — necessária",
    fr: "Toute transition nomme sa base — requise",
  },
  "hyp.transitionError": { es: "Transición rechazada: ", en: "Transition refused: ", pt: "Transição recusada: ", fr: "Transition refusée : " },
  "hyp.relate": { es: "Relacionar", en: "Relate", pt: "Relacionar", fr: "Relier" },
  "hyp.relateTo": { es: "Segunda hipótesis", en: "Second hypothesis", pt: "Segunda hipótese", fr: "Seconde hypothèse" },
  "hyp.relationKind": { es: "Tipo de relación", en: "Relation kind", pt: "Tipo de relação", fr: "Type de relation" },
  "hyp.relationPick": { es: "Elige una hipótesis…", en: "Pick a hypothesis…", pt: "Escolhe uma hipótese…", fr: "Choisis une hypothèse…" },
  "hyp.relationError": { es: "No se pudo crear la relación: ", en: "Could not create the relation: ", pt: "Não foi possível criar a relação: ", fr: "Impossible de créer la relation : " },
  "hyp.status.proposed": { es: "Propuesta", en: "Proposed", pt: "Proposta", fr: "Proposée" },
  "hyp.status.testing": { es: "En prueba", en: "Testing", pt: "Em teste", fr: "En test" },
  "hyp.status.supported": { es: "Soportada", en: "Supported", pt: "Sustentada", fr: "Soutenue" },
  "hyp.status.refuted": { es: "Refutada", en: "Refuted", pt: "Refutada", fr: "Réfutée" },
  "hyp.status.revised": { es: "Revisada", en: "Revised", pt: "Revista", fr: "Révisée" },
  // relation chip labels (FR-2.3 vocabulary): outgoing / incoming per kind;
  // supports_the_same_claim is symmetric — one label both directions.
  "hyp.rel.contradicts.out": { es: "contradice a", en: "contradicts", pt: "contradiz", fr: "contredit" },
  "hyp.rel.contradicts.in": { es: "contradicha por", en: "contradicted-by", pt: "contraditada por", fr: "contredite par" },
  "hyp.rel.extends.out": { es: "extiende a", en: "extends", pt: "estende", fr: "étend" },
  "hyp.rel.extends.in": { es: "extendida por", en: "extended-by", pt: "estendida por", fr: "étendue par" },
  "hyp.rel.specializes.out": { es: "especializa a", en: "specializes", pt: "especializa", fr: "spécialise" },
  "hyp.rel.specializes.in": { es: "especializada por", en: "specialized-by", pt: "especializada por", fr: "spécialisée par" },
  "hyp.rel.supports_the_same_claim": {
    es: "sostiene la misma afirmación que",
    en: "supports-the-same-claim-as",
    pt: "sustenta a mesma alegação que",
    fr: "soutient la même affirmation que",
  },
  // relation-kind picker labels (fuller forms of the same vocabulary)
  "hyp.relKind.contradicts": { es: "contradice", en: "contradicts", pt: "contradiz", fr: "contredit" },
  "hyp.relKind.extends": { es: "extiende", en: "extends", pt: "estende", fr: "étend" },
  "hyp.relKind.specializes": { es: "especializa", en: "specializes", pt: "especializa", fr: "spécialise" },
  "hyp.relKind.supports_the_same_claim": { es: "sostiene la misma afirmación", en: "supports the same claim", pt: "sustenta a mesma alegação", fr: "soutient la même affirmation" },

  // evidence pins (FR-3, Story 1.7): claims on hypothesis cards, the
  // unpinned amber chip (FR-3.4), and the citation pin anatomy (source
  // icon + author-year + confidence dot labeled with the assessing model).
  "ev.claims": { es: "Afirmaciones de IA", en: "AI claims", pt: "Alegações de IA", fr: "Affirmations IA" },
  "ev.empty": { es: "Sin afirmaciones aún — añade un fragmento de la salida de IA.", en: "No claims yet — attach a fragment of the AI output.", pt: "Sem alegações — anexa um fragmento da saída de IA.", fr: "Pas encore d'affirmations — ajoute un fragment de la sortie IA." },
  "ev.addPh": { es: "Fragments de afirmaciones de IA…", en: "AI claim fragments…", pt: "Fragmentos de alegações de IA…", fr: "Fragments d'affirmations IA…" },
  "ev.add": { es: "Añadir afirmación", en: "Add claim", pt: "Adicionar alegação", fr: "Ajouter l'affirmation" },
  "ev.claimLabel": { es: "Afirmación", en: "Claim", pt: "Alegação", fr: "Affirmation" },
  "ev.unpinned": { es: "sin ancla", en: "unpinned", pt: "sem âncora", fr: "sans ancrage" },
  "ev.pinnedTo": { es: "anclada a", en: "pinned to", pt: "ancorada em", fr: "ancrée à" },
  "ev.pin": { es: "Anclar a cita", en: "Pin to citation", pt: "Ancorar a citação", fr: "Ancrer à la citation" },
  "ev.pinning": { es: "Anclando…", en: "Pinning…", pt: "A ancorar…", fr: "Ancrage…" },
  "ev.pinClaim": { es: "Anclar afirmación", en: "Pin claim", pt: "Ancorar alegação", fr: "Ancrer l'affirmation" },
  "ev.refPick": { es: "Elige una referencia…", en: "Pick a reference…", pt: "Escolhe uma referência…", fr: "Choisis une référence…" },
  "ev.excerpt": { es: "Extracto citado", en: "Quoted excerpt", pt: "Excerto citado", fr: "Extrait cité" },
  "ev.excerptPh": { es: "Pega el pasaje exacto que sostiene la afirmación…", en: "Paste the exact passage supporting the claim…", pt: "Cola a passagem exata que sustenta a alegação…", fr: "Collez le passage exact qui soutient l'affirmation…" },
  "ev.excerptPrefill": { es: "Se rellena con el texto de la afirmación — confírmalo o edítalo.", en: "Prefilled with the claim text — confirm or edit it.", pt: "Pré-preenchido com o texto da alegação — confirma ou edita.", fr: "Prérempli avec le texte de l'affirmation — confirmez ou modifiez." },
  "ev.confidence": { es: "Confianza (0–1)", en: "Confidence (0–1)", pt: "Confiança (0–1)", fr: "Confiance (0–1)" },
  "ev.model": { es: "Modelo que evalúa", en: "Assessing model", pt: "Modelo avaliador", fr: "Modèle évaluateur" },
  "ev.modelPh": { es: "p. ej. GLM-5.3", en: "e.g. GLM-5.3", pt: "ex. GLM-5.3", fr: "ex. GLM-5.3" },
  "ev.confirmExcerpt": { es: "Confirmar extracto", en: "Confirm excerpt", pt: "Confirmar excerto", fr: "Confirmer l'extrait" },
  "ev.digest": { es: "sha-256", en: "sha-256", pt: "sha-256", fr: "sha-256" },
  "ev.excerptQuoted": { es: "Extracto citado", en: "Quoted excerpt", pt: "Excerto citado", fr: "Extrait cité" },
  "ev.pinError": { es: "No se pudo anclar: ", en: "Could not pin: ", pt: "Não foi possível ancorar: ", fr: "Impossible d'ancrer : " },
  "ev.claimError": { es: "No se pudo registrar la afirmación: ", en: "Could not register the claim: ", pt: "Não foi possível registar a alegação: ", fr: "Impossible d'enregistrer l'affirmation : " },
  "ev.loadError": { es: "No se pudo cargar el evidencia: ", en: "Could not load evidence: ", pt: "Não foi possível carregar o evidência: ", fr: "Impossible de charger les preuves : " },
  "ev.required": { es: "El extracto, la referencia y el modelo son obligatorios.", en: "Excerpt, reference, and model are required.", pt: "Excerto, referência e modelo são obrigatórios.", fr: "Extrait, référence et modèle sont requis." },
  "ev.claimRequired": { es: "La afirmación no puede quedar vacía.", en: "The claim text is required.", pt: "O texto da alegação é obrigatório.", fr: "Le texte de l'affirmation est requis." },

  // numerical pins (FR-3.3, Story 1.8) — the artifact anatomy: chart icon,
  // artifact name, mono digest fragment.
  "ev.pinnedToArtifact": { es: "anclada a artefacto", en: "pinned to artifact", pt: "ancorada a artefacto", fr: "ancrée à l'artefact" },
  "ev.pinArtifact": { es: "Anclar a artefacto", en: "Pin to artifact", pt: "Ancorar a artefacto", fr: "Ancrer à l'artefact" },
  "ev.artifact": { es: "Artefacto numérico", en: "Numerical artifact", pt: "Artefacto numérico", fr: "Artefact numérique" },
  "ev.artifactPh": { es: "p. ej. runs/007/table-3.csv", en: "e.g. runs/007/table-3.csv", pt: "ex. runs/007/table-3.csv", fr: "ex. runs/007/table-3.csv" },
  "ev.contentQuoted": { es: "Contenido anclado", en: "Anchored content", pt: "Conteúdo ancorado", fr: "Contenu ancré" },

  // board at a glance (FR-2.4, Story 1.8): all hypotheses, all states,
  // one view — with the checkpoint control in its header.
  "board.title": { es: "El tablero de un vistazo", en: "The board at a glance", pt: "O quadro num relance", fr: "Le tableau en un coup d'œil" },
  "board.sub": { es: "Todas las hipótesis y sus estados, en una sola vista.", en: "Every hypothesis and its state, in one view.", pt: "Todas as hipóteses e os seus estados, numa só vista.", fr: "Toutes les hypothèses et leurs états, en une seule vue." },
  "board.filter": { es: "Filtrar por estado", en: "Filter by status", pt: "Filtrar por estado", fr: "Filtrer par état" },
  "board.all": { es: "Todos los estados", en: "All states", pt: "Todos os estados", fr: "Tous les états" },
  "board.empty": { es: "Aún no hay hipótesis — las misiones propondrán las primeras.", en: "No hypotheses yet — missions will propose the first.", pt: "Ainda não há hipóteses — as missões propõem as primeiras.", fr: "Pas encore d'hypothèses — les missions proposeront les premières." },
  "board.filterEmpty": { es: "Ninguna hipótesis en ese estado.", en: "No hypotheses in that state.", pt: "Nenhuma hipótese nesse estado.", fr: "Aucune hypothèse dans cet état." },
  "board.loadError": { es: "No se pudo cargar el tablero: ", en: "Could not load the board: ", pt: "Não foi possível carregar o quadro: ", fr: "Impossible de charger le tableau : " },
  "board.mission": { es: "Misión", en: "Mission", pt: "Missão", fr: "Mission" },

  // checkpoints (entry point for Story 2.6): the board header control and
  // its empty restore-points list state — presence and UI only.
  "cp.title": { es: "Puntos de control", en: "Checkpoints", pt: "Pontos de controlo", fr: "Points de contrôle" },
  "cp.empty": { es: "Aún no hay puntos de control.", en: "No checkpoints yet.", pt: "Ainda não há pontos de controlo.", fr: "Pas encore de points de contrôle." },

  // quarantine review (Story 2.2, AD-3/AD-13): agent proposals wait here —
  // nothing changes until a human merges; the basis-stale warning variant;
  // decided proposals stay visible with their receipt stamps.
  "quarantine.title": { es: "Revisión pendiente", en: "Pending review", pt: "Revisão pendente", fr: "Révision en attente" },
  "quarantine.sub": {
    es: "Las propuestas del agente esperan aquí — nada cambia hasta que las fusiones.",
    en: "Agent proposals wait here — nothing changes until you merge them.",
    pt: "As propostas do agente esperam aqui — nada muda até as fundires.",
    fr: "Les propositions de l'agent attendent ici — rien ne change tant que vous ne les fusionnez pas.",
  },
  "quarantine.empty": {
    es: "Sin propuestas en espera — los cambios del agente aparecerán aquí.",
    en: "No proposals waiting — agent changes will appear here.",
    pt: "Sem propostas em espera — as mudanças do agente aparecerão aqui.",
    fr: "Aucune proposition en attente — les changements de l'agent apparaîtront ici.",
  },
  "quarantine.pending": { es: "pendientes", en: "pending", pt: "pendentes", fr: "en attente" },
  "quarantine.history": { es: "Historial", en: "History", pt: "Histórico", fr: "Historique" },
  "quarantine.historyEmpty": {
    es: "Nada decidido aún.",
    en: "Nothing decided yet.",
    pt: "Nada decidido ainda.",
    fr: "Rien de décidé pour l'instant.",
  },
  "quarantine.proposes": { es: "propone", en: "proposes", pt: "propõe", fr: "propose" },
  "quarantine.change": { es: "cambio de estado", en: "status change", pt: "mudança de estado", fr: "changement d'état" },
  "quarantine.basis": { es: "base", en: "basis", pt: "base", fr: "base" },
  "quarantine.run": { es: "run", en: "run", pt: "run", fr: "run" },
  "quarantine.basisStale": {
    es: "La entidad cambió desde esta propuesta",
    en: "Entity changed since this proposal was made",
    pt: "A entidade mudou desde esta proposta",
    fr: "L'entité a changé depuis cette proposition",
  },
  "quarantine.approve": { es: "Aprobar", en: "Approve", pt: "Aprovar", fr: "Approuver" },
  "quarantine.forceApprove": { es: "Aprobar de todos modos", en: "Force approve", pt: "Aprovar mesmo assim", fr: "Approuver quand même" },
  "quarantine.reject": { es: "Rechazar", en: "Reject", pt: "Rejeitar", fr: "Rejeter" },
  "quarantine.approving": { es: "Aprobando…", en: "Approving…", pt: "A aprovar…", fr: "Approbation…" },
  "quarantine.rejecting": { es: "Rechazando…", en: "Rejecting…", pt: "A rejeitar…", fr: "Rejet…" },
  "quarantine.actionError": {
    es: "No se pudo decidir la propuesta: ",
    en: "Could not decide the proposal: ",
    pt: "Não foi possível decidir a proposta: ",
    fr: "Impossible de décider la proposition : ",
  },
  "quarantine.loadError": {
    es: "No se pudieron cargar las propuestas: ",
    en: "Could not load proposals: ",
    pt: "Não foi possível carregar as propostas: ",
    fr: "Impossible de charger les propositions : ",
  },
  "quarantine.mergedStale": {
    es: "fusionada sobre una base vencida",
    en: "merged past a stale basis",
    pt: "fundida sobre uma base vencida",
    fr: "fusionnée sur une base périmée",
  },
  "quarantine.supersededNote": {
    es: "reemplazada por la fusión de una propuesta hermana",
    en: "superseded by a sibling proposal's merge",
    pt: "substituída pela fusão de uma proposta irmã",
    fr: "remplacée par la fusion d'une proposition sœur",
  },
  "quarantine.decided": { es: "decidida", en: "decided", pt: "decidida", fr: "décidée" },

  // onboarding — the sixty-second first value (Story 1.9, FR-8.1/8.2): the
  // welcome's two quiet actions and the result moment. Zero jargon beyond
  // the current layer: papers, candidates, one mission with its stop
  // condition and criterion.
  "onb.pasteTitle": { es: "Pega un enlace de arXiv", en: "Paste an arXiv URL", pt: "Cola um link do arXiv", fr: "Colle un lien arXiv" },
  "onb.urlPh": { es: "https://arxiv.org/abs/1706.03762", en: "https://arxiv.org/abs/1706.03762", pt: "https://arxiv.org/abs/1706.03762", fr: "https://arxiv.org/abs/1706.03762" },
  "onb.generate": { es: "Generar", en: "Generate", pt: "Gerar", fr: "Générer" },
  "onb.generating": { es: "Generando…", en: "Generating…", pt: "A gerar…", fr: "Génération…" },
  "onb.back": { es: "Volver", en: "Back", pt: "Voltar", fr: "Retour" },
  "onb.zoteroTitle": { es: "Importa de tu biblioteca Zotero", en: "Import from your Zotero library", pt: "Importa da tua biblioteca Zotero", fr: "Importe depuis votre bibliothèque Zotero" },
  "onb.zoteroHint": {
    es: "Tu biblioteca ya migrada — elige una referencia para empezar.",
    en: "Your already-migrated library — pick a reference to start from.",
    pt: "A tua biblioteca já migrada — escolhe uma referência para começar.",
    fr: "Votre bibliothèque déjà migrée — choisissez une référence pour commencer.",
  },
  "onb.zoteroEmpty": { es: "Aún no hay referencias en la biblioteca.", en: "No references in the library yet.", pt: "Ainda não há referências na biblioteca.", fr: "Pas encore de références dans la bibliothèque." },
  "onb.error": { es: "No se pudo generar: ", en: "Could not generate: ", pt: "Não foi possível gerar: ", fr: "Impossible de générer : " },
  "onb.resultKicker": { es: "De artículo a misión", en: "From paper to mission", pt: "Do artigo à missão", fr: "De l'article à la mission" },
  "onb.resultTitle": { es: "Tu primera misión está lista", en: "Your first mission is ready", pt: "A tua primeira missão está pronta", fr: "Votre première mission est prête" },
  "onb.genTitle": { es: "Generando candidatos de hipótesis", en: "Generating hypothesis candidates", pt: "A gerar candidatos de hipótese", fr: "Génération des candidats hypothèses" },
  "onb.logFetch": { es: "obtener", en: "fetch", pt: "obter", fr: "récupérer" },
  "onb.logClaims": { es: "extraer afirmaciones · {n} candidatos", en: "extract claims · {n} candidates", pt: "extrair afirmações · {n} candidatos", fr: "extraire les affirmations · {n} candidats" },
  "onb.logScore": { es: "medir confianza · {model}", en: "score confidence · {model}", pt: "medir confiança · {model}", fr: "mesurer la confiance · {model}" },
  "onb.candidatesTitle": { es: "Candidatos de hipótesis", en: "Hypothesis candidates", pt: "Candidatos de hipótese", fr: "Candidats hypothèses" },
  "onb.proposed": { es: "propuesta", en: "proposed", pt: "proposta", fr: "proposée" },
  "onb.missionLabel": { es: "Tu primera misión", en: "Your first mission", pt: "A tua primeira missão", fr: "Votre première mission" },
  "onb.stop": { es: "Paro", en: "Stop", pt: "Paragem", fr: "Arrêt" },
  "onb.criterion": { es: "Criterio", en: "Criterion", pt: "Critério", fr: "Critère" },
  "onb.spend": { es: "Gasto", en: "Spend", pt: "Gasto", fr: "Dépense" },
  "onb.ceiling": { es: "techo de {amount} por corrida", en: "{amount} per-run ceiling", pt: "teto de {amount} por corrida", fr: "plafond de {amount} par exécution" },
  "onb.active": { es: "activa", en: "active", pt: "ativa", fr: "active" },
  "onb.continue": { es: "Ir a tu misión", en: "Go to your mission", pt: "Ir para a tua missão", fr: "Aller à votre mission" },
  "onb.simulatedNote": {
    es: "proveedor simulado — sin clave configurada, coste $0.00",
    en: "simulated provider — no key configured, cost $0.00",
    pt: "provedor simulado — sem chave configurada, custo $0.00",
    fr: "fournisseur simulé — aucune clé configurée, coût 0,00 $",
  },
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
