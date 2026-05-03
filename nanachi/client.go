package main

import (
	"context"
	"errors"
	"fmt"
	"sync/atomic"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
)

// Client encapsula um *whatsmeow.Client por account_id.
type Client struct {
	mgr       *Manager
	accountID string
	wa        *whatsmeow.Client

	historyCount atomic.Int64
}

func newClient(mgr *Manager, accountID string, device *store.Device) *Client {
	wa := whatsmeow.NewClient(device, mgr.logger)
	wa.EnableAutoReconnect = true
	// Por padrão, eventos como Contact/PushName/BusinessName só são emitidos
	// DEPOIS que o app-state full sync termina. Ligando isso eles saem
	// durante o sync, e a UI vai resolvendo nomes em tempo real em vez de
	// receber tudo no fim — melhora muito a experiência do primeiro login.
	wa.EmitAppStateEventsOnFullSync = true
	c := &Client{
		mgr:       mgr,
		accountID: accountID,
		wa:        wa,
	}
	wa.AddEventHandler(c.handleEvent)
	return c
}

// connect inicia a sessão. Para um device novo, abre o canal de QR antes
// de chamar Connect (Connect com store.ID == nil dispara o pareamento).
func (c *Client) connect(ctx context.Context) error {
	if c.wa.Store.ID == nil {
		qrChan, err := c.wa.GetQRChannel(ctx)
		if err != nil {
			return fmt.Errorf("get qr channel: %w", err)
		}
		go c.consumeQR(qrChan)
	}
	if err := c.wa.Connect(); err != nil {
		return fmt.Errorf("connect: %w", err)
	}
	return nil
}

func (c *Client) consumeQR(ch <-chan whatsmeow.QRChannelItem) {
	for evt := range ch {
		switch evt.Event {
		case "code":
			emitQR(c.accountID, evt.Code)
		case "success":
			// pareou — Connected será emitido pelo handler de eventos.
			return
		case "timeout", "err-client-outdated", "err-scanned-without-multidevice":
			emitError(&c.accountID, fmt.Sprintf("pairing %s", evt.Event))
			return
		}
	}
}

func (c *Client) disconnect(reason string) {
	c.wa.Disconnect()
	emitDisconnected(c.accountID, reason)
}

// reconcile re-emite contatos/grupos/newsletters a partir do que o
// whatsmeow já tem em cache local — sem ir na rede.
func (c *Client) reconcile() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	// ---- Etapa 1: contatos ----
	emitReconcileProgress(c.accountID, "Lendo contatos do WhatsApp…", 0, 0, true)
	all, err := c.wa.Store.Contacts.GetAllContacts(ctx)
	if err != nil {
		emitError(&c.accountID, fmt.Sprintf("reconcile contacts: %v", err))
	} else {
		out := make([]ContactData, 0, len(all))
		for jid, info := range all {
			cd := ContactData{JID: jid.String()}
			cd.PhoneNumber = phoneOf(jid)
			if jid.Server == types.HiddenUserServer {
				s := jid.String()
				cd.LID = &s
			}
			if info.PushName != "" {
				n := info.PushName
				cd.Notify = &n
			}
			name := info.FullName
			if name == "" {
				name = info.FirstName
			}
			if name != "" {
				cd.Name = &name
			}
			if info.BusinessName != "" {
				vn := info.BusinessName
				cd.VerifiedName = &vn
			}
			out = append(out, cd)
		}
		total := len(out)
		emitReconcileProgress(c.accountID,
			fmt.Sprintf("Importando %d contatos…", total), 0, total, false)
		for i := 0; i < total; i += 200 {
			j := i + 200
			if j > total {
				j = total
			}
			emitContacts(c.accountID, out[i:j])
			emitReconcileProgress(c.accountID,
				"Importando contatos…", j, total, false)
		}
	}

	// ---- Etapa 2: grupos ----
	// Cada GroupData carrega a lista completa de participantes; um pacote
	// com 144 grupos vira uma linha JSON de vários MB e estoura o buffer
	// do pipe stdout. Chunkamos em batches pequenos.
	const groupBatch = 10

	emitReconcileProgress(c.accountID, "Carregando grupos…", 0, 0, true)
	groups, err := c.wa.GetJoinedGroups(ctx)
	if err != nil {
		emitError(&c.accountID, fmt.Sprintf("get joined groups: %v", err))
	} else {
		total := len(groups)
		emitReconcileProgress(c.accountID,
			fmt.Sprintf("Importando %d grupos…", total), 0, total, false)
		for i := 0; i < total; i += groupBatch {
			j := i + groupBatch
			if j > total {
				j = total
			}
			mapped := make([]GroupData, 0, j-i)
			contacts := make([]ContactData, 0)
			for _, g := range groups[i:j] {
				mapped = append(mapped, groupFromInfo(g))
				contacts = append(contacts, participantContacts(g)...)
			}
			emitGroups(c.accountID, mapped)
			if len(contacts) > 0 {
				emitContacts(c.accountID, contacts)
			}
			emitReconcileProgress(c.accountID, "Importando grupos…", j, total, false)
		}
	}

	// ---- Etapa 3: newsletters ----
	const newsletterBatch = 20

	emitReconcileProgress(c.accountID, "Carregando newsletters…", 0, 0, true)
	if newsletters, err := c.wa.GetSubscribedNewsletters(ctx); err == nil {
		total := len(newsletters)
		emitReconcileProgress(c.accountID,
			fmt.Sprintf("Importando %d newsletters…", total), 0, total, false)
		for i := 0; i < total; i += newsletterBatch {
			j := i + newsletterBatch
			if j > total {
				j = total
			}
			mapped := make([]GroupData, 0, j-i)
			for _, n := range newsletters[i:j] {
				mapped = append(mapped, newsletterToGroup(n))
			}
			if len(mapped) > 0 {
				emitGroups(c.accountID, mapped)
			}
			emitReconcileProgress(c.accountID, "Importando newsletters…", j, total, false)
		}
	}

	// ---- Fim ----
	emitReconcileProgress(c.accountID, "Concluído", 1, 1, false)
	emitHistorySyncComplete(c.accountID, 0)
}

func (c *Client) send(to, content string) (bool, error) {
	jid, err := types.ParseJID(to)
	if err != nil {
		return false, fmt.Errorf("invalid jid: %w", err)
	}
	if !c.wa.IsConnected() {
		return false, errors.New("client not connected")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	_, err = c.wa.SendMessage(ctx, jid, &waE2E.Message{
		Conversation: &content,
	})
	if err != nil {
		return false, err
	}
	return true, nil
}
