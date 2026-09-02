use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tokio::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Self::migrate(&conn)?;
        Self::seed(&conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                folder TEXT,
                kind TEXT NOT NULL DEFAULT 'paper',
                tags TEXT,
                color TEXT NOT NULL DEFAULT '#3B5BDB',
                chapter_index INTEGER DEFAULT 1,
                chapter_count INTEGER DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS collections (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                name TEXT NOT NULL,
                color TEXT NOT NULL DEFAULT '#3B5BDB',
                created_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS refs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                collection_id TEXT,
                title TEXT NOT NULL,
                authors TEXT,
                year INTEGER,
                venue TEXT,
                doi TEXT,
                url TEXT,
                isbn TEXT,
                attachment TEXT,
                attachment_size INTEGER,
                status TEXT NOT NULL DEFAULT 'unread',
                tags TEXT,
                used INTEGER NOT NULL DEFAULT 0,
                citation_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS ref_usages (
                id TEXT PRIMARY KEY,
                ref_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                location TEXT NOT NULL,
                context TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(ref_id) REFERENCES refs(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS reviews (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                number INTEGER NOT NULL,
                score REAL NOT NULL,
                focus TEXT,
                dims TEXT,
                verdict TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS findings (
                id TEXT PRIMARY KEY,
                review_id TEXT NOT NULL,
                severity TEXT NOT NULL,
                location TEXT,
                text TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(review_id) REFERENCES reviews(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS actions (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                code TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                priority TEXT NOT NULL DEFAULT 'med',
                origin TEXT NOT NULL DEFAULT 'manual',
                due TEXT,
                location TEXT,
                done INTEGER NOT NULL DEFAULT 0,
                done_at TEXT,
                source_review_id TEXT,
                source_finding TEXT,
                notes TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS chats (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'asistente',
                title TEXT NOT NULL,
                preview TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                classify_tag TEXT,
                meta TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(chat_id) REFERENCES chats(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                description TEXT,
                icon TEXT NOT NULL DEFAULT 'check',
                enabled INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL DEFAULT 'judge'
            );
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                transport TEXT NOT NULL DEFAULT 'http',
                command TEXT,
                args TEXT,
                env TEXT,
                url TEXT,
                tags TEXT,
                connected INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    fn seed(conn: &Connection) -> rusqlite::Result<()> {
        // Settings defaults (only if absent)
        let defaults = [
            ("lang", "es"),
            ("local_first", "true"),
            ("sync_zotero", "true"),
            ("cache_pdfs", "false"),
            ("agent_path", "~/.local/bin/claude"),
            ("provider", "openai-compatible"),
            ("base_url", ""),
            ("api_key", ""),
            ("model", "gpt-4o-mini"),
        ];
        for (k, v) in defaults {
            conn.execute(
                "INSERT OR IGNORE INTO settings(key,value) VALUES(?1,?2)",
                params![k, v],
            )?;
        }

        // Agents (idempotent)
        let agents = [
            ("rigor", "Rigor metodológico", "Validez del diseño experimental y supuestos", "check", 1, "judge"),
            ("novelty", "Originalidad", "Contribución novedosa frente al estado del arte", "spark", 1, "judge"),
            ("clarity", "Claridad", "Estructura, redacción y legibilidad general", "pen", 1, "judge"),
            ("repro", "Reproducibilidad", "Código y datos para replicar resultados", "refresh", 0, "judge"),
            ("cite", "Cobertura de citas", "Referencias clave citadas y usadas", "list", 0, "judge"),
            ("orchestrator", "Orquestador", "Coordina jueces y sintetiza la revisión", "brain", 1, "orchestrator"),
        ];
        for (key, name, desc, icon, enabled, kind) in agents {
            conn.execute(
                "INSERT OR IGNORE INTO agents(id,key,name,description,icon,enabled,kind) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![key, key, name, desc, icon, enabled, kind],
            )?;
        }

        // MCP servers (idempotent)
        let mcps = [
            ("zotero", "Zotero Connector", "http", "", "", "", "http://localhost:23919", "refs,search", 1),
            ("arxiv", "arXiv Search", "http", "", "", "", "https://export.arxiv.org/api", "refs,search", 1),
            ("filesystem", "Filesystem", "stdio", "npx", "-y @modelcontextprotocol/server-filesystem ~/Documents", "", "", "code,data", 1),
            ("semantic", "Semantic Scholar", "http", "", "", "", "https://api.semanticscholar.org", "refs", 0),
        ];
        for (id, name, transport, command, args, env, url, tags, connected) in mcps {
            // Upsert: refresh connection-critical fields (command/args/transport)
            // so existing installs pick up seed corrections (e.g. a fixed
            // filesystem path), while preserving the user's connected state.
            conn.execute(
                "INSERT INTO mcp_servers(id,name,transport,command,args,env,url,tags,connected,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET
                   name=excluded.name, transport=excluded.transport,
                   command=excluded.command, args=excluded.args, env=excluded.env,
                   url=excluded.url, tags=excluded.tags",
                params![id, name, transport, command, args, env, url, tags, connected, now()],
            )?;
        }

        // Seed a demo project only if no projects exist
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
        if n == 0 {
            seed_demo(conn)?;
        }
        Ok(())
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn seed_demo(conn: &Connection) -> rusqlite::Result<()> {
    let pid = "proj-tesis-cap2";
    conn.execute(
        "INSERT INTO projects(id,name,folder,kind,tags,color,chapter_index,chapter_count,created_at,updated_at,is_active)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1)",
        params![pid, "Optimización de Modelos de Atención", "~/research/tesis-cap2", "thesis-chapter",
                "transformer,attention,NLP", "#3B5BDB", 2, 5, now(), now()],
    )?;

    // Collections
    let cols = [
        ("col-thesis", "Tesis — Cap. 2", "#3B5BDB"),
        ("col-attn", "Attention Models", "#2F9E6B"),
        ("col-repro", "Reproducibility", "#D98324"),
        ("col-bench", "Benchmarks", "#9B5DE5"),
    ];
    for (id, name, color) in cols {
        conn.execute(
            "INSERT INTO collections(id,project_id,name,color,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![id, pid, name, color, now()],
        )?;
    }

    // Refs (title, authors, year, venue, doi, status, used, citations, collection, tags)
    let refs: &[(&str, &str, i64, &str, &str, &str, i64, i64, &str, &str)] = &[
        ("Attention Is All You Need", "Vaswani et al.", 2017, "NeurIPS", "10.48550/arXiv.1706.03762", "read", 1, 2, "col-attn", "transformer,attention,NLP"),
        ("Neural Machine Translation by Jointly Learning to Align and Translate", "Bahdanau et al.", 2015, "ICLR", "10.48550/arXiv.1409.0473", "read", 1, 1, "col-attn", "attention,NLP"),
        ("A Survey on Large Language Models", "Zhao et al.", 2023, "arXiv", "10.48550/arXiv.2303.18223", "unread", 0, 0, "col-thesis", "survey,LLM"),
        ("Learning Transferable Visual Models From Natural Language", "Radford et al.", 2021, "ICML", "10.48550/arXiv.2103.00020", "unread", 0, 0, "col-bench", "vision,contrastive"),
        ("Scaling Laws for Neural Language Models", "Kaplan et al.", 2020, "arXiv", "10.48550/arXiv.2001.08361", "read", 1, 1, "col-thesis", "scaling,NLP"),
        ("Direct Preference Optimization", "Rafailov et al.", 2023, "NeurIPS", "10.48550/arXiv.2305.18290", "unread", 0, 0, "col-thesis", "RLHF"),
    ];
    for (title, authors, year, venue, doi, status, used, cit, coll, tags) in refs {
        let rid = uid();
        conn.execute(
            "INSERT INTO refs(id,project_id,collection_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![rid, pid, coll, title, authors, year, venue, doi,
                    format!("https://arxiv.org/abs/{}", doi.trim_start_matches("10.48550/arXiv.")),
                    tags, status, used, cit, now()],
        )?;
        if *used > 0 && title == &"Attention Is All You Need" {
            conn.execute("INSERT INTO ref_usages(id,ref_id,project_id,location,context,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![uid(), rid, pid, "§3.2", "Mecanismo de atención — referencia fundacional al multi-head self-attention", now()])?;
            conn.execute("INSERT INTO ref_usages(id,ref_id,project_id,location,context,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![uid(), rid, pid, "§4.1", "Arquitectura del modelo — comparación con enfoques recurrentes previos", now()])?;
        }
    }

    // A review
    conn.execute(
        "INSERT INTO reviews(id,project_id,number,score,focus,dims,verdict,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![uid(), pid, 6, 7.8, "§4.2 + §3",
                r#"[{"name":"Rigor metodológico","score":8.2},{"name":"Claridad","score":7.4}]"#,
                "Buen estado general. El paper muestra rigor metodológico sólido y cobertura de citas completa. Prioridad: añadir varianza a los ablations (§4.2) y justificar el hiperparámetro de learning rate (§3.1).",
                now()],
    )?;

    // Actions
    let actions: &[(&str, &str, &str, &str, &str, &str, i64, &str)] = &[
        ("A-001", "Revisar sección 3.2 — justificar por qué self-attention reemplaza RNNs", "high", "review", "-2d", "§3.2", 0, "El reviewer señaló que la sección 3.2 afirma que self-attention reemplaza la necesidad de recurrencia, pero no justifica la afirmación."),
        ("A-002", "Añadir cita de Bahdanau 2015 en §4.1 sobre attention mechanisms", "high", "asistente", "+1d", "§4.1", 0, ""),
        ("A-003", "Reescribir introducción — foco en gap de literatura", "med", "manual", "5 sep", "§1", 0, ""),
        ("A-004", "Verificar reproducibilidad del experimento baseline (Tabla 2)", "med", "review", "8 sep", "§5", 0, ""),
        ("A-005", "Exportar bibliografía en BibTeX para envío a NeurIPS", "low", "manual", "", "", 0, ""),
        ("A-006", "Acortar abstract a 150 palabras", "med", "asistente", "-3d", "Abstract", 1, ""),
        ("A-007", "Regenerar Figura 1 con datos actualizados", "low", "review", "-5d", "Fig 1", 1, ""),
        ("A-008", "Ampliar related work — añadir 4 refs de scaling laws", "med", "manual", "-7d", "§2", 1, ""),
    ];
    for (code, title, prio, origin, due, loc, done, desc) in actions {
        conn.execute(
            "INSERT INTO actions(id,project_id,code,title,description,priority,origin,due,location,done,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![uid(), pid, code, title, desc, prio, origin, due, loc, done, now()],
        )?;
    }

    // Chats
    let chat1 = uid();
    conn.execute(
        "INSERT INTO chats(id,project_id,kind,title,preview,created_at,updated_at) VALUES(?1,?2,'asistente',?3,?4,?5,?6)",
        params![chat1, pid, "Borrador §3.1 — justificación LR", "«redacta un párrafo conectando…»", now(), now()],
    )?;
    conn.execute("INSERT INTO messages(id,chat_id,role,content,created_at) VALUES(?1,?2,'user',?3,?4)",
        params![uid(), chat1, "Redacta un párrafo conectando la justificación del learning rate con las Scaling Laws de Kaplan 2020. El LR que usé fue 3e-4 con batch 256.", now()])?;
    conn.execute("INSERT INTO messages(id,chat_id,role,content,created_at) VALUES(?1,?2,'agent',?3,?4)",
        params![uid(), chat1, "Aquí va una propuesta para §3.1. Conecta el LR elegido con la relación óptima de Kaplan et al. (2020):\n\nEl learning rate de 3×10⁻⁴ empleado en nuestros experimentos se sitúa dentro del régimen estable previsto por las leyes de escalamiento (Kaplan et al., 2020), donde para un tamaño de lote B = 256 la regla de escalamiento lineal sugiere lr ∝ B manteniendo la estabilidad del entrenamiento.", now()])?;

    let chat2 = uid();
    conn.execute("INSERT INTO chats(id,project_id,kind,title,preview,created_at,updated_at) VALUES(?1,?2,'asistente',?3,?4,?5,?6)",
        params![chat2, pid, "Resumen de Kaplan 2020", "«¿cuál es el aporte principal de…»", now(), now()])?;
    let chat3 = uid();
    conn.execute("INSERT INTO chats(id,project_id,kind,title,preview,created_at,updated_at) VALUES(?1,?2,'review',?3,?4,?5,?6)",
        params![chat3, pid, "Revisión §4.2 + §3", "«Revisa la §4.2, los ablations…»", now(), now()])?;
    conn.execute("INSERT INTO messages(id,chat_id,role,content,classify_tag,created_at) VALUES(?1,?2,'user',?3,NULL,?4)",
        params![uid(), chat3, "Revisa la §4.2, los ablations necesitan intervalos de confianza. También quiero que revises la coherencia del argumento en la §3.", now()])?;
    conn.execute("INSERT INTO messages(id,chat_id,role,content,classify_tag,created_at) VALUES(?1,?2,'agent',?3,'review',?4)",
        params![uid(), chat3, "Entiendo que quieres una revisión enfocada en §4.2 (ablation con intervalos de confianza) y §3 (coherencia del argumento). Voy a activar los jueces Rigor metodológico y Claridad, y procesar el manuscrito + código vinculado. Tarda ~2 min.", now()])?;
    conn.execute("INSERT INTO messages(id,chat_id,role,content,classify_tag,created_at) VALUES(?1,?2,'agent',?3,'review',?4)",
        params![uid(), chat3, "Revisión completa. Puntaje 7.8 / 10 (mejora de +0.6 desde #5). Guardé la revisión y creé 3 actions para atender los hallazgos.", now()])?;

    Ok(())
}

// ---- query helpers returning serde_json::Value ----

pub fn row_to_value(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    use rusqlite::types::ValueRef;
    let stmt: &rusqlite::Statement = r.as_ref();
    let n = stmt.column_count();
    let mut map = serde_json::Map::new();
    for i in 0..n {
        let name = stmt.column_name(i)?.to_string();
        let v = match r.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(i) => Value::from(i),
            ValueRef::Real(f) => Value::from(f),
            ValueRef::Text(t) => {
                Value::from(std::str::from_utf8(t).unwrap_or("").to_string())
            }
            ValueRef::Blob(b) => Value::from(format!("<blob {} bytes>", b.len())),
        };
        map.insert(name, v);
    }
    Ok(Value::Object(map))
}

pub fn query_all<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    p: P,
) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, row_to_value)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn query_one<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    p: P,
) -> rusqlite::Result<Option<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map(p, row_to_value)?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn get_setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get::<_, String>(0))
        .unwrap_or_default()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value])?;
    Ok(())
}

pub fn _unused_json() -> Value { json!(null) }
