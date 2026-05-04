package main

import (
	"fmt"
	"os"

	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

func (c *Client) onHistorySync(evt *events.HistorySync) {
	syncType := evt.Data.GetSyncType().String()
	progress := evt.Data.GetProgress()
	conv := evt.Data.GetConversations()
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
