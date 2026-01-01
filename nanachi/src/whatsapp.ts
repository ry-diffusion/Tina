import type { Boom } from "@hapi/boom";
import makeWASocket, {
  DisconnectReason,
  type WASocket,
  type GroupMetadata,
  type WAMessage,
  type GroupParticipant,
} from "baileys";
import pino from "pino";

import { createAuthState } from "./auth";
import * as ipc from "./ipc";
import type {
  ContactData,
  GroupData,
  MessageData,
  ParticipantData,
} from "./types";

const logger = pino({ level: "silent" });

interface AccountConnection {
  socket: WASocket;
  accountId: string;
}

const connections: Map<string, AccountConnection> = new Map();
const accountAuthStates: Map<string, string> = new Map();

export async function startAccount(accountId: string): Promise<void> {
  if (connections.has(accountId)) {
    ipc.sendError(accountId, "Account already connected");
    return;
  }

  const savedAuthState = accountAuthStates.get(accountId);

  const { state, saveCreds, getSerializedState } = await createAuthState(
    accountId,
    savedAuthState
  );

  const sock = makeWASocket({
    logger,
    auth: state,
    syncFullHistory: true,
    printQRInTerminal: false,
  });

  connections.set(accountId, { socket: sock, accountId });

  sock.ev.on("connection.update", (update) => {
    const { connection, lastDisconnect, qr } = update;

    if (qr) {
      ipc.sendQrCode(accountId, qr);
    }

    if (connection === "close") {
      const statusCode = (lastDisconnect?.error as Boom)?.output?.statusCode;
      const shouldReconnect = statusCode !== DisconnectReason.loggedOut;

      connections.delete(accountId);

      if (statusCode === DisconnectReason.loggedOut) {
        accountAuthStates.delete(accountId);
        ipc.sendLoggedOut(accountId);
      } else {
        ipc.sendDisconnected(
          accountId,
          lastDisconnect?.error?.message || "Unknown"
        );
        if (shouldReconnect) {
          setTimeout(() => startAccount(accountId), 3000);
        }
      }
    } else if (connection === "open") {
      const phoneNumber = sock.user?.id?.split(":")[0];
      ipc.sendConnected(accountId, phoneNumber);
      fetchAllGroups(sock, accountId);
    }
  });

  sock.ev.on("creds.update", async () => {
    await saveCreds();
    const serialized = getSerializedState();
    accountAuthStates.set(accountId, serialized);
    ipc.sendAuthStateUpdated(accountId, serialized);
  });

  sock.ev.on("contacts.upsert", (contacts) => {
    const mapped: ContactData[] = contacts.map((c) => ({
      jid: c.id,
      lid: c.lid ?? undefined,
      phone_number: c.phoneNumber ?? undefined,
      name: c.name ?? undefined,
      notify: c.notify ?? undefined,
      verified_name: c.verifiedName ?? undefined,
      img_url: c.imgUrl ?? undefined,
      status: c.status ?? undefined,
    }));
    ipc.sendContactsUpsert(accountId, mapped);
  });

  sock.ev.on("contacts.update", (contacts) => {
    const mapped: ContactData[] = contacts.map((c) => ({
      jid: c.id!,
      lid: c.lid ?? undefined,
      name: c.name ?? undefined,
      notify: c.notify ?? undefined,
      verified_name: c.verifiedName ?? undefined,
      img_url: c.imgUrl ?? undefined,
      status: c.status ?? undefined,
    }));
    ipc.sendContactsUpdate(accountId, mapped);
  });

  sock.ev.on("groups.upsert", (groups) => {
    processGroupsUpsert(accountId, groups);
  });

  sock.ev.on("groups.update", (groups) => {
    processGroupsUpdate(accountId, groups);
  });

  sock.ev.on("messages.upsert", (m) => {
    const messages: MessageData[] = [];

    for (const msg of m.messages) {
      if (!msg.key.id || !msg.key.remoteJid) continue;

      const content = extractMessageContent(msg);
      const messageType = extractMessageType(msg);
      const timestamp =
        typeof msg.messageTimestamp === "number"
          ? msg.messageTimestamp
          : Number(msg.messageTimestamp) || Math.floor(Date.now() / 1000);

      messages.push({
        message_id: msg.key.id,
        chat_jid: msg.key.remoteJid,
        sender_jid: msg.key.participant || msg.key.remoteJid,
        content,
        message_type: messageType,
        timestamp,
        is_from_me: msg.key.fromMe || false,
        raw_json: JSON.stringify(msg),
      });
    }

    if (messages.length > 0) {
      ipc.sendMessagesUpsert(accountId, messages);
    }
  });

  sock.ev.on("messaging-history.set", ({ messages }) => {
    const mapped: MessageData[] = [];

    for (const msg of messages) {
      if (!msg.key.id || !msg.key.remoteJid) continue;

      const content = extractMessageContent(msg);
      const messageType = extractMessageType(msg);
      const timestamp =
        typeof msg.messageTimestamp === "number"
          ? msg.messageTimestamp
          : Number(msg.messageTimestamp) || Math.floor(Date.now() / 1000);

      mapped.push({
        message_id: msg.key.id,
        chat_jid: msg.key.remoteJid,
        sender_jid: msg.key.participant || msg.key.remoteJid,
        content,
        message_type: messageType,
        timestamp,
        is_from_me: msg.key.fromMe || false,
        raw_json: JSON.stringify(msg),
      });
    }

    if (mapped.length > 0) {
      ipc.sendMessagesUpsert(accountId, mapped);
    }
    ipc.sendHistorySyncComplete(accountId, mapped.length);
  });

  ipc.sendReady(accountId);
}

