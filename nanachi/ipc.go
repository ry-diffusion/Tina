package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
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
}

// Writer protege stdout de escritas concorrentes (cada goroutine de evento
// emite linhas independentemente).
var stdoutMu sync.Mutex

func emit(eventType string, payload any) {
	body, err := json.Marshal(payload)
	if err != nil {
		// payload inválido — converte para Error para não quebrar o canal.
		body, _ = json.Marshal(map[string]any{
			"account_id": nil,
			"error":      fmt.Sprintf("failed to marshal %s payload: %v", eventType, err),
		})
		eventType = "Error"
	}

	msg := IpcMessage{
		ID:      newID(),
		Type:    eventType,
		Payload: body,
	}
	line, err := json.Marshal(msg)
	if err != nil {
		return
	}

	// Um único Write evita 2 syscalls por evento — durante history sync
	// isso é dezenas de milhares de chamadas a menos.
	line = append(line, '\n')
	stdoutMu.Lock()
	defer stdoutMu.Unlock()
	os.Stdout.Write(line)
}

func emitReady(accountID string) {
	emit("Ready", map[string]string{"account_id": accountID})
}

func emitQR(accountID, qr string) {
	emit("QrCode", map[string]string{"account_id": accountID, "qr": qr})
}

func emitPairingCode(accountID, code string) {
	emit("PairingCode", map[string]string{"account_id": accountID, "code": code})
}

func emitConnected(accountID string, phone, jid *string) {
	emit("Connected", map[string]any{
		"account_id":   accountID,
		"phone_number": phone,
		"jid":          jid,
	})
}

func emitDisconnected(accountID, reason string) {
	emit("Disconnected", map[string]string{"account_id": accountID, "reason": reason})
}

func emitLoggedOut(accountID string) {
	emit("LoggedOut", map[string]string{"account_id": accountID})
}

func emitContacts(accountID string, contacts []ContactData) {
	if len(contacts) == 0 {
		return
	}
	emit("ContactsUpsert", map[string]any{
		"account_id": accountID,
		"contacts":   contacts,
	})
}

func emitGroups(accountID string, groups []GroupData) {
	if len(groups) == 0 {
		return
	}
	emit("GroupsUpsert", map[string]any{
		"account_id": accountID,
		"groups":     groups,
	})
}

func emitMessages(accountID string, messages []MessageData) {
	if len(messages) == 0 {
		return
	}
	emit("MessagesUpsert", map[string]any{
		"account_id": accountID,
		"messages":   messages,
	})
}

func emitHistorySyncComplete(accountID string, count int) {
	emit("HistorySyncComplete", map[string]any{
		"account_id":     accountID,
		"messages_count": count,
	})
}

func emitReconcileProgress(accountID, stage string, current, total int, indeterminate bool) {
	emit("ReconcileProgress", map[string]any{
		"account_id":    accountID,
		"stage":         stage,
		"current":       current,
		"total":         total,
		"indeterminate": indeterminate,
	})
}

func emitError(accountID *string, err string) {
	emit("Error", map[string]any{
		"account_id": accountID,
		"error":      err,
	})
}

func emitMediaDownloadProgress(accountID, messageID string, current, total int64) {
	emit("MediaDownloadProgress", map[string]any{
		"account_id": accountID,
		"message_id": messageID,
		"current":    current,
		"total":      total,
	})
}

func emitMediaDownloaded(accountID, messageID, path, sha256hex, mimetype string) {
	emit("MediaDownloaded", map[string]any{
		"account_id": accountID,
		"message_id": messageID,
		"path":       path,
		"sha256":     strPtr(sha256hex),
		"mimetype":   strPtr(mimetype),
	})
}

func emitMediaDownloadFailed(accountID, messageID, err string) {
	emit("MediaDownloadFailed", map[string]any{
		"account_id": accountID,
		"message_id": messageID,
		"error":      err,
	})
}

func emitCommandResult(commandID string, success bool, data any, errStr *string) {
	emit("CommandResult", map[string]any{
		"command_id": commandID,
		"success":    success,
		"data":       data,
		"error":      errStr,
	})
}

func newID() string {
	var b [12]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

func strPtr(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}
