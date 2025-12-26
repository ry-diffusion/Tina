import {
    type AuthenticationCreds,
    type AuthenticationState,
    BufferJSON,
    initAuthCreds,
    type SignalDataTypeMap,
} from "baileys";
import * as ipc from "./ipc";

interface AuthStateResult {
    state: AuthenticationState;
    saveCreds: () => Promise<void>;
    getSerializedState: () => string;
}

export async function createAuthState(
    accountId: string,
    initialState?: string
): Promise<AuthStateResult> {
    let creds: AuthenticationCreds;
    let keys: Record<string, unknown> = {};

    if (initialState) {
        try {
            const parsed = JSON.parse(initialState, BufferJSON.reviver);
            creds = parsed.creds;
            keys = parsed.keys || {};
        } catch {
            creds = initAuthCreds();
            keys = {};
        }
    } else {
        creds = initAuthCreds();
        keys = {};
    }

    const getSerializedState = (): string => {
        return JSON.stringify({ creds, keys }, BufferJSON.replacer);
    };

    const saveCreds = async (): Promise<void> => {
    };

    return {
        state: {
            creds,
            keys: {
                get: async <T extends keyof SignalDataTypeMap>(
                    type: T,
                    ids: string[]
                ): Promise<{ [id: string]: SignalDataTypeMap[T] }> => {
                    const data: { [id: string]: SignalDataTypeMap[T] } = {};
                    for (const id of ids) {
                        const value = keys[`${type}-${id}`];
                        if (value) {
                            data[id] = value as SignalDataTypeMap[T];
                        }
                    }
                    return data;
                },
                set: async (data: Record<string, Record<string, unknown>>): Promise<void> => {
                    for (const category in data) {
                        for (const id in data[category]) {
                            const key = `${category}-${id}`;
                            keys[key] = data[category]![id];
                        }
                    }
                },
            },
        },
        saveCreds,
        getSerializedState,
    };
}
