package main

import (
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

func (c *Client) onHistorySync(evt *events.HistorySync) {
	conv := evt.Data.GetConversations()
	total := 0
	for _, conversation := range conv {
		chatJID, err := types.ParseJID(conversation.GetID())
		if err != nil {
			continue
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
	emitHistorySyncComplete(c.accountID, total)
}
