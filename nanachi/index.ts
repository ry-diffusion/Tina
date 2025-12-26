import * as readline from "node:readline";
import type { IpcMessage } from "./src/types";
import * as whatsapp from "./src/whatsapp";
import * as ipc from "./src/ipc";

const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    terminal: false,
});

rl.on("line", async (line) => {
    if (!line.trim()) return;

    try {
        const msg: IpcMessage = JSON.parse(line);
        await handleCommand(msg);
    } catch (err) {
        ipc.sendError(null, `Failed to parse command: ${err}`);
    }
});

async function handleCommand(msg: IpcMessage): Promise<void> {
    const { id, type, payload } = msg;

    try {
        switch (type) {
            case "StartAccount": {
                const p = payload as { account_id: string };
                await whatsapp.startAccount(p.account_id);
                ipc.sendCommandResult(id, true);
                break;
            }
            case "StopAccount": {
                const p = payload as { account_id: string };
                whatsapp.stopAccount(p.account_id);
                ipc.sendCommandResult(id, true);
                break;
            }
            case "SetAuthState": {
                const p = payload as { account_id: string; auth_state: string };
                whatsapp.setAuthState(p.account_id, p.auth_state);
                ipc.sendCommandResult(id, true);
                break;
            }
            case "SendMessage": {
                const p = payload as { account_id: string; to: string; content: string };
                const success = await whatsapp.sendMessage(p.account_id, p.to, p.content);
                ipc.sendCommandResult(id, success);
                break;
            }
            case "Shutdown": {
                ipc.sendCommandResult(id, true);
                whatsapp.shutdown();
                break;
            }
            default:
                ipc.sendCommandResult(id, false, undefined, `Unknown command: ${type}`);
        }
    } catch (err) {
        ipc.sendCommandResult(id, false, undefined, `${err}`);
    }
}

process.on("SIGTERM", () => {
    whatsapp.shutdown();
});

process.on("SIGINT", () => {
    whatsapp.shutdown();
});

ipc.sendEvent("Ready", { account_id: null });
