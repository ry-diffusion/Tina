package main

import (
	"context"
	"fmt"
	"strings"
	"time"

	"go.mau.fi/whatsmeow/types"
)

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
	var pushName *string
	if pn := c.wa.Store.PushName; pn != "" {
		pushName = &pn
	}
	emitConnected(c.accountID, phone, jid, pushName)

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

func (c *Client) refreshGroup(jid types.JID) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	info, err := c.wa.GetGroupInfo(ctx, jid)
	if err != nil {
		return
	}
	emitGroups(c.accountID, []GroupData{groupFromInfo(info)})
}
