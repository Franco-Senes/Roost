-- M9: Agente de IA, claves cifradas, conversaciones, acciones con aprobación y proyectos 24/7

CREATE TABLE IF NOT EXISTS ai_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('ollama', 'openrouter', 'hackclub')),
    api_key_encrypted TEXT NOT NULL DEFAULT '',
    key_last4 TEXT NOT NULL DEFAULT '',
    base_url TEXT NOT NULL DEFAULT '',
    UNIQUE(user_id, provider)
);

CREATE TABLE IF NOT EXISTS ai_model_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    allowed_role TEXT,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    daily_limit INTEGER NOT NULL DEFAULT 50
);

CREATE TABLE IF NOT EXISTS ai_conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES ai_conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
    content TEXT NOT NULL,
    reasoning TEXT NOT NULL DEFAULT '',
    tool_calls TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    summary TEXT NOT NULL,
    requires_admin INTEGER NOT NULL DEFAULT 1 CHECK (requires_admin IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'executed', 'failed')),
    result TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    runner TEXT NOT NULL CHECK (runner IN ('antigravity', 'claude', 'custom')),
    workdir TEXT NOT NULL,
    command TEXT NOT NULL,
    auto_restart INTEGER NOT NULL DEFAULT 1 CHECK (auto_restart IN (0, 1)),
    created_at INTEGER NOT NULL
);
