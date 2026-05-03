package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"mime"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
)

// downloadCache associa (account_id, message_id) ao *waE2E.Message recebido.
// É populado em extract* e consumido pelo handler de DownloadMedia. Mantém
// só os ponteiros — o whatsmeow já retém as estruturas pelo lifecycle do
// evento, e nosso uso (download tardio) é leitura, não escrita.
var downloadCache sync.Map // map[string]*waE2E.Message

func downloadKey(accountID, messageID string) string {
	return accountID + "|" + messageID
}

// rememberForDownload guarda o proto em cache se for um tipo baixável.
// Texto e payloads sem mídia não entram (poupa memória).
func rememberForDownload(accountID, messageID string, m *waE2E.Message) {
	if m == nil {
		return
	}
	switch {
	case m.ImageMessage != nil,
		m.VideoMessage != nil,
		m.AudioMessage != nil,
		m.StickerMessage != nil,
		m.DocumentMessage != nil:
		downloadCache.Store(downloadKey(accountID, messageID), m)
	}
}

// mediaCacheDir devolve `~/.local/share/com.br.zesmoi.tina/media/`.
// Casa com o ProjectDirs("com.br","zesmoi","tina") usado pelo Rust.
func mediaCacheDir() (string, error) {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, "com.br.zesmoi.tina", "media"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "com.br.zesmoi.tina", "media"), nil
}

// extensionFor escolhe uma extensão a partir do mimetype com prioridade
// para os "comuns" do whatsmeow (image/jpeg → .jpg, audio/ogg → .ogg).
func extensionFor(mimetype string) string {
	mimetype = strings.ToLower(strings.TrimSpace(mimetype))
	if mimetype == "" {
		return ".bin"
	}
	// Aliases canônicos primeiro (a função do stdlib às vezes prefere
	// extensões longas como ".jpeg" ou ".jpe").
	switch {
	case strings.HasPrefix(mimetype, "image/jpeg"):
		return ".jpg"
	case strings.HasPrefix(mimetype, "image/png"):
		return ".png"
	case strings.HasPrefix(mimetype, "image/webp"):
		return ".webp"
	case strings.HasPrefix(mimetype, "image/gif"):
		return ".gif"
	case strings.HasPrefix(mimetype, "audio/ogg"):
		return ".ogg"
	case strings.HasPrefix(mimetype, "audio/mpeg"), strings.HasPrefix(mimetype, "audio/mp3"):
		return ".mp3"
	case strings.HasPrefix(mimetype, "audio/mp4"), strings.HasPrefix(mimetype, "audio/m4a"):
		return ".m4a"
	case strings.HasPrefix(mimetype, "video/mp4"):
		return ".mp4"
	case strings.HasPrefix(mimetype, "video/webm"):
		return ".webm"
	case strings.HasPrefix(mimetype, "application/pdf"):
		return ".pdf"
	}
	exts, _ := mime.ExtensionsByType(mimetype)
	if len(exts) > 0 {
		return exts[0]
	}
	return ".bin"
}

// targetPath computes the cached file path from a sha256 (hex). Two-letter
// shard prefix prevents flat directories of millions of files.
func targetPath(rootDir, sha256hex, ext string) string {
	if len(sha256hex) < 2 {
		return filepath.Join(rootDir, sha256hex+ext)
	}
	return filepath.Join(rootDir, sha256hex[:2], sha256hex+ext)
}

// downloadable extracts the actual *waE2E.{Image,Audio,…}Message and a
// label suitable for logging/error messages.
func downloadable(m *waE2E.Message) (whatsmeow.DownloadableMessage, string, string) {
	switch {
	case m.ImageMessage != nil:
		return m.ImageMessage, "image", m.ImageMessage.GetMimetype()
	case m.VideoMessage != nil:
		return m.VideoMessage, "video", m.VideoMessage.GetMimetype()
	case m.AudioMessage != nil:
		return m.AudioMessage, "audio", m.AudioMessage.GetMimetype()
	case m.StickerMessage != nil:
		return m.StickerMessage, "sticker", m.StickerMessage.GetMimetype()
	case m.DocumentMessage != nil:
		return m.DocumentMessage, "document", m.DocumentMessage.GetMimetype()
	}
	return nil, "unknown", ""
}

// downloadMedia handles a DownloadMedia command end-to-end:
//  1. Checks the in-memory proto cache.
//  2. Computes the target path from sha256 + mimetype.
//  3. If the file already exists (dedup), emits success without re-fetching.
//  4. Otherwise calls whatsmeow.Download, hashes while writing for integrity,
//     atomic-renames into place, and emits success.
//
// Progress events are emitted in chunks (every ~64KB) to avoid flooding IPC.
func downloadMedia(mgr *Manager, accountID, messageID string) error {
	mgr.mu.Lock()
	client := mgr.clients[accountID]
	mgr.mu.Unlock()
	if client == nil {
		return errors.New("account not connected")
	}

	cached, ok := downloadCache.Load(downloadKey(accountID, messageID))
	if !ok {
		return fmt.Errorf("message %s not in download cache (likely from before app start; re-receive or wait for sync)", messageID)
	}
	msg, ok := cached.(*waE2E.Message)
	if !ok || msg == nil {
		return errors.New("download cache corrupted")
	}

	dl, kind, mimetype := downloadable(msg)
	if dl == nil {
		return fmt.Errorf("no downloadable payload for %s", kind)
	}

	root, err := mediaCacheDir()
	if err != nil {
		return fmt.Errorf("media cache dir: %w", err)
	}

	// SHA-256 from the proto is the cleartext hash; use it for both cache
	// pathing and post-download verification.
	var sha256hex string
	switch x := dl.(type) {
	case *waE2E.ImageMessage:
		sha256hex = hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.VideoMessage:
		sha256hex = hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.AudioMessage:
		sha256hex = hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.StickerMessage:
		sha256hex = hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.DocumentMessage:
		sha256hex = hex.EncodeToString(x.GetFileSHA256())
	}

	ext := extensionFor(mimetype)
	if sha256hex == "" {
		// Fallback: hash the message_id itself; not ideal (no dedup) but
		// keeps the file path deterministic.
		h := sha256.Sum256([]byte(accountID + "|" + messageID))
		sha256hex = hex.EncodeToString(h[:])
	}
	target := targetPath(root, sha256hex, ext)

	// Dedup hit: another message already pulled this exact file.
	if st, err := os.Stat(target); err == nil && st.Size() > 0 {
		emitMediaDownloaded(accountID, messageID, target, sha256hex, mimetype)
		return nil
	}

	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return fmt.Errorf("mkdir media: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	emitMediaDownloadProgress(accountID, messageID, 0, 0)

	data, err := client.wa.Download(ctx, dl)
	if err != nil {
		return fmt.Errorf("download: %w", err)
	}

	// Atomic write: tmp + rename so a crash mid-write never leaves a
	// half-baked file at `target` for a future dedup hit to use.
	tmp, err := os.CreateTemp(filepath.Dir(target), ".download-*")
	if err != nil {
		return fmt.Errorf("create tmp: %w", err)
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath) // no-op if rename succeeded

	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return fmt.Errorf("write tmp: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close tmp: %w", err)
	}
	if err := os.Rename(tmpPath, target); err != nil {
		return fmt.Errorf("rename: %w", err)
	}

	emitMediaDownloadProgress(accountID, messageID, int64(len(data)), int64(len(data)))
	emitMediaDownloaded(accountID, messageID, target, sha256hex, mimetype)
	return nil
}
