package main

import (
	"time"

	"go.mau.fi/whatsmeow/proto/waWeb"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

// emitSenderContact registra um contato (com push name) a partir do sender de
// uma mensagem. Lida com Sender e SenderAlt, populando JID + LID quando
// ambos estão disponíveis.
func emitSenderContact(accountID string, sender, alt types.JID, pushName string) {
	if sender.IsEmpty() {
		return
	}
	jidPtr, lidPtr := splitJIDLID(sender, alt)
	primary := jidPtr
	altLid := lidPtr
	if primary == nil {
		// Só temos LID — registra com LID como id primário.
		s := sender.String()
		primary = &s
		altLid = nil
	}
	phone := phoneOf(sender)
	if phone == nil {
		phone = phoneOf(alt)
	}
	cd := ContactData{
		JID:         *primary,
		LID:         altLid,
		PhoneNumber: phone,
	}
	if pushName != "" {
		n := pushName
		cd.Notify = &n
	}
	emitContacts(accountID, []ContactData{cd})
}

func contactFromEvent(evt *events.Contact) ContactData {
	c := ContactData{
		JID:         evt.JID.String(),
		PhoneNumber: phoneOf(evt.JID),
	}
	if evt.JID.Server == types.HiddenUserServer {
		// Se o evento veio com um LID como identidade primária, registra
		// também o LID na coluna dedicada.
		s := evt.JID.String()
		c.LID = &s
	}
	if evt.Action != nil {
		if name := evt.Action.GetFullName(); name != "" {
			n := name
			c.Name = &n
		} else if name := evt.Action.GetFirstName(); name != "" {
			n := name
			c.Name = &n
		}
	}
	return c
}

func newsletterToGroup(n *types.NewsletterMetadata) GroupData {
	name := n.ThreadMeta.Name.Text
	desc := n.ThreadMeta.Description.Text
	var avatarURL *string
	if n.ThreadMeta.Picture != nil && n.ThreadMeta.Picture.URL != "" {
		avatarURL = &n.ThreadMeta.Picture.URL
	}
	return GroupData{
		JID:          n.ID.String(),
		Subject:      strPtr(name),
		Description:  strPtr(desc),
		AvatarURL:    avatarURL,
		Participants: []ParticipantData{},
	}
}

func groupFromInfo(g *types.GroupInfo) GroupData {
	parts := make([]ParticipantData, 0, len(g.Participants))
	for _, p := range g.Participants {
		var admin *string
		switch {
		case p.IsSuperAdmin:
			a := "superadmin"
			admin = &a
		case p.IsAdmin:
			a := "admin"
			admin = &a
		}
		// Identificador primário do participante: prefere PN para casar com
		// `contacts.jid`. Cai pro LID se PN não estiver disponível.
		var phone *string
		if !p.PhoneNumber.IsEmpty() {
			phone = phoneOf(p.PhoneNumber)
		}
		if phone == nil {
			phone = phoneOf(p.JID)
		}
		parts = append(parts, ParticipantData{
			ID:          p.JID.String(),
			Admin:       admin,
			PhoneNumber: phone,
		})
	}
	return GroupData{
		JID:          g.JID.String(),
		Subject:      strPtr(g.Name),
		Owner:        strPtr(g.OwnerJID.String()),
		Description:  strPtr(g.Topic),
		Participants: parts,
	}
}

// participantContacts produz um ContactData "esqueleto" por participante
// preenchendo JID + LID + PhoneNumber sempre que essa informação está
// disponível, para a tabela `contacts` conseguir resolver lookups por
// qualquer das formas.
func participantContacts(g *types.GroupInfo) []ContactData {
	out := make([]ContactData, 0, len(g.Participants))
	for _, p := range g.Participants {
		jid, lid := splitJIDLID(p.PhoneNumber, p.LID)
		if jid == nil && lid == nil {
			// Nenhum dos dois explícitos; cai no JID primário.
			s := p.JID.String()
			jid = &s
		}
		if jid == nil {
			// Só temos LID — usa como chave primária.
			jid = lid
			lid = nil
		}
		out = append(out, ContactData{
			JID:         *jid,
			LID:         lid,
			PhoneNumber: phoneOf(p.PhoneNumber),
		})
	}
	return out
}

func mapMessage(evt *events.Message) *MessageData {
	if evt.Info.ID == "" {
		return nil
	}
	// Mensagens "peer" são sync interno entre meus próprios dispositivos
	// (devicesentmeta, app-state etc). Não têm conteúdo de usuário.
	if evt.Info.Category == "peer" {
		return nil
	}
	content, mtype := extractContent(evt.Message)
	if mtype == "unknown" {
		// Provável protocolMessage / senderKeyDistribution / receiptSync.
		// Sem conteúdo legível — pula.
		return nil
	}
	ts := evt.Info.Timestamp.Unix()
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	sender := evt.Info.Sender.String()
	if sender == "" {
		sender = evt.Info.Chat.String()
	}
	md := MessageData{
		MessageID:   evt.Info.ID,
		ChatJID:     evt.Info.Chat.String(),
		SenderJID:   sender,
		MessageType: mtype,
		Timestamp:   ts,
		IsFromMe:    evt.Info.IsFromMe,
	}
	if content != "" {
		md.Content = &content
	}
	if raw, ok := marshalProto(evt.Message); ok {
		md.RawJSON = &raw
	}
	if thumb := extractThumbnail(evt.Message); len(thumb) > 0 {
		md.Thumbnail = thumb
	}
	applyMedia(&md, extractMedia(evt.Message))
	applyQuoteInfo(&md, extractContextInfo(evt.Message))
	return &md
}

func mapWebMessageInfo(chat types.JID, wmi *waWeb.WebMessageInfo) *MessageData {
	key := wmi.GetKey()
	if key == nil || key.GetID() == "" {
		return nil
	}
	content, mtype := extractContent(wmi.GetMessage())
	if mtype == "unknown" {
		return nil
	}
	ts := int64(wmi.GetMessageTimestamp())
	if ts <= 0 {
		ts = time.Now().Unix()
	}
	sender := key.GetParticipant()
	if sender == "" {
		sender = chat.String()
	}
	md := MessageData{
		MessageID:   key.GetID(),
		ChatJID:     chat.String(),
		SenderJID:   sender,
		MessageType: mtype,
		Timestamp:   ts,
		IsFromMe:    key.GetFromMe(),
	}
	if content != "" {
		md.Content = &content
	}
	if raw, ok := marshalProto(wmi.GetMessage()); ok {
		md.RawJSON = &raw
	}
	if thumb := extractThumbnail(wmi.GetMessage()); len(thumb) > 0 {
		md.Thumbnail = thumb
	}
	applyMedia(&md, extractMedia(wmi.GetMessage()))
	applyQuoteInfo(&md, extractContextInfo(wmi.GetMessage()))
	return &md
}
