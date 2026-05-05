package main

import "go.mau.fi/whatsmeow/proto/waE2E"

// extractContent inspeciona um *waE2E.Message e retorna (texto, tipo).
// Para tipos não-texto, devolve um placeholder estilo "[Image]" para
// preservar a UX da implementação anterior em TS.
func extractContent(m *waE2E.Message) (string, string) {
	if m == nil {
		return "", "unknown"
	}
	switch {
	case m.Conversation != nil && *m.Conversation != "":
		return *m.Conversation, "text"
	case m.ExtendedTextMessage != nil:
		if t := m.ExtendedTextMessage.GetText(); t != "" {
			return t, "text"
		}
		return "", "text"
	case m.ImageMessage != nil:
		if cap := m.ImageMessage.GetCaption(); cap != "" {
			return cap, "image"
		}
		return "[Image]", "image"
	case m.VideoMessage != nil:
		if cap := m.VideoMessage.GetCaption(); cap != "" {
			return cap, "video"
		}
		return "[Video]", "video"
	case m.AudioMessage != nil:
		return "[Audio]", "audio"
	case m.DocumentMessage != nil:
		if cap := m.DocumentMessage.GetCaption(); cap != "" {
			return cap, "document"
		}
		return "[Document]", "document"
	case m.StickerMessage != nil:
		return "[Sticker]", "sticker"
	case m.LottieStickerMessage != nil:
		// Lottie / animated stickers are wrapped in FutureProofMessage.
		// The inner message holds the same StickerMessage download fields.
		return "[Sticker]", "sticker"
	case m.ContactMessage != nil:
		return "[Contact]", "contact"
	case m.LocationMessage != nil:
		return "[Location]", "location"
	case m.LiveLocationMessage != nil:
		return "[Live Location]", "location"
	case m.ReactionMessage != nil:
		return m.ReactionMessage.GetText(), "reaction"
	case m.PollCreationMessage != nil:
		return m.PollCreationMessage.GetName(), "poll"
	}
	return "", "unknown"
}
