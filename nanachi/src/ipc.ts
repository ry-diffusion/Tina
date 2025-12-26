import type { ContactData, GroupData, MessageData } from "./types";

export function sendEvent(type: string, payload: unknown): void {
    const message = JSON.stringify({ id: generateId(), type, payload });
    process.stdout.write(message + "\n");
}

export function sendReady(accountId: string): void {
    sendEvent("Ready", { account_id: accountId });
}

export function sendQrCode(accountId: string, qr: string): void {
    sendEvent("QrCode", { account_id: accountId, qr });
}

export function sendConnected(accountId: string, phoneNumber?: string): void {
    sendEvent("Connected", { account_id: accountId, phone_number: phoneNumber });
}

export function sendDisconnected(accountId: string, reason: string): void {
    sendEvent("Disconnected", { account_id: accountId, reason });
}

export function sendLoggedOut(accountId: string): void {
    sendEvent("LoggedOut", { account_id: accountId });
}

export function sendAuthStateUpdated(accountId: string, authState: string): void {
    sendEvent("AuthStateUpdated", { account_id: accountId, auth_state: authState });
}

export function sendContactsUpsert(accountId: string, contacts: ContactData[]): void {
    sendEvent("ContactsUpsert", { account_id: accountId, contacts });
}

export function sendContactsUpdate(accountId: string, contacts: ContactData[]): void {
    sendEvent("ContactsUpdate", { account_id: accountId, contacts });
}

export function sendGroupsUpsert(accountId: string, groups: GroupData[]): void {
    sendEvent("GroupsUpsert", { account_id: accountId, groups });
}

export function sendGroupsUpdate(accountId: string, groups: GroupData[]): void {
    sendEvent("GroupsUpdate", { account_id: accountId, groups });
}

export function sendMessagesUpsert(accountId: string, messages: MessageData[]): void {
    sendEvent("MessagesUpsert", { account_id: accountId, messages });
}

export function sendHistorySyncComplete(accountId: string, messagesCount: number): void {
    sendEvent("HistorySyncComplete", { account_id: accountId, messages_count: messagesCount });
}

export function sendError(accountId: string | null, error: string): void {
    sendEvent("Error", { account_id: accountId, error });
}

export function sendCommandResult(commandId: string, success: boolean, data?: unknown, error?: string): void {
    sendEvent("CommandResult", { command_id: commandId, success, data, error });
}

function generateId(): string {
    return Date.now().toString(16) + Math.random().toString(16).slice(2);
}