export function stopAccount(accountId: string): void {
  const conn = connections.get(accountId);
  if (conn) {
    conn.socket.end(undefined);
    connections.delete(accountId);
    ipc.sendDisconnected(accountId, "Stopped by user");
  }
}

export function setAuthState(accountId: string, authState: string): void {
  accountAuthStates.set(accountId, authState);
}

export async function sendMessage(
  accountId: string,
  to: string,
  content: string
): Promise<boolean> {
  const conn = connections.get(accountId);
  if (!conn) {
    ipc.sendError(accountId, "Account not connected");
    return false;
  }

  try {
    await conn.socket.sendMessage(to, { text: content });
    return true;
  } catch (err) {
    ipc.sendError(accountId, `Failed to send message: ${err}`);
    return false;
  }
}

export function shutdown(): void {
  for (const [accountId, conn] of connections) {
    conn.socket.end(undefined);
    ipc.sendDisconnected(accountId, "Shutdown");
  }
  connections.clear();
  process.exit(0);
}

function mapGroup(g: GroupMetadata): GroupData {
  const participants: ParticipantData[] = g.participants.map((p) => ({
    id: p.id,

    admin: p.admin ?? undefined,
    phone_number: p.phoneNumber ?? undefined,
  }));

  return {
    jid: g.id,
    subject: g.subject ?? undefined,
    owner: g.owner ?? undefined,
    description: g.desc ?? undefined,
    participants,
  };
}

async function fetchAllGroups(
  sock: WASocket,
  accountId: string
): Promise<void> {
  try {
    const groups = await sock.groupFetchAllParticipating();
    console.warn(groups);
    const groupsArray = Object.values(groups);
    processGroupsUpsert(accountId, groupsArray);
  } catch (err) {
    ipc.sendError(accountId, `Failed to fetch groups: ${err}`);
  }
}

function extractMessageContent(msg: WAMessage): string | undefined {
  const message = msg.message;
  if (!message) return undefined;

  if (message.conversation) return message.conversation as string;
  if (message.extendedTextMessage) {
    return message.extendedTextMessage.text ?? undefined;
  }
  if (message.imageMessage) return "[Image]";
  if (message.videoMessage) return "[Video]";
  if (message.audioMessage) return "[Audio]";
  if (message.documentMessage) return "[Document]";
  if (message.stickerMessage) return "[Sticker]";
  if (message.contactMessage) return "[Contact]";
  if (message.locationMessage) return "[Location]";

  return undefined;
}

function extractMessageType(msg: WAMessage): string {
  const message = msg.message;
  if (!message) return "unknown";

  if (message.conversation || message.extendedTextMessage) return "text";
  if (message.imageMessage) return "image";
  if (message.videoMessage) return "video";
  if (message.audioMessage) return "audio";
  if (message.documentMessage) return "document";
  if (message.stickerMessage) return "sticker";
  if (message.contactMessage) return "contact";
  if (message.locationMessage) return "location";

  return "unknown";
}

function extractContactsFromParticipants(
  participants: GroupParticipant[]
): ContactData[] {
  return participants.map((p) => ({
    jid: p.phoneNumber,
    lid: p.lid ?? undefined,
    phone_number: p.phoneNumber ?? undefined,
    name: p.name ?? undefined,
    notify: p.notify ?? undefined,
    verified_name: p.verifiedName ?? undefined,
    img_url: p.imgUrl ?? undefined,
    status: p.status ?? undefined,
  }));
}

function processGroupsUpsert(accountId: string, groups: GroupMetadata[]): void {
  const mapped = groups.map(mapGroup);
  if (mapped.length > 0) {
    ipc.sendGroupsUpsert(accountId, mapped);
  }

  const contacts = groups.flatMap((g) =>
    extractContactsFromParticipants(g.participants)
  );
  if (contacts.length > 0) {
    ipc.sendContactsUpsert(accountId, contacts);
  }
}

function processGroupsUpdate(accountId: string, groups: GroupMetadata[]): void {
  const mapped: GroupData[] = groups.map((g) => ({
    jid: g.id!,
    subject: g.subject ?? undefined,
    owner: g.owner ?? undefined,
    description: g.desc ?? undefined,
    participants: [],
  }));
  if (mapped.length > 0) {
    ipc.sendGroupsUpdate(accountId, mapped);
  }

  const contacts = groups.flatMap((g) =>
    extractContactsFromParticipants(g.participants)
  );
  if (contacts.length > 0) {
    ipc.sendContactsUpsert(accountId, contacts);
  }
}
