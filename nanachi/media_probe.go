package main

// Best-effort host-tool integrations: JPEG thumbnails, audio
// duration, voice-note waveform. Each helper falls back gracefully
// when the matching binary is missing (ffmpeg / gst-discoverer-1.0)
// — emitNotice surfaces the degradation to the UI so the user knows
// the bubble was sent without a preview / waveform / duration.

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"image"
	"image/jpeg"
	"io"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"golang.org/x/image/draw"
)

// noticeDedup tracks one-off "tool missing" warnings per accountID +
// reason so a chat that sends 50 photos doesn't emit 50 toasts.
var (
	noticeDedupMu sync.Mutex
	noticeDedup   = map[string]struct{}{}
)

func noticeOnce(accountID, key, message string) {
	dedupKey := accountID + "|" + key
	noticeDedupMu.Lock()
	if _, seen := noticeDedup[dedupKey]; seen {
		noticeDedupMu.Unlock()
		return
	}
	noticeDedup[dedupKey] = struct{}{}
	noticeDedupMu.Unlock()
	emitNotice(&accountID, message)
}

// jpegThumbnailFromImage downscales `data` (any stdlib-decodable
// image) to at most 200px on the long side and re-encodes as JPEG.
// Returns nil + nil if decoding fails — callers treat that as "no
// thumbnail this time", not as a fatal error.
func jpegThumbnailFromImage(data []byte) ([]byte, error) {
	src, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	return encodeJPEGThumbnail(src), nil
}

func encodeJPEGThumbnail(src image.Image) []byte {
	const max = 200
	b := src.Bounds()
	w, h := b.Dx(), b.Dy()
	scale := 1.0
	if w > h && w > max {
		scale = float64(max) / float64(w)
	} else if h >= w && h > max {
		scale = float64(max) / float64(h)
	}
	tw := int(math.Round(float64(w) * scale))
	th := int(math.Round(float64(h) * scale))
	if tw < 1 {
		tw = 1
	}
	if th < 1 {
		th = 1
	}
	dst := image.NewRGBA(image.Rect(0, 0, tw, th))
	draw.CatmullRom.Scale(dst, dst.Bounds(), src, b, draw.Over, nil)
	var out bytes.Buffer
	if err := jpeg.Encode(&out, dst, &jpeg.Options{Quality: 70}); err != nil {
		return nil
	}
	return out.Bytes()
}

// videoThumbnailJPEG extracts the first frame of `path` via ffmpeg
// and re-encodes it through `encodeJPEGThumbnail` (so it ends up at
// the same ~200px envelope and quality as image thumbnails).
// Returns nil if ffmpeg is missing — caller should `noticeOnce` so
// the user gets a one-time toast.
func videoThumbnailJPEG(ctx context.Context, path string) ([]byte, bool) {
	if _, err := exec.LookPath("ffmpeg"); err != nil {
		return nil, false
	}
	tmp := filepath.Join(os.TempDir(), fmt.Sprintf("tina-thumb-%d.jpg", time.Now().UnixNano()))
	defer os.Remove(tmp)
	cmd := exec.CommandContext(ctx,
		"ffmpeg", "-y",
		"-loglevel", "error",
		"-ss", "0",
		"-i", path,
		"-frames:v", "1",
		"-q:v", "5",
		tmp,
	)
	if err := cmd.Run(); err != nil {
		return nil, true
	}
	raw, err := os.ReadFile(tmp)
	if err != nil {
		return nil, true
	}
	thumb, err := jpegThumbnailFromImage(raw)
	if err != nil {
		return nil, true
	}
	return thumb, true
}

// probeAudioDurationSecs returns the duration of `path` in seconds,
// trying gst-discoverer-1.0 first and falling back to ffprobe.
// Returns (0, false) when neither is available; the caller should
// noticeOnce in that case.
func probeAudioDurationSecs(ctx context.Context, path string) (uint32, bool) {
	if secs, ok := probeViaGstDiscoverer(ctx, path); ok {
		return secs, true
	}
	if secs, ok := probeViaFfprobe(ctx, path); ok {
		return secs, true
	}
	return 0, false
}

