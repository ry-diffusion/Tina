package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sync/atomic"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
)

// Client encapsula um *whatsmeow.Client por account_id.
type Client struct {
	mgr       *Manager
	accountID string
	wa        *whatsmeow.Client

	historyCount atomic.Int64
	// historySyncSeen flips to true on the first onHistorySync chunk
	// (regardless of progress). The OfflineSyncCompleted /
	// AppStateSyncComplete fallbacks check this before emitting their
	// own HistorySyncComplete — if a real HistorySync stream is
	// running, those events would prematurely close the syncing scene
	// while the device is still streaming INITIAL_BOOTSTRAP chunks.
	historySyncSeen atomic.Bool
	// fallbackScheduled guards `scheduleFallbackHistoryComplete` so
	// repeated AppStateSyncComplete events don't queue overlapping
	// timers (whatsmeow fires it once per app-state name on every
	// sync — 5+ times during the initial boot).
	fallbackScheduled atomic.Bool
}

// scheduleFallbackHistoryComplete arms a one-shot timer that emits
// HistorySyncComplete after `fallbackHistoryCompleteDelay` if no
// HistorySync chunk has arrived in the meantime. Called from
// OfflineSyncCompleted and AppStateSyncComplete; idempotent across
// multiple calls.
func (c *Client) scheduleFallbackHistoryComplete() {
	if !c.fallbackScheduled.CompareAndSwap(false, true) {
		return
	}
	accountID := c.accountID
	go func() {
		time.Sleep(fallbackHistoryCompleteDelay)
		if c.historySyncSeen.Load() {
			fmt.Fprintf(os.Stderr,
				"[sync] fallback skipped for %s — HistorySync took over\n",
				accountID,
			)
			return
		}
		fmt.Fprintf(os.Stderr,
			"[sync] fallback HistorySyncComplete for %s (no chunks within %s)\n",
			accountID, fallbackHistoryCompleteDelay,
		)
		emitHistorySyncComplete(accountID, int(c.historyCount.Load()))
	}()
}

func newClient(mgr *Manager, accountID string, device *store.Device) *Client {
	wa := whatsmeow.NewClient(device, mgr.logger)
	wa.EnableAutoReconnect = true
	// Por padrão, eventos como Contact/PushName/BusinessName só são
	// emitidos DEPOIS que o app-state full sync termina. Ligando isso
	// eles saem durante o sync, e a UI vai resolvendo nomes em tempo
	// real em vez de receber tudo no fim — melhora muito a experiência
	// do primeiro login.
	wa.EmitAppStateEventsOnFullSync = true
	c := &Client{
		mgr:       mgr,
		accountID: accountID,
		wa:        wa,
	}
	wa.AddEventHandler(c.handleEvent)
	return c
}

// connect inicia a sessão. Para um device novo, abre o canal de QR
// antes de chamar Connect (Connect com store.ID == nil dispara o
// pareamento).
func (c *Client) connect(ctx context.Context) error {
	if c.wa.Store.ID == nil {
		qrChan, err := c.wa.GetQRChannel(ctx)
		if err != nil {
			return fmt.Errorf("get qr channel: %w", err)
		}
		go c.consumeQR(qrChan)
	}
	if err := c.wa.Connect(); err != nil {
		return fmt.Errorf("connect: %w", err)
	}
	return nil
}

func (c *Client) consumeQR(ch <-chan whatsmeow.QRChannelItem) {
	for evt := range ch {
		switch evt.Event {
		case "code":
			emitQR(c.accountID, evt.Code)
		case "success":
			// pareou — Connected será emitido pelo handler de eventos.
			return
		case "timeout", "err-client-outdated", "err-scanned-without-multidevice":
			emitError(&c.accountID, fmt.Sprintf("pairing %s", evt.Event))
			return
		}
	}
}

func (c *Client) disconnect(reason string) {
	c.wa.Disconnect()
	emitDisconnected(c.accountID, reason)
}

func (c *Client) send(to, content string) (bool, error) {
	jid, err := types.ParseJID(to)
	if err != nil {
		return false, fmt.Errorf("invalid jid: %w", err)
	}
	if !c.wa.IsConnected() {
		return false, errors.New("client not connected")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	resp, err := c.wa.SendMessage(ctx, jid, &waE2E.Message{
		Conversation: &content,
	})
	if err != nil {
		return false, err
	}

	// whatsmeow only fires events.Message for incoming traffic; outgoing
	// messages stay invisible to our pipeline unless we synthesise an
	// echo. Without this, the user's own messages never appear in their
	// chat thread until the next history sync.
	ts := resp.Timestamp.Unix()
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	senderJID := jid.String()
	if id := c.wa.Store.ID; id != nil {
		senderJID = id.String()
	}
	md := MessageData{
		MessageID:   resp.ID,
		ChatJID:     jid.String(),
		SenderJID:   senderJID,
		Content:     &content,
		MessageType: "text",
		Timestamp:   ts,
		IsFromMe:    true,
	}
	emitMessages(c.accountID, []MessageData{md})
	return true, nil
}
