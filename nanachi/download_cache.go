package main

import (
	"sync"

	"go.mau.fi/whatsmeow/proto/waE2E"
)

// downloadCache associa (account_id, message_id) ao *waE2E.Message
// recebido. É populado em extract* e consumido pelo handler de
// DownloadMedia. Mantém só os ponteiros — o whatsmeow já retém as
// estruturas pelo lifecycle do evento, e nosso uso (download tardio)
// é leitura, não escrita.
var downloadCache sync.Map // map[string]*waE2E.Message

func downloadKey(accountID, messageID string) string {
	return accountID + "|" + messageID
}

// rememberForDownload guarda o proto em cache se for um tipo baixável.
// Texto e payloads sem mídia não entram (poupa memória).
func rememberForDownload(accountID, messageID string, m *waE2E.Message) {
	if m == nil {
		return
	}
	switch {
	case m.ImageMessage != nil,
		m.VideoMessage != nil,
		m.AudioMessage != nil,
		m.StickerMessage != nil,
		m.DocumentMessage != nil:
		downloadCache.Store(downloadKey(accountID, messageID), m)
	}
}
