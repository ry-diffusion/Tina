package main

import "go.mau.fi/whatsmeow/types"

// splitJIDLID separa um par (primário, alternativo) em (phone-jid, lid).
// Retorna ponteiros nil quando não há valor para aquele lado.
//
// Whatsmeow expõe identidades em duas formas: o "JID" tradicional baseado em
// número de telefone (server `s.whatsapp.net`) e o "LID" privacy-aware
// (server `lid`). Em vários eventos ambas vêm — `MessageSource.Sender` +
// `SenderAlt`, `events.PushName.JID` + `JIDAlt`, `GroupParticipant.JID` +
// `LID/PhoneNumber`. Persistimos as duas porque a Meta usa tanto uma como
// a outra dependendo do modo de privacidade do grupo/chat.
func splitJIDLID(primary, alt types.JID) (jid *string, lid *string) {
	for _, j := range [2]types.JID{primary, alt} {
		if j.IsEmpty() {
			continue
		}
		s := j.String()
		switch j.Server {
		case types.HiddenUserServer:
			if lid == nil {
				lid = &s
			}
		case types.DefaultUserServer, types.LegacyUserServer, types.HostedServer:
			if jid == nil {
				jid = &s
			}
		}
	}
	return
}

// phoneOf devolve o User de um JID PN, ou nil se for LID/vazio.
func phoneOf(j types.JID) *string {
	if j.IsEmpty() {
		return nil
	}
	if j.Server == types.DefaultUserServer || j.Server == types.LegacyUserServer || j.Server == types.HostedServer {
		u := j.User
		return &u
	}
	return nil
}
