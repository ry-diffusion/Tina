package main

import (
	"context"
	"fmt"
	"strings"
	"time"

	"go.mau.fi/whatsmeow/proto/waWeb"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

// handleEvent é registrado em Client.wa.AddEventHandler.
func (c *Client) handleEvent(rawEvt any) {
	switch evt := rawEvt.(type) {
	case *events.Connected:
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
		if md := mapMessage(evt); md != nil {
			emitMessages(c.accountID, []MessageData{*md})
		}
		// Persiste o sender com push_name quando disponível. É o caminho
		// principal de resolução de nomes — events.PushName/Contact são
		// raros e o app-state-sync nem sempre traz tudo. Não condicionamos
		// a SenderAlt: a maioria das mensagens em DM só carrega Sender.
		if !evt.Info.Sender.IsEmpty() && evt.Info.PushName != "" {
			emitSenderContact(c.accountID, evt.Info.Sender, evt.Info.SenderAlt, evt.Info.PushName)
		}

	case *events.HistorySync:
		c.onHistorySync(evt)

	case *events.OfflineSyncCompleted:
		// Sinal canônico do whatsmeow de que a sincronização inicial
		// (mensagens offline + app state) acabou. Sem isso, a UI fica
		// travada na tela "Syncing Messages" esperando HistorySync.
		emitHistorySyncComplete(c.accountID, int(c.historyCount.Load()))
		// Reconcile automático: nesse ponto o whatsmeow já populou seu
		// próprio store de contatos com push names do app-state. Re-emitimos
		// pro tina pra preencher os nomes que escaparam dos eventos.
		go c.reconcile()

	case *events.AppStateSyncComplete:
		// Backup: alguns dispositivos só emitem AppStateSyncComplete.
		emitHistorySyncComplete(c.accountID, int(c.historyCount.Load()))
		go c.reconcile()

	case *events.Contact:
		emitContacts(c.accountID, []ContactData{contactFromEvent(evt)})

	case *events.PushName:
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

func (c *Client) onConnected() {
	var phone, jid *string
	if id := c.wa.Store.ID; id != nil {
		j := id.String()
		jid = &j
		// JID do WhatsApp é "5511...@s.whatsapp.net"; o número fica antes do @.
		if idx := strings.IndexByte(j, '@'); idx > 0 {
			p := j[:idx]
			if dot := strings.IndexByte(p, ':'); dot > 0 {
				p = p[:dot]
			}
			phone = &p
		}
		_ = c.mgr.saveDeviceJID(c.accountID, j)
	}
	emitConnected(c.accountID, phone, jid)

	go c.fetchAllGroups()
	go c.fetchAllNewsletters()
}

func (c *Client) fetchAllGroups() {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	groups, err := c.wa.GetJoinedGroups(ctx)
	if err != nil {
		emitError(&c.accountID, fmt.Sprintf("get joined groups: %v", err))
		return
	}
	mapped := make([]GroupData, 0, len(groups))
	contacts := make([]ContactData, 0)
	for _, g := range groups {
		mapped = append(mapped, groupFromInfo(g))
		contacts = append(contacts, participantContacts(g)...)
	}
	emitGroups(c.accountID, mapped)
	emitContacts(c.accountID, contacts)
}

func (c *Client) fetchAllNewsletters() {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	newsletters, err := c.wa.GetSubscribedNewsletters(ctx)
	if err != nil {
		// Não é fatal — alguns devices não têm newsletters habilitadas.
		emitError(&c.accountID, fmt.Sprintf("get subscribed newsletters: %v", err))
		return
	}
	mapped := make([]GroupData, 0, len(newsletters))
	for _, n := range newsletters {
		mapped = append(mapped, newsletterToGroup(n))
	}
	emitGroups(c.accountID, mapped)
}

func newsletterToGroup(n *types.NewsletterMetadata) GroupData {
	name := n.ThreadMeta.Name.Text
	desc := n.ThreadMeta.Description.Text
	return GroupData{
		JID:          n.ID.String(),
		Subject:      strPtr(name),
		Description:  strPtr(desc),
		Participants: nil,
	}
}

func (c *Client) refreshGroup(jid types.JID) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	info, err := c.wa.GetGroupInfo(ctx, jid)
	if err != nil {
		return
	}
	emitGroups(c.accountID, []GroupData{groupFromInfo(info)})
}

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
	return &md
}
