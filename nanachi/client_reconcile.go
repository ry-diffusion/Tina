package main

import (
	"context"
	"fmt"
	"time"

	"go.mau.fi/whatsmeow/types"
)

// reconcile re-emite contatos/grupos/newsletters a partir do que o
// whatsmeow já tem em cache local — sem ir na rede.
func (c *Client) reconcile() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	c.reconcileContacts(ctx)
	c.reconcileGroups(ctx)
	c.reconcileNewsletters(ctx)

	emitReconcileProgress(c.accountID, "Concluído", 1, 1, false)
	emitHistorySyncComplete(c.accountID, 0)
}

func (c *Client) reconcileContacts(ctx context.Context) {
	emitReconcileProgress(c.accountID, "Lendo contatos do WhatsApp…", 0, 0, true)
	all, err := c.wa.Store.Contacts.GetAllContacts(ctx)
	if err != nil {
		emitError(&c.accountID, fmt.Sprintf("reconcile contacts: %v", err))
		return
	}
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

func (c *Client) reconcileGroups(ctx context.Context) {
	// Cada GroupData carrega a lista completa de participantes; um pacote
	// com 144 grupos vira uma linha JSON de vários MB e estoura o buffer
	// do pipe stdout. Chunkamos em batches pequenos.
	const groupBatch = 10

	emitReconcileProgress(c.accountID, "Carregando grupos…", 0, 0, true)
	groups, err := c.wa.GetJoinedGroups(ctx)
	if err != nil {
		emitError(&c.accountID, fmt.Sprintf("get joined groups: %v", err))
		return
	}
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

func (c *Client) reconcileNewsletters(ctx context.Context) {
	const newsletterBatch = 20

	emitReconcileProgress(c.accountID, "Carregando newsletters…", 0, 0, true)
	newsletters, err := c.wa.GetSubscribedNewsletters(ctx)
	if err != nil {
		return
	}
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
