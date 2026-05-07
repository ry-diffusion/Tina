package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	waLog "go.mau.fi/whatsmeow/util/log"
)

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

func emitConnected(accountID string, phone, jid, pushName *string) {
	emit("Connected", map[string]any{
		"account_id":   accountID,
		"phone_number": phone,
		"jid":          jid,
		"push_name":    pushName,
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

// emitHistorySyncProgress repassa o `progress` (0..100) que o whatsmeow
// já calcula em cada chunk de HistorySync. `sync_type` carrega o nome
// do enum (INITIAL_BOOTSTRAP, RECENT, FULL, …) pra a UI poder distinguir
// "trazendo histórico antigo" vs. "pegando o último mês".
// `messagesCount` é o acumulado de mensagens sincronizadas até agora —
// exibido na tela de Syncing durante reconects e startups.
func emitHistorySyncProgress(accountID, syncType string, progress uint32, messagesCount int) {
	emit("HistorySyncProgress", map[string]any{
		"account_id":     accountID,
		"sync_type":      syncType,
		"progress":       progress,
		"messages_count": messagesCount,
	})
}

// chatPinItem mirrors `tina_core::ChatPinItem` — emitted in batches
// from `onHistorySync` so the Rust side can flip `chats.pinned` to
// match the WhatsApp app-state.
type chatPinItem struct {
	ChatJID string `json:"chat_jid"`
	Pinned  bool   `json:"pinned"`
}

func emitChatsPinUpdate(accountID string, items []chatPinItem) {
	if len(items) == 0 {
		return
	}
	emit("ChatsPinUpdate", map[string]any{
		"account_id": accountID,
		"items":      items,
	})
}

// chatReadHintItem carries a per-chat `last_read_ts` watermark
// derived from the WhatsApp HistorySync `Conversation.UnreadCount`.
// The Rust side bumps `chats.last_read_ts` so the auto-derived
// unread badge matches what your phone shows.
type chatReadHintItem struct {
	ChatJID    string `json:"chat_jid"`
	LastReadTs int64  `json:"last_read_ts"`
}

func emitChatsReadHint(accountID string, items []chatReadHintItem) {
	if len(items) == 0 {
		return
	}
	emit("ChatsReadHint", map[string]any{
		"account_id": accountID,
		"items":      items,
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

// emitNotice surfaces a non-fatal, user-visible warning — silently
// degraded operations like a missing ffmpeg still completed, but the
// UI should let the user know which corner was cut.
func emitNotice(accountID *string, message string) {
	emit("Notice", map[string]any{
		"account_id": accountID,
		"message":    message,
	})
}

func emitReceiptUpdate(accountID string, messageIDs []string, status string) {
	emit("ReceiptUpdate", map[string]any{
		"account_id":  accountID,
		"message_ids": messageIDs,
		"status":      status,
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

func emitAvatarUpdated(accountID, jid, path string) {
	emit("AvatarUpdated", map[string]any{
		"account_id": accountID,
		"jid":        jid,
		"path":       path,
	})
}

func emitAvatarFailed(accountID, jid, err string) {
	emit("AvatarFailed", map[string]any{
		"account_id": accountID,
		"jid":        jid,
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

// stderrLogger implements waLog.Logger writing to stderr, keeping
// whatsmeow's internal log lines off the IPC stdout channel.
type stderrLogger struct {
	mod string
	min int
}

var levelToInt = map[string]int{
	"":      -1,
	"DEBUG": 0,
	"INFO":  1,
	"WARN":  2,
	"ERROR": 3,
}

func newStderrLogger(module, minLevel string) waLog.Logger {
	return &stderrLogger{mod: module, min: levelToInt[strings.ToUpper(minLevel)]}
}

func (s *stderrLogger) outputf(level, msg string, args ...interface{}) {
	if levelToInt[level] < s.min {
		return
	}
	fmt.Fprintf(os.Stderr, "%s [%s %s] %s\n",
		time.Now().Format("15:04:05.000"), s.mod, level, fmt.Sprintf(msg, args...))
}

func (s *stderrLogger) Errorf(msg string, args ...interface{}) { s.outputf("ERROR", msg, args...) }
func (s *stderrLogger) Warnf(msg string, args ...interface{})  { s.outputf("WARN", msg, args...) }
func (s *stderrLogger) Infof(msg string, args ...interface{})  { s.outputf("INFO", msg, args...) }
func (s *stderrLogger) Debugf(msg string, args ...interface{}) { s.outputf("DEBUG", msg, args...) }
func (s *stderrLogger) Sub(sub string) waLog.Logger {
	return &stderrLogger{mod: fmt.Sprintf("%s/%s", s.mod, sub), min: s.min}
}
