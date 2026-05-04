package main

import (
	"fmt"
	"os"
	"sort"

	"go.mau.fi/whatsmeow/proto/waHistorySync"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

// computeReadWatermark turns a HistorySync `Conversation` into a
// `last_read_ts` value the Rust side stamps onto `chats.last_read_ts`.
// The contract:
//
//   - `UnreadCount=0` → user has read everything; watermark is the
//     newest message in the conversation (so any future arrival
//     bumps the badge).
//   - `UnreadCount=N` → the most recent N incoming messages are
//     unread; watermark is the timestamp just BEFORE the Nth
//     newest incoming row, i.e. the last row the user actually saw.
//   - `MarkedAsUnread=true` → user explicitly tagged this chat as
//     unread; watermark stays at zero so the badge persists.
//
// Returns `(0, false)` when we don't have enough data to make a
// confident call (no messages in the chunk, missing timestamps).
// The caller skips emitting a hint in that case — the seed migration
// or a later chunk handles it.
func computeReadWatermark(conv *waHistorySync.Conversation) (int64, bool) {
	if conv.GetMarkedAsUnread() {
		return 0, true
	}
	msgs := conv.GetMessages()
	if len(msgs) == 0 {
		return 0, false
	}
	// Collect (timestamp, fromMe) pairs. We don't trust insertion
	// order — the iOS client streams oldest-first, Android
	// newest-first. Sort newest-first ourselves.
	type row struct {
		ts     int64
		fromMe bool
	}
	rows := make([]row, 0, len(msgs))
	for _, m := range msgs {
		wmi := m.GetMessage()
		if wmi == nil {
			continue
		}
		ts := int64(wmi.GetMessageTimestamp())
		if ts <= 0 {
			continue
		}
		fromMe := false
		if k := wmi.GetKey(); k != nil {
			fromMe = k.GetFromMe()
		}
		rows = append(rows, row{ts: ts, fromMe: fromMe})
	}
	if len(rows) == 0 {
		return 0, false
	}
	sort.Slice(rows, func(i, j int) bool { return rows[i].ts > rows[j].ts })

	unread := int(conv.GetUnreadCount())
	if unread <= 0 {
		// Everything seen — watermark = newest message.
		return rows[0].ts, true
	}
	// Walk newest-first, skip `unread` incoming rows, take the next
	// row's ts as the watermark. If the chunk doesn't carry enough
	// incoming messages to satisfy the count we fall back to one
	// second before the oldest row in the chunk — guarantees the
	// rows present here are flagged unread without dragging older
	// (already-read) rows back into the count.
	skipped := 0
	for _, r := range rows {
		if r.fromMe {
			continue
		}
		if skipped < unread {
			skipped++
			continue
		}
		return r.ts, true
	}
	if last := rows[len(rows)-1].ts; last > 0 {
		return last - 1, true
	}
	return 0, true
}

func (c *Client) onHistorySync(evt *events.HistorySync) {
	syncType := evt.Data.GetSyncType().String()
	progress := evt.Data.GetProgress()
	conv := evt.Data.GetConversations()
	// Mark the stream as live so OfflineSyncCompleted /
	// AppStateSyncComplete don't fire their fallback Complete and
	// close the syncing scene while we're still mid-bootstrap.
	c.historySyncSeen.Store(true)
	// Goes to stderr so the Rust side surfaces it as a log line —
	// stdout is reserved for the IPC JSON channel. Returning users
	// don't see the syncing scene (the UI goes straight to InApp),
	// so this is the only signal they have that whatsmeow is actually
	// working through the chunked HistorySync.
	fmt.Fprintf(os.Stderr,
		"[sync] HistorySync chunk: account=%s type=%s progress=%d%% conversations=%d\n",
		c.accountID, syncType, progress, len(conv),
	)
	// Emite antes do trabalho do chunk: a UI já sai de 0% assim que o
	// primeiro evento chega, mesmo que parsing/emit dos contatos
	// embutidos demore um pouco.
	emitHistorySyncProgress(c.accountID, syncType, progress)
	total := 0
	pins := make([]chatPinItem, 0)
	for _, conversation := range conv {
		chatJID, err := types.ParseJID(conversation.GetID())
		if err != nil {
			continue
		}
		// Newsletters that arrive via HistorySync but aren't in
		// `GetSubscribedNewsletters` come back without a name on
		// our side — the row falls back to its raw JID. Trigger an
		// async metadata fetch so the next ChatsUpserted carries the
		// resolved channel name + description. Idempotent — the
		// dedupSet keeps us from re-querying the same JID for every
		// chunk.
		if chatJID.Server == types.NewsletterServer && c.queueNewsletterRefresh(chatJID) {
			go c.refreshNewsletter(chatJID)
		}
		// `Pinned` on a Conversation is the unix-second when the user
		// pinned it (0 = not pinned). We only care about the boolean
		// flip; the timestamp would let us preserve pin order but
		// `chats.pinned` in tina.db is a plain bool today.
		if conversation.GetPinned() > 0 {
			pins = append(pins, chatPinItem{
				ChatJID: conversation.GetID(),
				Pinned:  true,
			})
		}
		msgs := make([]MessageData, 0, len(conversation.GetMessages()))
		for _, hm := range conversation.GetMessages() {
			wmi := hm.GetMessage()
			if wmi == nil {
				continue
			}
			md := mapWebMessageInfo(chatJID, wmi)
			if md != nil {
				rememberForDownload(c.accountID, md.MessageID, wmi.GetMessage())
				msgs = append(msgs, *md)
			}
			// Aproveita o push name embutido no WebMessageInfo — é a
			// fonte mais densa de nomes durante o history sync inicial.
			if pn := wmi.GetPushName(); pn != "" {
				key := wmi.GetKey()
				if key != nil && !key.GetFromMe() {
					senderStr := key.GetParticipant()
					if senderStr == "" {
						senderStr = key.GetRemoteJID()
					}
					if sender, perr := types.ParseJID(senderStr); perr == nil {
						emitSenderContact(c.accountID, sender, types.EmptyJID, pn)
					}
				}
			}
		}
		// Chunkamos pra não estourar o buffer do pipe stdout (~64KB no
		// Linux): conversation com 5k mensagens vira uma linha JSON de
		// vários MB e bloqueia o Go até o Rust drenar. 500 msg/lote é
		// um sweet spot empírico.
		const msgBatch = 500
		for i := 0; i < len(msgs); i += msgBatch {
			j := i + msgBatch
			if j > len(msgs) {
				j = len(msgs)
			}
			emitMessages(c.accountID, msgs[i:j])
		}
		total += len(msgs)
	}
	c.historyCount.Add(int64(total))
	// Pin updates ride out alongside the message stream — the realtime
	// handler tolerates rows that don't exist yet (logs and skips), so
	// even if a pin lands before its conversation's first message
	// flush we just need any later upsert to bring the row in and the
	// next pin batch to catch it.
	emitChatsPinUpdate(c.accountID, pins)
	// Read-watermark seeding. WhatsApp ships per-conversation unread
	// counts in HistorySync; turn them into a `last_read_ts` for each
	// chat so the sidebar shows the same numbers your phone does
	// instead of "everything in the backlog is unread". Computed
	// from the conversation's messages — see `computeReadWatermark`.
	hints := make([]chatReadHintItem, 0, len(conv))
	for _, conversation := range conv {
		if ts, ok := computeReadWatermark(conversation); ok {
			hints = append(hints, chatReadHintItem{
				ChatJID:    conversation.GetID(),
				LastReadTs: ts,
			})
		}
	}
	if len(hints) > 0 {
		emitChatsReadHint(c.accountID, hints)
	}
	// Antes este emit acontecia em todo chunk e fazia a UI pular pra
	// "InApp" no primeiro pacote — anulando a tela de progresso.
	// Agora só sinaliza completo quando o progress reportado atinge 100.
	// OfflineSyncCompleted / AppStateSyncComplete continuam como rede
	// de proteção pra dispositivos que nunca chegam a 100.
	if progress >= 100 {
		fmt.Fprintf(os.Stderr,
			"[sync] HistorySync 100%% — emitting Complete (cumulative messages=%d)\n",
			c.historyCount.Load(),
		)
		emitHistorySyncComplete(c.accountID, int(c.historyCount.Load()))
	}
}
