package main

import (
	"encoding/json"
	"sync"
)

// IpcMessage espelha o formato {id, type, payload} usado pelo lado Rust.
// Comandos chegam como type+payload; eventos saem da mesma forma.
type IpcMessage struct {
	ID      string          `json:"id"`
	Type    string          `json:"type"`
	Payload json.RawMessage `json:"payload,omitempty"`
}

// Comandos do Rust → Go.
type StartAccountPayload struct {
	AccountID string `json:"account_id"`
}

type StopAccountPayload struct {
	AccountID string `json:"account_id"`
}

type LogoutPayload struct {
	AccountID string `json:"account_id"`
}

type SendMessagePayload struct {
	AccountID string `json:"account_id"`
	To        string `json:"to"`
	Content   string `json:"content"`
}

// MarkReadPayload mirrors `IpcCommand::MarkRead`. `SenderJID` matches
// whatsmeow's MarkRead semantics: required for group chats (the
// participant), redundant-but-fine for DMs where it should equal the
// chat JID.
type MarkReadPayload struct {
	AccountID  string   `json:"account_id"`
	ChatJID    string   `json:"chat_jid"`
	SenderJID  string   `json:"sender_jid"`
	MessageIDs []string `json:"message_ids"`
}

// SendMediaPayload mirrors `IpcCommand::SendMedia` from the Rust side.
// `Kind` is one of: image, video, audio, voice, sticker, document.
type SendMediaPayload struct {
	AccountID string  `json:"account_id"`
	To        string  `json:"to"`
	Kind      string  `json:"kind"`
	Path      string  `json:"path"`
	Caption   *string `json:"caption,omitempty"`
	Mimetype  *string `json:"mimetype,omitempty"`
	Filename  *string `json:"filename,omitempty"`
}

// Eventos Go → Rust.
type ContactData struct {
	JID          string  `json:"jid"`
	LID          *string `json:"lid,omitempty"`
	PhoneNumber  *string `json:"phone_number,omitempty"`
	Name         *string `json:"name,omitempty"`
	Notify       *string `json:"notify,omitempty"`
	VerifiedName *string `json:"verified_name,omitempty"`
	ImgURL       *string `json:"img_url,omitempty"`
	Status       *string `json:"status,omitempty"`
}

type ParticipantData struct {
	ID          string  `json:"id"`
	Admin       *string `json:"admin,omitempty"`
	PhoneNumber *string `json:"phone_number,omitempty"`
}

type GroupData struct {
	JID          string            `json:"jid"`
	Subject      *string           `json:"subject,omitempty"`
	Owner        *string           `json:"owner,omitempty"`
	Description  *string           `json:"description,omitempty"`
	Participants []ParticipantData `json:"participants"`
}

type MessageData struct {
	MessageID   string  `json:"message_id"`
	ChatJID     string  `json:"chat_jid"`
	SenderJID   string  `json:"sender_jid"`
	Content     *string `json:"content,omitempty"`
	MessageType string  `json:"message_type"`
	Timestamp   int64   `json:"timestamp"`
	IsFromMe    bool    `json:"is_from_me"`
	RawJSON     *string `json:"raw_json,omitempty"`
	// Inline preview bytes (JPEG for image/video/document, PNG for
	// sticker). `[]byte` round-trips through JSON as base64. Stored in
	// the DB as a BLOB and rendered by the UI as a placeholder before
	// the full media is downloaded.
	Thumbnail []byte `json:"thumbnail,omitempty"`
	// Metadados de mídia (apenas para image/audio/video/sticker/document).
	// Preenchidos pelo extrator a partir do proto sem baixar o arquivo;
	// o download em si é uma operação separada (DownloadMedia).
	MediaMimetype     *string `json:"media_mimetype,omitempty"`
	MediaFilename     *string `json:"media_filename,omitempty"`
	MediaDurationSecs *int64  `json:"media_duration_secs,omitempty"`
	MediaWidth        *int64  `json:"media_width,omitempty"`
	MediaHeight       *int64  `json:"media_height,omitempty"`
	MediaSizeBytes    *int64  `json:"media_size_bytes,omitempty"`
	// SHA256 hex (64 chars) do conteúdo decodificado, vindo do proto
	// (FileSHA256). Permite que múltiplas mensagens reaproveitem o mesmo
	// download em cache.
	MediaSHA256 *string `json:"media_sha256,omitempty"`
	// Reply / quoted-message — extracted from
	// `proto.contextInfo.quotedMessage`. The Rust side stores these
	// alongside the message so the chat bubble can render the
	// dissent-style quote header without a separate lookup.
	QuotedMessageID *string `json:"quoted_message_id,omitempty"`
	QuotedSenderID  *string `json:"quoted_sender_id,omitempty"`
	QuotedPreview   *string `json:"quoted_preview,omitempty"`
	// Mentions — JIDs called out by `@<digits>` in the message text,
	// from `proto.contextInfo.mentionedJID`.
	MentionedJIDs []string `json:"mentioned_jids,omitempty"`
}

// stdoutMu protege stdout de escritas concorrentes (cada goroutine de
// evento emite linhas independentemente).
var stdoutMu sync.Mutex
