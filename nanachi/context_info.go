package main

import "go.mau.fi/whatsmeow/proto/waE2E"

// quoteInfo carrega o subset de ContextInfo que persistimos: o id da
// mensagem citada, o sender dela, um preview textual e os JIDs
// mencionados.
type quoteInfo struct {
	QuotedMessageID *string
	QuotedSenderID  *string
	QuotedPreview   *string
	MentionedJIDs   []string
}

// extractContextInfo lê o ContextInfo da variante apropriada e devolve
// os campos que a UI precisa para renderizar reply + menções.
func extractContextInfo(m *waE2E.Message) quoteInfo {
	var q quoteInfo
	if m == nil {
		return q
	}
	ci := getContextInfo(m)
	if ci == nil {
		return q
	}
	if id := ci.GetStanzaID(); id != "" {
		s := id
		q.QuotedMessageID = &s
	}
	if p := ci.GetParticipant(); p != "" {
		s := p
		q.QuotedSenderID = &s
	}
	if quoted := ci.GetQuotedMessage(); quoted != nil {
		if text, _ := extractContent(quoted); text != "" {
			t := text
			q.QuotedPreview = &t
		}
	}
	if mentioned := ci.GetMentionedJID(); len(mentioned) > 0 {
		q.MentionedJIDs = append([]string(nil), mentioned...)
	}
	return q
}

func getContextInfo(m *waE2E.Message) *waE2E.ContextInfo {
	switch {
	case m.ExtendedTextMessage != nil:
		return m.ExtendedTextMessage.GetContextInfo()
	case m.ImageMessage != nil:
		return m.ImageMessage.GetContextInfo()
	case m.VideoMessage != nil:
		return m.VideoMessage.GetContextInfo()
	case m.AudioMessage != nil:
		return m.AudioMessage.GetContextInfo()
	case m.DocumentMessage != nil:
		return m.DocumentMessage.GetContextInfo()
	case m.StickerMessage != nil:
		return m.StickerMessage.GetContextInfo()
	case m.ContactMessage != nil:
		return m.ContactMessage.GetContextInfo()
	case m.LocationMessage != nil:
		return m.LocationMessage.GetContextInfo()
	case m.LiveLocationMessage != nil:
		return m.LiveLocationMessage.GetContextInfo()
	case m.PollCreationMessage != nil:
		return m.PollCreationMessage.GetContextInfo()
	}
	return nil
}

// applyQuoteInfo escreve os campos extraídos no MessageData.
func applyQuoteInfo(md *MessageData, q quoteInfo) {
	if q.QuotedMessageID != nil {
		md.QuotedMessageID = q.QuotedMessageID
	}
	if q.QuotedSenderID != nil {
		md.QuotedSenderID = q.QuotedSenderID
	}
	if q.QuotedPreview != nil {
		md.QuotedPreview = q.QuotedPreview
	}
	if len(q.MentionedJIDs) > 0 {
		md.MentionedJIDs = q.MentionedJIDs
	}
}
