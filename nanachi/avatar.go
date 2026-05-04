package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/types"
)

// avatarCacheDir mirrors mediaCacheDir() but for profile pictures.
// `~/.local/share/tina/avatars/<sha[:2]>/<sha>.<ext>`.
func avatarCacheDir() (string, error) {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, "tina", "avatars"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "tina", "avatars"), nil
}

// fetchAvatar handles a FetchAvatar IPC command end-to-end:
//
//  1. Resolves the JID and asks whatsmeow for the latest profile picture
//     metadata (URL, ID).
//  2. Downloads the bytes via plain HTTP (the WhatsApp CDN URLs are
//     publicly fetchable, no auth headers needed).
//  3. Hashes the bytes; the cached file lives at <sha>.jpg. If the
//     target already exists, we short-circuit.
//  4. Atomic-rename into place and emit AvatarUpdated with the local path.
//
// Failure modes (no profile picture, network error, parse error) all
// surface through emitAvatarFailed so the UI can stop spinning.
func fetchAvatar(mgr *Manager, accountID, jidStr string) error {
	mgr.mu.Lock()
	client := mgr.clients[accountID]
	mgr.mu.Unlock()
	if client == nil {
		return errors.New("account not connected")
	}

	jid, err := types.ParseJID(jidStr)
	if err != nil {
		return fmt.Errorf("parse jid: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pic, err := client.wa.GetProfilePictureInfo(ctx, jid, &whatsmeow.GetProfilePictureParams{
		// Default (preview) is enough for chat-list / headerbar display
		// at 30–48 px. Set IsCommunity false; we want the actual user.
	})
	if err != nil {
		return fmt.Errorf("get profile picture: %w", err)
	}
	if pic == nil || pic.URL == "" {
		return errors.New("no profile picture")
	}

	root, err := avatarCacheDir()
	if err != nil {
		return fmt.Errorf("avatar cache dir: %w", err)
	}

	// Download the bytes.
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, pic.URL, nil)
	if err != nil {
		return fmt.Errorf("new request: %w", err)
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("http get: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("http %d", resp.StatusCode)
	}
	bytes, err := io.ReadAll(io.LimitReader(resp.Body, 5*1024*1024))
	if err != nil {
		return fmt.Errorf("read body: %w", err)
	}
	if len(bytes) == 0 {
		return errors.New("empty avatar")
	}

	hash := sha256.Sum256(bytes)
	shaHex := hex.EncodeToString(hash[:])
	target := filepath.Join(root, shaHex[:2], shaHex+".jpg")

	if st, err := os.Stat(target); err == nil && st.Size() > 0 {
		// Dedup: same image hash already on disk for some other JID.
		emitAvatarUpdated(accountID, jidStr, target)
		return nil
	}

	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return fmt.Errorf("mkdir: %w", err)
	}
	tmp, err := os.CreateTemp(filepath.Dir(target), ".avatar-*")
	if err != nil {
		return fmt.Errorf("tempfile: %w", err)
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)
	if _, err := tmp.Write(bytes); err != nil {
		tmp.Close()
		return fmt.Errorf("write: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close tmp: %w", err)
	}
	if err := os.Rename(tmpPath, target); err != nil {
		return fmt.Errorf("rename: %w", err)
	}

	emitAvatarUpdated(accountID, jidStr, target)
	return nil
}
