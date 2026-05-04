package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
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
