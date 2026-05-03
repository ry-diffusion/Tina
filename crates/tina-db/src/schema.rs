/// Versão lógica do schema. Incrementar quando houver mudança incompatível.
///
/// Histórico:
/// - v2: schema inicial com aliases canônicos.
/// - v3: campos de mídia (mimetype, dims, duração, sha256 pra dedup).
/// - v4: avatar_path em chats e contacts (cache local de profile pics).
/// - v5: last_message_type + last_message_duration_secs em chats (preview).
/// - v6: media_thumbnail BLOB em messages (inline preview JPEG/PNG).
pub const SCHEMA_VERSION: i64 = 6;

/// Comandos para *recriar* o schema do zero (não suporta migração in-place
/// — quando `user_version` diverge, dropamos tudo e criamos de novo).
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT,
    phone_number TEXT,
    jid TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Entidades canônicas: chat e contato têm IDs internos e tabelas de aliases
-- que mapeiam toda forma de JID/LID já vista para o ID canônico. Fora do
-- resolver, ninguém faz lookup por JID — UI e worker usam só chat_id /
-- contact_id.

CREATE TABLE IF NOT EXISTS chats (
    account_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    kind TEXT NOT NULL,                              -- 'dm','group','newsletter','broadcast','status','unknown'
    display_name TEXT,                               -- preenchido p/ groups/newsletters; NULL p/ DM (resolvido via JOIN)
    avatar_url TEXT,
    avatar_path TEXT,                                -- caminho local do profile pic em cache
    last_message_id TEXT,
    last_message_preview TEXT,
    last_message_ts INTEGER,
    last_message_from_me INTEGER NOT NULL DEFAULT 0,
    last_sender_contact_id TEXT,
    last_message_type TEXT,                          -- 'text','image','audio','video','sticker','document'
    last_message_duration_secs INTEGER,              -- preenchido só pra audio/video
    unread_count INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    muted_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (account_id, chat_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chats_last_ts ON chats(account_id, last_message_ts);

CREATE TABLE IF NOT EXISTS chat_aliases (
    account_id TEXT NOT NULL,
    alias_jid TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    PRIMARY KEY (account_id, alias_jid),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chat_aliases_chat ON chat_aliases(account_id, chat_id);

CREATE TABLE IF NOT EXISTS contacts (
    account_id TEXT NOT NULL,
    contact_id TEXT NOT NULL,
    pn_jid TEXT,
    lid_jid TEXT,
    phone_number TEXT,
    push_name TEXT,
    contact_name TEXT,
    business_name TEXT,
    verified_name TEXT,
    avatar_url TEXT,
    avatar_path TEXT,
    status TEXT,
    is_local INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (account_id, contact_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_contacts_pn ON contacts(account_id, pn_jid);
CREATE INDEX IF NOT EXISTS idx_contacts_lid ON contacts(account_id, lid_jid);

CREATE TABLE IF NOT EXISTS contact_aliases (
    account_id TEXT NOT NULL,
    alias_jid TEXT NOT NULL,
    contact_id TEXT NOT NULL,
    PRIMARY KEY (account_id, alias_jid),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_contact_aliases_contact ON contact_aliases(account_id, contact_id);

CREATE TABLE IF NOT EXISTS groups (
    account_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    subject TEXT,
    owner_contact_id TEXT,
    description TEXT,
    participants_json TEXT,
    PRIMARY KEY (account_id, chat_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    sender_contact_id TEXT,
    content TEXT,
    message_type TEXT NOT NULL DEFAULT 'text',
    timestamp INTEGER NOT NULL,
    is_from_me INTEGER NOT NULL DEFAULT 0,
    raw_json TEXT,
    -- Mídia. Campos populados quando a mensagem é image/audio/video/sticker/
    -- document. `media_sha256` é hash do conteúdo claro (decodificado);
    -- usado pra dedup (mesmo arquivo enviado em vários chats vira um path
    -- só na cache). `media_path` fica NULL até o download concluir.
    media_mimetype TEXT,
    media_filename TEXT,
    media_duration_secs INTEGER,
    media_width INTEGER,
    media_height INTEGER,
    media_size_bytes INTEGER,
    media_sha256 TEXT,
    media_path TEXT,
    media_status TEXT NOT NULL DEFAULT 'none',  -- 'none'|'pending'|'downloading'|'done'|'failed'
    -- Inline preview JPEG/PNG bytes para image/video/sticker/document.
    -- ~5-50 KB cada; renderizado como placeholder antes do download.
    media_thumbnail BLOB,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    UNIQUE(account_id, message_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_messages_chat ON messages(account_id, chat_id, timestamp);
-- Acelera merge_contacts_tx (UPDATE messages SET sender_contact_id = ? WHERE
-- sender_contact_id = ?). Sem ele, full scan da tabela inteira por merge.
CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(account_id, sender_contact_id);
-- Lookup por hash pra dedup: encontra outras mensagens que apontam pro
-- mesmo arquivo já baixado.
CREATE INDEX IF NOT EXISTS idx_messages_media_sha ON messages(media_sha256)
    WHERE media_sha256 IS NOT NULL;

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT
);
"#;

pub const MIGRATION_V3_TO_V4: &str = r#"
ALTER TABLE chats ADD COLUMN avatar_path TEXT;
ALTER TABLE contacts ADD COLUMN avatar_path TEXT;
"#;

pub const MIGRATION_V4_TO_V5: &str = r#"
ALTER TABLE chats ADD COLUMN last_message_type TEXT;
ALTER TABLE chats ADD COLUMN last_message_duration_secs INTEGER;
"#;

pub const MIGRATION_V5_TO_V6: &str = r#"
ALTER TABLE messages ADD COLUMN media_thumbnail BLOB;
"#;

/// Migrações in-place pra evitar dropar o banco do usuário. Cada função roda
/// dentro de uma transação no caller.
pub const MIGRATION_V2_TO_V3: &str = r#"
ALTER TABLE messages ADD COLUMN media_mimetype TEXT;
ALTER TABLE messages ADD COLUMN media_filename TEXT;
ALTER TABLE messages ADD COLUMN media_duration_secs INTEGER;
ALTER TABLE messages ADD COLUMN media_width INTEGER;
ALTER TABLE messages ADD COLUMN media_height INTEGER;
ALTER TABLE messages ADD COLUMN media_size_bytes INTEGER;
ALTER TABLE messages ADD COLUMN media_sha256 TEXT;
ALTER TABLE messages ADD COLUMN media_path TEXT;
ALTER TABLE messages ADD COLUMN media_status TEXT NOT NULL DEFAULT 'none';
CREATE INDEX IF NOT EXISTS idx_messages_media_sha ON messages(media_sha256)
    WHERE media_sha256 IS NOT NULL;
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT
);
"#;

/// SQL para apagar todas as tabelas (usado quando `user_version` muda).
pub const SCHEMA_DROP: &str = r#"
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS groups;
DROP TABLE IF EXISTS contact_aliases;
DROP TABLE IF EXISTS contacts;
DROP TABLE IF EXISTS chat_aliases;
DROP TABLE IF EXISTS chats;
DROP TABLE IF EXISTS accounts;
"#;
