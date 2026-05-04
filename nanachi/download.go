package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"go.mau.fi/whatsmeow/proto/waE2E"
)

// downloadMedia handles a DownloadMedia command end-to-end:
//  1. Checks the in-memory proto cache (or rehydrates from raw_json).
//  2. Computes the target path from sha256 + mimetype.
//  3. If the file already exists (dedup), emits success without
//     re-fetching.
//  4. Otherwise calls whatsmeow.Download, atomic-renames into place,
//     and emits success.
func downloadMedia(mgr *Manager, accountID, messageID string, rawJSON *string) error {
	mgr.mu.Lock()
	client := mgr.clients[accountID]
	mgr.mu.Unlock()
	if client == nil {
		return errors.New("account not connected")
	}

	msg, err := loadMessageProto(accountID, messageID, rawJSON)
	if err != nil {
		return err
	}

	dl, kind, mimetype := downloadable(msg)
	if dl == nil {
		return fmt.Errorf("no downloadable payload for %s", kind)
	}

	root, err := mediaCacheDir()
	if err != nil {
		return fmt.Errorf("media cache dir: %w", err)
	}

	sha256hex := protoSha256(dl)
	if sha256hex == "" {
		// Fallback: hash the message_id itself; not ideal (no dedup) but
		// keeps the file path deterministic.
		h := sha256.Sum256([]byte(accountID + "|" + messageID))
		sha256hex = hex.EncodeToString(h[:])
	}
	target := targetPath(root, sha256hex, extensionFor(mimetype))

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

	if err := writeAtomic(target, data); err != nil {
		return err
	}

	emitMediaDownloadProgress(accountID, messageID, int64(len(data)), int64(len(data)))
	emitMediaDownloaded(accountID, messageID, target, sha256hex, mimetype)
	return nil
}

// loadMessageProto fetches the proto from cache, then falls back to
// rehydrating it from the persisted raw_json the Rust side passes back
// with the command. Required for any chat row that predates this
// process — without it, reopening the app would lose access to all
// previous-day media.
func loadMessageProto(accountID, messageID string, rawJSON *string) (*waE2E.Message, error) {
	if cached, ok := downloadCache.Load(downloadKey(accountID, messageID)); ok {
		m, ok := cached.(*waE2E.Message)
		if !ok || m == nil {
			return nil, errors.New("download cache corrupted")
		}
		return m, nil
	}

	if rawJSON != nil && *rawJSON != "" {
		m, err := unmarshalProto(*rawJSON)
		if err != nil {
			return nil, fmt.Errorf("rehydrate proto from raw_json: %w", err)
		}
		// Repopulate cache so subsequent retries on the same message
		// hit the hot path.
		downloadCache.Store(downloadKey(accountID, messageID), m)
		return m, nil
	}

	return nil, fmt.Errorf("message %s not in download cache and no raw_json provided", messageID)
}

// protoSha256 reads the cleartext FileSHA256 off whichever submessage
// type carries it. Used for both cache pathing and post-download
// verification.
func protoSha256(dl any) string {
	switch x := dl.(type) {
	case *waE2E.ImageMessage:
		return hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.VideoMessage:
		return hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.AudioMessage:
		return hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.StickerMessage:
		return hex.EncodeToString(x.GetFileSHA256())
	case *waE2E.DocumentMessage:
		return hex.EncodeToString(x.GetFileSHA256())
	}
	return ""
}

// writeAtomic writes `data` to `target` via tmp + rename so a crash
// mid-write never leaves a half-baked file at `target` for a future
// dedup hit to use.
func writeAtomic(target string, data []byte) error {
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
	return nil
}
