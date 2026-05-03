package main

import (
	"encoding/hex"

	"go.mau.fi/whatsmeow/proto/waE2E"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"
)

// stripForDownload returns a minimal `*waE2E.Message` carrying the
// fields needed for download (URL/DirectPath, MediaKey, FileSha256,
// FileEncSha256, FileLength, Mimetype) plus the JpegThumbnail bytes
// for visual types — those drive the in-chat preview while the user
// hasn't tapped Download yet, so they earn their bytes. We still drop:
//   - Caption/text, mentions, contextInfo, quoted messages
//   - Sibling fields like Conversation/ExtendedTextMessage
//   - The thumbnail on AudioMessage (whatsmeow doesn't expose one)
//
// Returns nil for messages with no downloadable payload — callers must
// skip persistence in that case.
func stripForDownload(m *waE2E.Message) *waE2E.Message {
	if m == nil {
		return nil
	}
	var out waE2E.Message
	switch {
	case m.ImageMessage != nil:
		x := m.ImageMessage
		out.ImageMessage = &waE2E.ImageMessage{
			URL:           x.URL,
			DirectPath:    x.DirectPath,
			MediaKey:      x.MediaKey,
			FileEncSHA256: x.FileEncSHA256,
			FileSHA256:    x.FileSHA256,
			FileLength:    x.FileLength,
			Mimetype:      x.Mimetype,
			Height:        x.Height,
			Width:         x.Width,
		}
	case m.VideoMessage != nil:
		x := m.VideoMessage
		out.VideoMessage = &waE2E.VideoMessage{
			URL:           x.URL,
			DirectPath:    x.DirectPath,
			MediaKey:      x.MediaKey,
			FileEncSHA256: x.FileEncSHA256,
			FileSHA256:    x.FileSHA256,
			FileLength:    x.FileLength,
			Mimetype:      x.Mimetype,
			Height:        x.Height,
			Width:         x.Width,
			Seconds:       x.Seconds,
		}
	case m.AudioMessage != nil:
		x := m.AudioMessage
		out.AudioMessage = &waE2E.AudioMessage{
			URL:           x.URL,
			DirectPath:    x.DirectPath,
			MediaKey:      x.MediaKey,
			FileEncSHA256: x.FileEncSHA256,
			FileSHA256:    x.FileSHA256,
			FileLength:    x.FileLength,
			Mimetype:      x.Mimetype,
			Seconds:       x.Seconds,
		}
	case m.StickerMessage != nil:
		x := m.StickerMessage
		out.StickerMessage = &waE2E.StickerMessage{
			URL:           x.URL,
			DirectPath:    x.DirectPath,
			MediaKey:      x.MediaKey,
			FileEncSHA256: x.FileEncSHA256,
			FileSHA256:    x.FileSHA256,
			FileLength:    x.FileLength,
			Mimetype:      x.Mimetype,
			Height:        x.Height,
			Width:         x.Width,
		}
	case m.DocumentMessage != nil:
		x := m.DocumentMessage
		out.DocumentMessage = &waE2E.DocumentMessage{
			URL:           x.URL,
			DirectPath:    x.DirectPath,
			MediaKey:      x.MediaKey,
			FileEncSHA256: x.FileEncSHA256,
			FileSHA256:    x.FileSHA256,
			FileLength:    x.FileLength,
			Mimetype:      x.Mimetype,
			FileName:      x.FileName,
		}
	default:
		return nil
	}
	return &out
}

// extractThumbnail picks the inline preview bytes off whichever
// submessage carries them. Audio has none. Returned bytes get stored
// in `messages.media_thumbnail` (BLOB) and rendered by the UI as a
// placeholder before the full media is downloaded.
func extractThumbnail(m *waE2E.Message) []byte {
	if m == nil {
		return nil
	}
	switch {
	case m.ImageMessage != nil:
		return m.ImageMessage.GetJPEGThumbnail()
	case m.VideoMessage != nil:
		return m.VideoMessage.GetJPEGThumbnail()
	case m.StickerMessage != nil:
		return m.StickerMessage.GetPngThumbnail()
	case m.DocumentMessage != nil:
		return m.DocumentMessage.GetJPEGThumbnail()
	}
	return nil
}

// marshalProto turns the download-only subset of a `*waE2E.Message`
// (see `stripForDownload`) into a JSON string for persistence in the
// `messages.raw_json` column. We round-trip through `unmarshalProto`
// when DownloadMedia hits a row whose proto isn't in the in-memory
// cache. `protojson` keeps field names stable across proto upgrades.
func marshalProto(m *waE2E.Message) (string, bool) {
	stripped := stripForDownload(m)
	if stripped == nil {
		return "", false
	}
	b, err := protojson.MarshalOptions{
		UseProtoNames:   true,
		EmitUnpopulated: false,
	}.Marshal(stripped)
	if err != nil {
		return "", false
	}
	return string(b), true
}

// unmarshalProto rebuilds a *waE2E.Message from the JSON we cached at
// receive time. Used on the cold path when DownloadMedia hits a row
// that wasn't seen during the current process lifetime.
func unmarshalProto(s string) (*waE2E.Message, error) {
	var m waE2E.Message
	opts := protojson.UnmarshalOptions{DiscardUnknown: true}
	if err := opts.Unmarshal([]byte(s), &m); err != nil {
		return nil, err
	}
	return &m, nil
}

var _ proto.Message = (*waE2E.Message)(nil)

// extractContent inspeciona um *waE2E.Message e retorna (texto, tipo).
// Para tipos não-texto, devolve um placeholder estilo "[Image]" para
// preservar a UX da implementação anterior em TS.
func extractContent(m *waE2E.Message) (string, string) {
	if m == nil {
		return "", "unknown"
	}
	switch {
	case m.Conversation != nil && *m.Conversation != "":
		return *m.Conversation, "text"
	case m.ExtendedTextMessage != nil:
		if t := m.ExtendedTextMessage.GetText(); t != "" {
			return t, "text"
		}
		return "", "text"
	case m.ImageMessage != nil:
		if cap := m.ImageMessage.GetCaption(); cap != "" {
			return cap, "image"
		}
		return "[Image]", "image"
	case m.VideoMessage != nil:
		if cap := m.VideoMessage.GetCaption(); cap != "" {
			return cap, "video"
		}
		return "[Video]", "video"
	case m.AudioMessage != nil:
		return "[Audio]", "audio"
	case m.DocumentMessage != nil:
		if cap := m.DocumentMessage.GetCaption(); cap != "" {
			return cap, "document"
		}
		return "[Document]", "document"
	case m.StickerMessage != nil:
		return "[Sticker]", "sticker"
	case m.ContactMessage != nil:
		return "[Contact]", "contact"
	case m.LocationMessage != nil:
		return "[Location]", "location"
	case m.LiveLocationMessage != nil:
		return "[Live Location]", "location"
	case m.ReactionMessage != nil:
		return m.ReactionMessage.GetText(), "reaction"
	case m.PollCreationMessage != nil:
		return m.PollCreationMessage.GetName(), "poll"
	}
	return "", "unknown"
}

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
// Não baixa nada — só lê o que o whatsmeow já entregou no proto. O download
// real (com decryption) é orquestrado pelo Rust via DownloadMedia.
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

// applyMedia copia os campos de MediaInfo (se presente) para o MessageData
// alvo. Campo a campo pra preservar nil em ausência.
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
