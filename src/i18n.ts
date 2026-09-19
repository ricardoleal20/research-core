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
  "missions.loadRunsError": { es: "No se pudieron cargar las ejecuciones: ", en: "Could not load runs: ", pt: "Não foi possível carregar as execuções: ", fr: "Impossible de charger les exécutions : " },

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
