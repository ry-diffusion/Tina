package main

import (
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
