package main

import (
	"fmt"
	"os"
	"time"

	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

// fallbackHistoryCompleteDelay is how long we wait after
// OfflineSyncCompleted / AppStateSyncComplete before emitting a
// fallback HistorySyncComplete. The first HistorySync chunk on this
// account historically lands ~4 s after Connected, so 10 s leaves
// generous headroom while still recovering devices that genuinely
// never receive a chunk (the original comment's "stuck on Syncing"
// case).
const fallbackHistoryCompleteDelay = 10 * time.Second

// handleEvent é registrado em Client.wa.AddEventHandler.
func (c *Client) handleEvent(rawEvt any) {
	switch evt := rawEvt.(type) {
	case *events.Connected:
		fmt.Fprintf(os.Stderr, "[sync] Connected event for %s\n", c.accountID)
		c.onConnected()

	case *events.PairSuccess:
		_ = c.mgr.saveDeviceJID(c.accountID, evt.ID.String())

	case *events.LoggedOut:
		_ = c.mgr.clearDeviceJID(c.accountID)
		emitLoggedOut(c.accountID)
		c.mgr.mu.Lock()
		delete(c.mgr.clients, c.accountID)
		c.mgr.mu.Unlock()
		c.wa.Disconnect()

	case *events.Disconnected:
		emitDisconnected(c.accountID, "transport disconnected")

	case *events.StreamReplaced:
		emitDisconnected(c.accountID, "stream replaced")

	case *events.Message:
		c.handleMessage(evt)

	case *events.Receipt:
		c.handleReceipt(evt)

	case *events.HistorySync:
		c.onHistorySync(evt)

	case *events.OfflineSyncCompleted:
		fmt.Fprintf(os.Stderr,
			"[sync] OfflineSyncCompleted for %s (cumulative messages=%d)\n",
			c.accountID, c.historyCount.Load(),
		)
		if c.inReconnectSync.CompareAndSwap(true, false) {
			// We emitted a synthetic HistorySyncProgress on reconnect;
			// dismiss the "Catching up" indicator now that the offline
			// queue has fully drained.
			fmt.Fprintf(os.Stderr,
				"[sync] reconnect offline queue drained for %s — emitting Complete\n",
				c.accountID,
			)
			emitHistorySyncComplete(c.accountID, int(c.historyCount.Load()))
		} else {
			// Fallback for fresh-pair devices that genuinely never get
			// any HistorySync chunk. We can't emit Complete inline — the
			// real chunk stream typically lands several seconds AFTER
			// OfflineSyncCompleted, and emitting now closes the syncing
			// page before the first chunk arrives. Defer the check; if
			// `historySyncSeen` flipped during the wait, the chunk
			// stream's own progress=100 path will close the page and
			// this fallback no-ops.
			c.scheduleFallbackHistoryComplete()
		}
		// Reconcile automático: nesse ponto o whatsmeow já populou seu
		// próprio store de contatos com push names do app-state. Re-emitimos
		// pro tina pra preencher os nomes que escaparam dos eventos.
		go c.reconcile()

	case *events.AppStateSyncComplete:
		fmt.Fprintf(os.Stderr,
			"[sync] AppStateSyncComplete for %s\n", c.accountID,
		)
		// Same delayed-fallback strategy as OfflineSyncCompleted.
		c.scheduleFallbackHistoryComplete()
		go c.reconcile()

	case *events.Contact:
		emitContacts(c.accountID, []ContactData{contactFromEvent(evt)})

	case *events.PushName:
		c.handlePushName(evt)

	case *events.GroupInfo:
		c.refreshGroup(evt.JID)

	case *events.JoinedGroup:
		emitGroups(c.accountID, []GroupData{groupFromInfo(&evt.GroupInfo)})

	case *events.NewsletterJoin:
		emitGroups(c.accountID, []GroupData{newsletterToGroup(&evt.NewsletterMetadata)})

	case *events.NewsletterLeave:
		// O whatsmeow só nos diz que saímos; emitimos um GroupsUpsert vazio
		// pra manter coerência (a UI ainda vai ver o chat com mensagens
		// passadas; remoção total fica para um evento dedicado no futuro).
		_ = evt
	}
}

// handleReceipt maps whatsmeow's events.Receipt onto our wire-level
// "delivery_status" string. We only care about the receipt types
// that surface in the bubble's status icon: delivered, read, played.
// Sender / Retry / etc. are silently dropped.
func (c *Client) handleReceipt(evt *events.Receipt) {
	if len(evt.MessageIDs) == 0 {
		return
	}
	status := receiptStatus(evt.Type)
	if status == "" {
		return
	}
	emitReceiptUpdate(c.accountID, evt.MessageIDs, status)
}

func receiptStatus(t types.ReceiptType) string {
	switch t {
	case types.ReceiptTypeDelivered:
		return "delivered"
	case types.ReceiptTypeRead, types.ReceiptTypeReadSelf:
		return "read"
	case types.ReceiptTypePlayed, types.ReceiptTypePlayedSelf:
		return "played"
	}
	return ""
}

func (c *Client) handleMessage(evt *events.Message) {
	if md := mapMessage(evt); md != nil {
		// Cache the proto for later DownloadMedia requests. Cheap when
		// it's a non-media payload (rememberForDownload short-circuits).
		rememberForDownload(c.accountID, md.MessageID, evt.Message)
		emitMessages(c.accountID, []MessageData{*md})
	}
	// Persiste o sender com push_name quando disponível. É o caminho
	// principal de resolução de nomes — events.PushName/Contact são
	// raros e o app-state-sync nem sempre traz tudo. Não condicionamos
	// a SenderAlt: a maioria das mensagens em DM só carrega Sender.
	if !evt.Info.Sender.IsEmpty() && evt.Info.PushName != "" {
		emitSenderContact(c.accountID, evt.Info.Sender, evt.Info.SenderAlt, evt.Info.PushName)
	}
}

func (c *Client) handlePushName(evt *events.PushName) {
	jid, lid := splitJIDLID(evt.JID, evt.JIDAlt)
	// Quando só veio uma forma e ela é PN, usa como JID; só LID, vira LID.
	fallbackJID := evt.JID.String()
	if jid == nil && lid == nil {
		jid = &fallbackJID
	} else if jid == nil {
		// Sem PN — mas precisamos preencher `jid` (PK no DB Rust);
		// usamos a forma LID como identificador primário.
		jid = lid
		lid = nil
	}
	notify := evt.NewPushName
	var phone *string
	if evt.JID.Server == types.DefaultUserServer {
		u := evt.JID.User
		phone = &u
	} else if !evt.JIDAlt.IsEmpty() && evt.JIDAlt.Server == types.DefaultUserServer {
		u := evt.JIDAlt.User
		phone = &u
	}
	emitContacts(c.accountID, []ContactData{{
		JID:         *jid,
		LID:         lid,
		PhoneNumber: phone,
		Notify:      strPtr(notify),
	}})
}
