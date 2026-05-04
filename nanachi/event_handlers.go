package main

import (
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
		c.handleMessage(evt)

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
