package main

import (
	"encoding/hex"

	"go.mau.fi/whatsmeow/proto/waE2E"
)

// MediaInfo carrega metadados de mídia extraídos do proto (sem download).
// Bate 1-pra-1 com os campos opcionais de MessageData.
type MediaInfo struct {
	Mimetype     *string
	Filename     *string
	DurationSecs *int64
	Width        *int64
	Height       *int64
	SizeBytes    *int64
	SHA256       *string
}

// extractMedia retorna metadados pra mensagens com payload de mídia.
// Não baixa nada — só lê o que o whatsmeow já entregou no proto. O
// download real (com decryption) é orquestrado pelo Rust via
// DownloadMedia.
func extractMedia(m *waE2E.Message) *MediaInfo {
	if m == nil {
		return nil
	}
	switch {
	case m.ImageMessage != nil:
		x := m.ImageMessage
		return &MediaInfo{
			Mimetype:  strPtrOrNil(x.GetMimetype()),
			Width:     i64PtrOrNil(int64(x.GetWidth())),
			Height:    i64PtrOrNil(int64(x.GetHeight())),
			SizeBytes: i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:    hexPtrOrNil(x.GetFileSHA256()),
		}
	case m.VideoMessage != nil:
		x := m.VideoMessage
		return &MediaInfo{
			Mimetype:     strPtrOrNil(x.GetMimetype()),
			Width:        i64PtrOrNil(int64(x.GetWidth())),
			Height:       i64PtrOrNil(int64(x.GetHeight())),
			DurationSecs: i64PtrOrNil(int64(x.GetSeconds())),
			SizeBytes:    i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:       hexPtrOrNil(x.GetFileSHA256()),
		}
	case m.AudioMessage != nil:
		x := m.AudioMessage
		return &MediaInfo{
			Mimetype:     strPtrOrNil(x.GetMimetype()),
			DurationSecs: i64PtrOrNil(int64(x.GetSeconds())),
			SizeBytes:    i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:       hexPtrOrNil(x.GetFileSHA256()),
		}
	case m.StickerMessage != nil:
		x := m.StickerMessage
		return &MediaInfo{
			Mimetype:  strPtrOrNil(x.GetMimetype()),
			Width:     i64PtrOrNil(int64(x.GetWidth())),
			Height:    i64PtrOrNil(int64(x.GetHeight())),
			SizeBytes: i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:    hexPtrOrNil(x.GetFileSHA256()),
		}
	case m.LottieStickerMessage != nil:
		inner := m.LottieStickerMessage.GetMessage()
		if inner == nil || inner.StickerMessage == nil {
			return nil
		}
		x := inner.StickerMessage
		return &MediaInfo{
			Mimetype:  strPtrOrNil(x.GetMimetype()),
			Width:     i64PtrOrNil(int64(x.GetWidth())),
			Height:    i64PtrOrNil(int64(x.GetHeight())),
			SizeBytes: i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:    hexPtrOrNil(x.GetFileSHA256()),
		}
	case m.DocumentMessage != nil:
		x := m.DocumentMessage
		return &MediaInfo{
			Mimetype:  strPtrOrNil(x.GetMimetype()),
			Filename:  strPtrOrNil(x.GetFileName()),
			SizeBytes: i64PtrOrNil(int64(x.GetFileLength())),
			SHA256:    hexPtrOrNil(x.GetFileSHA256()),
		}
	}
	return nil
}

// applyMedia copia os campos de MediaInfo (se presente) para o
// MessageData alvo. Campo a campo pra preservar nil em ausência.
func applyMedia(md *MessageData, mi *MediaInfo) {
	if mi == nil {
		return
	}
	md.MediaMimetype = mi.Mimetype
	md.MediaFilename = mi.Filename
	md.MediaDurationSecs = mi.DurationSecs
	md.MediaWidth = mi.Width
	md.MediaHeight = mi.Height
	md.MediaSizeBytes = mi.SizeBytes
	md.MediaSHA256 = mi.SHA256
}

func strPtrOrNil(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

func i64PtrOrNil(v int64) *int64 {
	if v <= 0 {
		return nil
	}
	return &v
}

func hexPtrOrNil(b []byte) *string {
	if len(b) == 0 {
		return nil
	}
	s := hex.EncodeToString(b)
	return &s
}
