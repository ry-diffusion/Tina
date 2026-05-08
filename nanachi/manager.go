package main

import (
	"context"
	"errors"
	"fmt"
	"sync"

	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	waLog "go.mau.fi/whatsmeow/util/log"
)

// Manager mantém o estado por conta e a tabela account_id → device JID
// dentro do mesmo SQLite usado pelo sqlstore do whatsmeow.
type Manager struct {
	container *sqlstore.Container
	logger    waLog.Logger

	mu      sync.Mutex
	clients map[string]*Client
}

func newManager(container *sqlstore.Container, logger waLog.Logger) *Manager {
	m := &Manager{
		container: container,
		logger:    logger,
		clients:   make(map[string]*Client),
	}
	if err := m.ensureMappingTable(); err != nil {
		emitError(nil, fmt.Sprintf("failed to init account mapping: %v", err))
	}
	return m
}

// resolveDevice escolhe (ou cria) o *store.Device associado ao
// account_id. Se já há um JID salvo, recupera o device existente; senão,
// cria novo (que disparará pareamento via QR no whatsmeow).
func (m *Manager) resolveDevice(ctx context.Context, accountID string) (*store.Device, error) {
	jidStr, err := m.lookupDeviceJID(accountID)
	if err != nil {
		return nil, fmt.Errorf("lookup device: %w", err)
	}
	if jidStr != "" {
		jid, err := types.ParseJID(jidStr)
		if err == nil {
			dev, err := m.container.GetDevice(ctx, jid)
			if err == nil && dev != nil {
				return dev, nil
			}
		}
		// JID salvo mas device sumiu — limpa e segue para novo device.
		_ = m.clearDeviceJID(accountID)
	}
	return m.container.NewDevice(), nil
}

func (m *Manager) startAccount(accountID string) error {
	m.mu.Lock()
	if _, ok := m.clients[accountID]; ok {
		m.mu.Unlock()
		return errors.New("account already started")
	}
	m.mu.Unlock()

	ctx := context.Background()
	device, err := m.resolveDevice(ctx, accountID)
	if err != nil {
		return err
	}

	client := newClient(m, accountID, device)
	m.mu.Lock()
	m.clients[accountID] = client
	m.mu.Unlock()

	if err := client.connect(ctx); err != nil {
		m.mu.Lock()
		delete(m.clients, accountID)
		m.mu.Unlock()
		return err
	}
	return nil
}

func (m *Manager) stopAccount(accountID, reason string) {
	m.mu.Lock()
	client, ok := m.clients[accountID]
	if ok {
		delete(m.clients, accountID)
	}
	m.mu.Unlock()
	if !ok {
		return
	}
	client.disconnect(reason)
}

func (m *Manager) logoutAccount(accountID string) error {
	m.mu.Lock()
	client := m.clients[accountID]
	m.mu.Unlock()
	if client != nil {
		ctx := context.Background()
		if err := client.wa.Logout(ctx); err != nil {
			return err
		}
		m.mu.Lock()
		delete(m.clients, accountID)
		m.mu.Unlock()
	}
	_ = m.clearDeviceJID(accountID)
	emitLoggedOut(accountID)
	return nil
}

// reconcileAccount força um re-emit de tudo que o whatsmeow já sabe
// sobre contatos, grupos e newsletters. Útil para reconstruir o
// tina.db a partir do whatsmeow.db sem precisar de re-pareamento.
func (m *Manager) reconcileAccount(accountID string) error {
	m.mu.Lock()
	client := m.clients[accountID]
	m.mu.Unlock()
	if client == nil {
		return errors.New("account not connected")
	}
	go client.reconcile()
	return nil
}

func (m *Manager) sendMessage(accountID, to, content, localID string, mentioned []string) (bool, error) {
	m.mu.Lock()
	client := m.clients[accountID]
	m.mu.Unlock()
	if client == nil {
		return false, errors.New("account not connected")
	}
	return client.send(to, content, localID, mentioned)
}

func (m *Manager) sendMedia(p SendMediaPayload) (string, error) {
	m.mu.Lock()
	client := m.clients[p.AccountID]
	m.mu.Unlock()
	if client == nil {
		return "", errors.New("account not connected")
	}
	return client.sendMedia(p)
}

func (m *Manager) markRead(p MarkReadPayload) error {
	m.mu.Lock()
	client := m.clients[p.AccountID]
	m.mu.Unlock()
	if client == nil {
		return errors.New("account not connected")
	}
	return client.markRead(p)
}

func (m *Manager) shutdown() {
	m.mu.Lock()
	clients := make([]*Client, 0, len(m.clients))
	for _, c := range m.clients {
		clients = append(clients, c)
	}
	m.clients = make(map[string]*Client)
	m.mu.Unlock()
	for _, c := range clients {
		c.disconnect("Shutdown")
	}
}
