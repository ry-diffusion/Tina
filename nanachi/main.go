package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	_ "github.com/mattn/go-sqlite3"
	"go.mau.fi/whatsmeow/store/sqlstore"
	waLog "go.mau.fi/whatsmeow/util/log"
)

func dataDir() (string, error) {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, "tina"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "tina"), nil
}

func main() {
	dir, err := dataDir()
	if err != nil {
		emitError(nil, fmt.Sprintf("failed to resolve data dir: %v", err))
		os.Exit(1)
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		emitError(nil, fmt.Sprintf("failed to create data dir: %v", err))
		os.Exit(1)
	}

	dbPath := filepath.Join(dir, "whatsmeow.db")
	dsn := fmt.Sprintf("file:%s?_foreign_keys=on&_journal_mode=WAL", dbPath)

	logger := waLog.Stdout("whatsmeow", "WARN", false)
	ctx := context.Background()
	container, err := sqlstore.New(ctx, "sqlite3", dsn, logger)
	if err != nil {
		emitError(nil, fmt.Sprintf("failed to open whatsmeow store: %v", err))
		os.Exit(1)
	}

	mgr := newManager(container, logger)

	// Sinais derrubam o processo limpo (whatsmeow chama Disconnect via Stop).
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)
	go func() {
		<-sigCh
		mgr.shutdown()
		os.Exit(0)
	}()

	emitReady("")

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var msg IpcMessage
		if err := json.Unmarshal(line, &msg); err != nil {
			emitError(nil, fmt.Sprintf("failed to decode command: %v", err))
			continue
		}
		handleCommand(mgr, msg)
	}

	mgr.shutdown()
}

func handleCommand(mgr *Manager, msg IpcMessage) {
	switch msg.Type {
	case "StartAccount":
		var p StartAccountPayload
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		if err := mgr.startAccount(p.AccountID); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		emitCommandResult(msg.ID, true, nil, nil)

	case "StopAccount":
		var p StopAccountPayload
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		mgr.stopAccount(p.AccountID, "Stopped by user")
		emitCommandResult(msg.ID, true, nil, nil)

	case "Logout":
		var p LogoutPayload
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		if err := mgr.logoutAccount(p.AccountID); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		emitCommandResult(msg.ID, true, nil, nil)

	case "Reconcile":
		var p struct {
			AccountID string `json:"account_id"`
		}
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		if err := mgr.reconcileAccount(p.AccountID); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		emitCommandResult(msg.ID, true, nil, nil)

	case "SendMessage":
		var p SendMessagePayload
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		ok, err := mgr.sendMessage(p.AccountID, p.To, p.Content)
		var errStr *string
		if err != nil {
			s := err.Error()
			errStr = &s
		}
		emitCommandResult(msg.ID, ok, nil, errStr)

	case "DownloadMedia":
		var p struct {
			AccountID string `json:"account_id"`
			MessageID string `json:"message_id"`
		}
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		// Download é sempre async pra não bloquear o loop de IPC; o
		// CommandResult sai imediatamente como "aceito", e o resultado
		// real chega via MediaDownloaded / MediaDownloadFailed.
		emitCommandResult(msg.ID, true, nil, nil)
		go func() {
			if err := downloadMedia(mgr, p.AccountID, p.MessageID); err != nil {
				emitMediaDownloadFailed(p.AccountID, p.MessageID, err.Error())
			}
		}()

	case "FetchAvatar":
		var p struct {
			AccountID string `json:"account_id"`
			JID       string `json:"jid"`
		}
		if err := json.Unmarshal(msg.Payload, &p); err != nil {
			emitCommandResult(msg.ID, false, nil, strPtr(err.Error()))
			return
		}
		emitCommandResult(msg.ID, true, nil, nil)
		go func() {
			if err := fetchAvatar(mgr, p.AccountID, p.JID); err != nil {
				emitAvatarFailed(p.AccountID, p.JID, err.Error())
			}
		}()

	case "Shutdown":
		emitCommandResult(msg.ID, true, nil, nil)
		mgr.shutdown()
		os.Exit(0)

	default:
		emitCommandResult(msg.ID, false, nil, strPtr(fmt.Sprintf("unknown command: %s", msg.Type)))
	}
}