var gstDurationRe = regexp.MustCompile(`(?m)^Duration:\s+(\d+):(\d+):(\d+)\.(\d+)`)

func probeViaGstDiscoverer(ctx context.Context, path string) (uint32, bool) {
	if _, err := exec.LookPath("gst-discoverer-1.0"); err != nil {
		return 0, false
	}
	cmd := exec.CommandContext(ctx, "gst-discoverer-1.0", path)
	out, err := cmd.Output()
	if err != nil {
		return 0, false
	}
	m := gstDurationRe.FindStringSubmatch(string(out))
	if m == nil {
		return 0, false
	}
	h, _ := strconv.Atoi(m[1])
	mi, _ := strconv.Atoi(m[2])
	s, _ := strconv.Atoi(m[3])
	total := h*3600 + mi*60 + s
	if total <= 0 {
		// guarantee non-zero so a sub-second clip still shows as 1s
		total = 1
	}
	return uint32(total), true
}

func probeViaFfprobe(ctx context.Context, path string) (uint32, bool) {
	if _, err := exec.LookPath("ffprobe"); err != nil {
		return 0, false
	}
	cmd := exec.CommandContext(ctx,
		"ffprobe",
		"-v", "error",
		"-show_entries", "format=duration",
		"-of", "default=noprint_wrappers=1:nokey=1",
		path,
	)
	out, err := cmd.Output()
	if err != nil {
		return 0, false
	}
	f, err := strconv.ParseFloat(strings.TrimSpace(string(out)), 64)
	if err != nil {
		return 0, false
	}
	if f < 1 {
		return 1, true
	}
	return uint32(f), true
}

// generateWaveform decodes `path` to mono 8 kHz s16le via ffmpeg and
// bins the absolute amplitude into 64 buckets, normalised to 0..100.
// Matches the WhatsApp wire shape (`AudioMessage.Waveform` is a 64-
// byte array). Returns nil if ffmpeg is unavailable; nil result means
// "leave the proto field empty" — peers fall back to a flat bar.
func generateWaveform(ctx context.Context, path string) ([]byte, bool) {
	if _, err := exec.LookPath("ffmpeg"); err != nil {
		return nil, false
	}
	cmd := exec.CommandContext(ctx,
		"ffmpeg",
		"-loglevel", "error",
		"-i", path,
		"-ac", "1",
		"-ar", "8000",
		"-f", "s16le",
		"-",
	)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, true
	}
	if err := cmd.Start(); err != nil {
		return nil, true
	}
	raw, err := io.ReadAll(stdout)
	_ = cmd.Wait()
	if err != nil || len(raw) < 2 {
		return nil, true
	}
	return binWaveform(raw, 64), true
}

func binWaveform(raw []byte, buckets int) []byte {
	if buckets <= 0 {
		return nil
	}
	samples := len(raw) / 2
	if samples == 0 {
		return make([]byte, buckets)
	}
	out := make([]byte, buckets)
	per := samples / buckets
	if per < 1 {
		per = 1
	}
	var peak float64
	bucketPeaks := make([]float64, buckets)
	for b := 0; b < buckets; b++ {
		start := b * per
		end := start + per
		if b == buckets-1 || end > samples {
			end = samples
		}
		var sumSq float64
		var n int
		for i := start; i < end; i++ {
			lo := raw[i*2]
			hi := raw[i*2+1]
			s := int16(binary.LittleEndian.Uint16([]byte{lo, hi}))
			f := float64(s) / 32768.0
			sumSq += f * f
			n++
		}
		if n == 0 {
			continue
		}
		rms := math.Sqrt(sumSq / float64(n))
		bucketPeaks[b] = rms
		if rms > peak {
			peak = rms
		}
	}
	if peak <= 0 {
		return out
	}
	for b := 0; b < buckets; b++ {
		v := bucketPeaks[b] / peak * 100
		if v > 100 {
			v = 100
		}
		out[b] = byte(v)
	}
	return out
}
