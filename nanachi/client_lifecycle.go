package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sync"
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
	// isReturningUser is true when the device already had a paired JID
	// at the time newClient was called (i.e. the account was previously
	// registered). False for brand-new devices going through QR pairing.
	// Immutable after construction — safe to read without a lock.
	isReturningUser bool
	// firstConnected flips to true on the first onConnected call so we
	// can distinguish "startup" (false) from "mid-session reconnect" (true).
	firstConnected atomic.Bool
	// inReconnectSync is set to true in onConnected for returning users
	// (both startup and mid-session reconnect). It gates a synthetic
	// HistorySyncProgress emit so the UI shows "Catching up" while the
	// offline message queue drains. Cleared by OfflineSyncCompleted.
	inReconnectSync atomic.Bool
	// newsletterRefreshes dedupes async newsletter-info fetches
	// across HistorySync chunks. The same channel JID often shows
	// up in multiple chunks during the initial bootstrap; without
	// this we'd fire N duplicate GraphQL queries.
	newsletterRefreshes sync.Map
}

// queueNewsletterRefresh returns true the first time it sees a JID
// (the caller should kick off the fetch). Subsequent calls return
// false — caller skips. Resets are not needed: newsletter JIDs
// don't recycle within a session.
func (c *Client) queueNewsletterRefresh(jid types.JID) bool {
	_, loaded := c.newsletterRefreshes.LoadOrStore(jid.String(), struct{}{})
	return !loaded
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
		mgr:             mgr,
		accountID:       accountID,
		wa:              wa,
		isReturningUser: device.ID != nil,
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

// markRead sends a Read receipt for one or more incoming messages
// in a chat. The whatsmeow API requires that all ids belong to the
// same sender per call; we filter / accept whatever the caller
// provided since the UI already groups by sender JID.
func (c *Client) markRead(p MarkReadPayload) error {
	if !c.wa.IsConnected() {
		return errors.New("client not connected")
	}
	if len(p.MessageIDs) == 0 {
		return nil
	}
	chatJID, err := types.ParseJID(p.ChatJID)
	if err != nil {
		return fmt.Errorf("invalid chat jid: %w", err)
	}
	senderJID, err := types.ParseJID(p.SenderJID)
	if err != nil {
		return fmt.Errorf("invalid sender jid: %w", err)
	}
	ids := make([]types.MessageID, 0, len(p.MessageIDs))
	for _, id := range p.MessageIDs {
		ids = append(ids, types.MessageID(id))
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return c.wa.MarkRead(ctx, ids, time.Now(), chatJID, senderJID)
}

func (c *Client) send(to, content, localID string, mentioned []string) (bool, error) {
	jid, err := types.ParseJID(to)
	if err != nil {
		return false, fmt.Errorf("invalid jid: %w", err)
	}
	if !c.wa.IsConnected() {
		return false, errors.New("client not connected")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	// Whatsmeow's `Conversation` field doesn't carry context info,
	// so any message with mentions has to ride on
	// `ExtendedTextMessage`. We keep `Conversation` as the
	// lightweight default to avoid changing the wire shape for
	// every plain-text message.
	var msg *waE2E.Message
	if len(mentioned) > 0 {
		ctxInfo := &waE2E.ContextInfo{
			MentionedJID: append([]string(nil), mentioned...),
		}
		msg = &waE2E.Message{
			ExtendedTextMessage: &waE2E.ExtendedTextMessage{
				Text:        &content,
				ContextInfo: ctxInfo,
			},
		}
	} else {
		msg = &waE2E.Message{Conversation: &content}
	}

	var extra []whatsmeow.SendRequestExtra
	if localID != "" {
		extra = append(extra, whatsmeow.SendRequestExtra{ID: localID})
	}
	resp, err := c.wa.SendMessage(ctx, jid, msg, extra...)
	if err != nil {
		return false, err
	}

	ts := resp.Timestamp.Unix()
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	senderJID := jid.String()
	if id := c.wa.Store.ID; id != nil {
		senderJID = id.String()
	}

	// Echo with resp.ID. When localID != "" and SendRequestExtra was used,
	// resp.ID == localID, so the pre-inserted optimistic row gets a
	// guaranteed INSERT OR IGNORE no-op. Legacy path (localID == "") uses
	// the WA-assigned ID directly.
	md := MessageData{
		MessageID:   resp.ID,
		ChatJID:     jid.String(),
		SenderJID:   senderJID,
		Content:     &content,
		MessageType: "text",
		Timestamp:   ts,
		IsFromMe:    true,
	}
	if len(mentioned) > 0 {
		md.MentionedJIDs = append([]string(nil), mentioned...)
	}
	emitMessages(c.accountID, []MessageData{md})
	return true, nil
}
