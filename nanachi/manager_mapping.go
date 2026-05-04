package main

import (
	"database/sql"
	"errors"
	"fmt"
)

// O sqlstore.Container expõe o *sql.DB internamente; replicamos a tabela
// aqui num conn separado para não interferir com o pool dele.
func (m *Manager) db() (*sql.DB, error) {
	dir, err := dataDir()
	if err != nil {
		return nil, err
	}
	dsn := fmt.Sprintf("file:%s/whatsmeow.db?_foreign_keys=on", dir)
	return sql.Open("sqlite3", dsn)
}

func (m *Manager) ensureMappingTable() error {
	db, err := m.db()
	if err != nil {
		return err
	}
	defer db.Close()
	_, err = db.Exec(`CREATE TABLE IF NOT EXISTS tina_accounts (
		account_id TEXT PRIMARY KEY,
		device_jid TEXT
	)`)
	return err
}

func (m *Manager) lookupDeviceJID(accountID string) (string, error) {
	db, err := m.db()
	if err != nil {
		return "", err
	}
	defer db.Close()
	var jid sql.NullString
	row := db.QueryRow(`SELECT device_jid FROM tina_accounts WHERE account_id = ?`, accountID)
	if err := row.Scan(&jid); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", nil
		}
		return "", err
	}
	if !jid.Valid {
		return "", nil
	}
	return jid.String, nil
}

func (m *Manager) saveDeviceJID(accountID, jid string) error {
	db, err := m.db()
	if err != nil {
		return err
	}
	defer db.Close()
	_, err = db.Exec(`INSERT INTO tina_accounts (account_id, device_jid) VALUES (?, ?)
		ON CONFLICT(account_id) DO UPDATE SET device_jid = excluded.device_jid`,
		accountID, jid)
	return err
}

func (m *Manager) clearDeviceJID(accountID string) error {
	db, err := m.db()
	if err != nil {
		return err
	}
	defer db.Close()
	_, err = db.Exec(`DELETE FROM tina_accounts WHERE account_id = ?`, accountID)
	return err
}
