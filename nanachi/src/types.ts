export interface IpcCommand {
    type: string;
    payload: unknown;
}

export interface StartAccountPayload {
    account_id: string;
}

export interface StopAccountPayload {
    account_id: string;
}

export interface SetAuthStatePayload {
    account_id: string;
    auth_state: string;
}

export interface SendMessagePayload {
    account_id: string;
    to: string;
    content: string;
}

export interface GetMessagesPayload {
    account_id: string;
    chat_jid?: string;
    limit: number;
}

export type CommandPayload = 
    | { type: "StartAccount"; payload: StartAccountPayload }
    | { type: "StopAccount"; payload: StopAccountPayload }
    | { type: "SetAuthState"; payload: SetAuthStatePayload }
    | { type: "SendMessage"; payload: SendMessagePayload }
    | { type: "GetContacts"; payload: { account_id: string } }
    | { type: "GetGroups"; payload: { account_id: string } }
    | { type: "GetMessages"; payload: GetMessagesPayload }
    | { type: "Shutdown"; payload: null };

export interface IpcMessage {
    id: string;
    type: string;
    payload: unknown;
}

export interface ContactData {
    jid: string;
    lid?: string;
    phone_number?: string;
    name?: string;
    notify?: string;
    verified_name?: string;
    img_url?: string;
    status?: string;
}

export interface GroupData {
    jid: string;
    subject?: string;
    owner?: string;
    description?: string;
    participants: ParticipantData[];
}

export interface ParticipantData {
    id: string;
    admin?: string;
    phone_number?: string;
}

export interface MessageData {
    message_id: string;
    chat_jid: string;
    sender_jid: string;
    content?: string;
    message_type: string;
    timestamp: number;
    is_from_me: boolean;
    raw_json?: string;
}
