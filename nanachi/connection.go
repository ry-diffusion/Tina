package main

import (
	"context"
	"fmt"
	"os"
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
	fmt.Fprintf(os.Stderr, "[newsletter] GetSubscribedNewsletters: %d channels\n", len(newsletters))
	mapped := make([]GroupData, 0, len(newsletters))
	for _, n := range newsletters {
		fmt.Fprintf(os.Stderr, "[newsletter] channel %s name=%q\n", n.ID, n.ThreadMeta.Name.Text)
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

// refreshChat is the IPC entry point for `Cmd::RefreshChat`. Picks
// the right whatsmeow API based on the JID's server: newsletter
// metadata for channels, group info for groups. Anything else is a
// no-op (DMs already resolve via the contacts pipeline; status@broadcast
// has no metadata to fetch).
func refreshChat(mgr *Manager, accountID, chatJIDRaw string) {
	mgr.mu.Lock()
	c, ok := mgr.clients[accountID]
	mgr.mu.Unlock()
	if !ok {
		return
	}
	chatJID, err := types.ParseJID(chatJIDRaw)
	if err != nil {
		fmt.Fprintf(os.Stderr,
			"[refresh] bad JID %q: %v\n", chatJIDRaw, err)
		return
	}
	switch chatJID.Server {
	case types.NewsletterServer:
		c.refreshNewsletter(chatJID)
	case types.GroupServer:
		c.refreshGroup(chatJID)
	default:
		// DMs and status — caller should be using avatar fetches /
		// contact resolution instead.
		fmt.Fprintf(os.Stderr,
			"[refresh] %s has no metadata endpoint; ignoring\n",
			chatJID.String(),
		)
	}
}

// refreshNewsletter pulls a single newsletter's metadata via the
// whatsmeow GraphQL endpoint and emits a GroupsUpsert with the
// resolved name. Fired from `onHistorySync` for any newsletter chat
// we don't already have a `display_name` for — `GetSubscribedNewsletters`
// (which `fetchAllNewsletters` already calls) misses channels the user
// only follows but isn't subscribed to push from.
func (c *Client) refreshNewsletter(jid types.JID) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	info, err := c.wa.GetNewsletterInfo(ctx, jid)
	if err != nil {
		fmt.Fprintf(os.Stderr,
			"[newsletter] GetNewsletterInfo(%s): %v\n",
			jid.String(), err,
		)
		return
	}
	fmt.Fprintf(os.Stderr,
		"[newsletter] GetNewsletterInfo(%s): name=%q state=%q\n",
		jid.String(), info.ThreadMeta.Name.Text, info.State.Type,
	)
	emitGroups(c.accountID, []GroupData{newsletterToGroup(info)})
}
