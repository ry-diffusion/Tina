package main

import (
	"mime"
	"os"
	"path/filepath"
	"strings"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
)

// mediaCacheDir devolve `~/.local/share/tina/media/`. Casa com a raiz
// que `directories::ProjectDirs::from("com.br","zesmoi","tina")` resolve
// no Linux (qualifier/organization são ignorados — só o nome é usado),
// mantendo todos os artefatos do app sob o mesmo diretório do `tina.db`.
func mediaCacheDir() (string, error) {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, "tina", "media"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "tina", "media"), nil
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
