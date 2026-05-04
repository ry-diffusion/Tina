package main

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/types"
	"google.golang.org/protobuf/proto"
)

// sendMedia is the entry point for the SendMedia IPC command. It reads
// the file at `path` from disk, uploads it via whatsmeow, builds the
// matching *waE2E.Message and dispatches it. On success it emits a
// synthetic MessageData echo (mirroring Client.send) so the chat
// thread paints the bubble immediately, and atomic-renames the
// source file into the same SHA256-keyed cache dir that `download.go`
// uses — letting the bubble find the local copy without an extra
// download round trip.
func (c *Client) sendMedia(p SendMediaPayload) (string, error) {
	if !c.wa.IsConnected() {
		return "", errors.New("client not connected")
	}
	jid, err := types.ParseJID(p.To)
	if err != nil {
		return "", fmt.Errorf("invalid jid: %w", err)
	}

	data, err := os.ReadFile(p.Path)
	if err != nil {
		return "", fmt.Errorf("read media: %w", err)
	}
	if len(data) == 0 {
		return "", errors.New("empty media file")
	}

	mimetype := strings.TrimSpace(derefStr(p.Mimetype))
	if mimetype == "" {
		mimetype = http.DetectContentType(data)
	}
	// Stickers are always image/webp on the wire; force it even if the
	// detected mime says otherwise (whatsmeow rejects mismatched sticker
	// mimetypes silently).
	if p.Kind == "sticker" {
		mimetype = "image/webp"
	}

	mediaType := uploadMediaType(p.Kind)
	if mediaType == "" {
		return "", fmt.Errorf("unsupported media kind: %s", p.Kind)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()

	resp, err := c.wa.Upload(ctx, data, mediaType)
	if err != nil {
		return "", fmt.Errorf("upload: %w", err)
	}

	width, height := imageSize(p.Kind, data)
	caption := derefStr(p.Caption)
	filename := derefStr(p.Filename)
	if filename == "" {
		filename = filepath.Base(p.Path)
	}

	// Best-effort enrichment: thumbnail / duration / waveform.
	// Each helper returns whatever it managed to compute and we
	// degrade gracefully when a host tool is missing — the
	// optimistic-echo path already works without these.
	enrichCtx, enrichCancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer enrichCancel()
	extras := enrichMedia(enrichCtx, c.accountID, p.Kind, p.Path, data)

	msg := buildMediaMessage(
		p.Kind, &resp, mimetype, caption, filename, width, height, extras,
	)
	if msg == nil {
		return "", fmt.Errorf("unsupported media kind: %s", p.Kind)
	}

	// Pre-generate the message ID so we can stash the proto in the
	// download cache under THE SAME ID whatsmeow ends up using on the
	// wire. Without this the cache key from `rememberForDownload`
	// would be a fresh ID that future DownloadMedia clicks never see.
	preMsgID := c.wa.GenerateMessageID()

	sendCtx, sendCancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer sendCancel()
	sendResp, err := c.wa.SendMessage(sendCtx, jid, msg, whatsmeow.SendRequestExtra{ID: preMsgID})
	if err != nil {
		return "", fmt.Errorf("send: %w", err)
	}

	// Stash both the proto (for the in-memory `downloadCache`) and a
	// JSON-serialised copy on the synthetic echo so persistence works
	// across restart.
	rememberForDownload(c.accountID, sendResp.ID, msg)
	rawJSON, _ := marshalProto(msg)

	// Move the source file into the SHA256-keyed cache so the
	// MediaInventory / lightbox find it on first paint without a
	// re-download. Best-effort: a copy failure only costs one
	// future round trip.
	sha256hex := hex.EncodeToString(resp.FileSHA256)
	cachePath := stashLocalCopy(sha256hex, mimetype, data)

	ts := sendResp.Timestamp.Unix()
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	senderJID := jid.String()
	if id := c.wa.Store.ID; id != nil {
		senderJID = id.String()
	}

	contentSummary := mediaSummaryContent(p.Kind, caption)
	sizeBytes := int64(len(data))
	durationSecs := int64(extras.DurationSecs)
	if durationSecs == 0 {
		durationSecs = mediaDurationSecs(msg)
	}
	w64, h64 := int64(width), int64(height)
	mt := mimetype
	fn := filename
	echo := MessageData{
		MessageID:     sendResp.ID,
		ChatJID:       jid.String(),
		SenderJID:     senderJID,
		Content:       &contentSummary,
		MessageType:   echoMessageType(p.Kind),
		Timestamp:     ts,
		IsFromMe:      true,
		RawJSON:       optStr(rawJSON),
		MediaMimetype: &mt,
		MediaFilename: optStr(fn),
		MediaSizeBytes: &sizeBytes,
		MediaSHA256:   &sha256hex,
		Thumbnail:     extras.JPEGThumbnail,
	}
	if w64 > 0 {
		echo.MediaWidth = &w64
	}
	if h64 > 0 {
		echo.MediaHeight = &h64
	}
	if durationSecs > 0 {
		echo.MediaDurationSecs = &durationSecs
	}
	emitMessages(c.accountID, []MessageData{echo})

	// MediaDownloaded carries the on-disk path back to the UI so the
	// bubble flips out of "tap to download" into "play / view" without
	// the user having to wait for a real Download.
	if cachePath != "" {
		emitMediaDownloaded(c.accountID, sendResp.ID, cachePath, sha256hex, mimetype)
	}

	return sendResp.ID, nil
}

// mediaExtras carries the optional enrichment data we shovel onto
// the proto when host tools are available. Each field is set to its
// zero value when the relevant probe failed or didn't apply.
type mediaExtras struct {
	JPEGThumbnail []byte
	DurationSecs  uint32
	Waveform      []byte
}

// enrichMedia runs the cheap-but-optional probes for `kind` and
// returns whatever it managed to compute. Always succeeds — missing
// tools surface as one-shot toast notices via `noticeOnce`.
func enrichMedia(ctx context.Context, accountID, kind, path string, data []byte) mediaExtras {
	var extras mediaExtras
	switch kind {
	case "image":
		if thumb, err := jpegThumbnailFromImage(data); err == nil {
			extras.JPEGThumbnail = thumb
		}
	case "video":
		thumb, attempted := videoThumbnailJPEG(ctx, path)
		if !attempted {
			noticeOnce(accountID, "ffmpeg-thumb",
				"Sent without preview: install ffmpeg to attach video thumbnails.")
		}
		extras.JPEGThumbnail = thumb
	case "audio":
		secs, ok := probeAudioDurationSecs(ctx, path)
		if !ok {
			noticeOnce(accountID, "audio-duration",
				"Sent without duration: install gst-discoverer-1.0 or ffprobe.")
		}
		extras.DurationSecs = secs
	case "voice":
		secs, ok := probeAudioDurationSecs(ctx, path)
		if !ok {
			noticeOnce(accountID, "audio-duration",
				"Sent without duration: install gst-discoverer-1.0 or ffprobe.")
		}
		extras.DurationSecs = secs
		wave, attempted := generateWaveform(ctx, path)
		if !attempted {
			noticeOnce(accountID, "ffmpeg-waveform",
				"Sent without waveform: install ffmpeg to attach amplitude bars.")
		}
		extras.Waveform = wave
	}
	return extras
}

// uploadMediaType maps our wire-level kind string to a whatsmeow
// MediaType. Stickers piggyback on the Image keys (whatsmeow has no
// dedicated sticker MediaType for single-sticker uploads).
func uploadMediaType(kind string) whatsmeow.MediaType {
	switch kind {
	case "image", "sticker":
		return whatsmeow.MediaImage
	case "video":
		return whatsmeow.MediaVideo
	case "audio", "voice":
		return whatsmeow.MediaAudio
	case "document":
		return whatsmeow.MediaDocument
	}
	return ""
}

// echoMessageType is the value we put on the synthetic echo's
// `message_type`. Stickers/voices collapse onto the same buckets the
// rest of the pipeline (download.go, message_bubble) already knows
// how to render — voice notes share the `audio` bucket because the
// only proto-level difference is the PTT bit.
func echoMessageType(kind string) string {
	switch kind {
	case "voice":
		return "audio"
	}
	return kind
}

func buildMediaMessage(
	kind string,
	resp *whatsmeow.UploadResponse,
	mimetype, caption, filename string,
	width, height int,
	extras mediaExtras,
) *waE2E.Message {
	switch kind {
	case "image":
		im := &waE2E.ImageMessage{
			URL:           proto.String(resp.URL),
			DirectPath:    proto.String(resp.DirectPath),
			MediaKey:      resp.MediaKey,
			Mimetype:      proto.String(mimetype),
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    proto.Uint64(resp.FileLength),
		}
		if caption != "" {
			im.Caption = proto.String(caption)
		}
		if width > 0 {
			im.Width = proto.Uint32(uint32(width))
		}
		if height > 0 {
			im.Height = proto.Uint32(uint32(height))
		}
		if len(extras.JPEGThumbnail) > 0 {
			im.JPEGThumbnail = extras.JPEGThumbnail
		}
		return &waE2E.Message{ImageMessage: im}

	case "video":
		vm := &waE2E.VideoMessage{
			URL:           proto.String(resp.URL),
			DirectPath:    proto.String(resp.DirectPath),
			MediaKey:      resp.MediaKey,
			Mimetype:      proto.String(mimetype),
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    proto.Uint64(resp.FileLength),
		}
		if caption != "" {
			vm.Caption = proto.String(caption)
		}
		if len(extras.JPEGThumbnail) > 0 {
			vm.JPEGThumbnail = extras.JPEGThumbnail
		}
		return &waE2E.Message{VideoMessage: vm}

	case "audio", "voice":
		am := &waE2E.AudioMessage{
			URL:           proto.String(resp.URL),
			DirectPath:    proto.String(resp.DirectPath),
			MediaKey:      resp.MediaKey,
			Mimetype:      proto.String(mimetype),
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    proto.Uint64(resp.FileLength),
		}
		if kind == "voice" {
			am.PTT = proto.Bool(true)
		}
		if extras.DurationSecs > 0 {
			am.Seconds = proto.Uint32(extras.DurationSecs)
		}
		if len(extras.Waveform) > 0 {
			am.Waveform = extras.Waveform
		}
		return &waE2E.Message{AudioMessage: am}

	case "sticker":
		sm := &waE2E.StickerMessage{
			URL:           proto.String(resp.URL),
			DirectPath:    proto.String(resp.DirectPath),
			MediaKey:      resp.MediaKey,
			Mimetype:      proto.String(mimetype),
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    proto.Uint64(resp.FileLength),
		}
		if width > 0 {
			sm.Width = proto.Uint32(uint32(width))
		}
		if height > 0 {
			sm.Height = proto.Uint32(uint32(height))
		}
		return &waE2E.Message{StickerMessage: sm}

	case "document":
		dm := &waE2E.DocumentMessage{
			URL:           proto.String(resp.URL),
			DirectPath:    proto.String(resp.DirectPath),
			MediaKey:      resp.MediaKey,
			Mimetype:      proto.String(mimetype),
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    proto.Uint64(resp.FileLength),
		}
		if filename != "" {
			dm.FileName = proto.String(filename)
			dm.Title = proto.String(strings.TrimSuffix(filename, filepath.Ext(filename)))
		}
		if caption != "" {
			dm.Caption = proto.String(caption)
		}
		return &waE2E.Message{DocumentMessage: dm}
	}
	return nil
}

// imageSize cheap-decodes the header of an image/sticker payload to
// fill width/height on the proto. Failures fall back to (0, 0); the
// peer can still render without those hints.
func imageSize(kind string, data []byte) (int, int) {
	if kind != "image" && kind != "sticker" {
		return 0, 0
	}
	cfg, _, err := image.DecodeConfig(strings.NewReader(string(data)))
	if err != nil {
		return 0, 0
	}
	return cfg.Width, cfg.Height
}

// mediaDurationSecs pulls the seconds out of an audio/video proto we
// just built. We don't ffprobe ourselves — for caller-supplied audio
// we'd need a decoder dependency we'd rather avoid; voice notes
// recorded by the UI carry their own duration via the wrapper. Left
// at 0 means "unknown", which the bubble already handles.
func mediaDurationSecs(m *waE2E.Message) int64 {
	if m.AudioMessage != nil {
		return int64(m.AudioMessage.GetSeconds())
	}
	return 0
}

// stashLocalCopy writes the raw plaintext into the on-disk media cache
// keyed by sha256, the same scheme `downloadMedia` uses. Best-effort —
// a failure here only means the UI re-downloads next time the bubble
// scrolls into view. Returns the absolute path on success or "" on
// failure.
func stashLocalCopy(sha256hex, mimetype string, data []byte) string {
	root, err := mediaCacheDir()
	if err != nil {
		return ""
	}
	target := targetPath(root, sha256hex, extensionFor(mimetype))
	if st, err := os.Stat(target); err == nil && st.Size() > 0 {
		return target
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return ""
	}
	if err := writeAtomic(target, data); err != nil {
		return ""
	}
	return target
}

// mediaSummaryContent matches the placeholder text emitted by
// extractContent() on the download path: image/video/document carry
// the caption (if any) as `content`, the rest get a `[Kind]`
// placeholder. Keeps echoed rows visually identical to history-sync
// rows.
func mediaSummaryContent(kind, caption string) string {
	switch kind {
	case "image":
		if caption != "" {
			return caption
		}
		return "[Image]"
	case "video":
		if caption != "" {
			return caption
		}
		return "[Video]"
	case "document":
		if caption != "" {
			return caption
		}
		return "[Document]"
	case "audio":
		return "[Audio]"
	case "voice":
		return "[Audio]"
	case "sticker":
		return "[Sticker]"
	}
	return ""
}

func derefStr(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

func optStr(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}
