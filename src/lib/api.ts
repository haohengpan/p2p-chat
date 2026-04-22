import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Types matching src-tauri/src/api.rs
// ---------------------------------------------------------------------------

export interface MessageInfo {
  msg_id: string;
  from: string;
  content: { Text: string };
  timestamp: number;
  status: "Sent" | { Failed: string };
}

export interface PeerInfo {
  peer_id: string;
  peer_name: string;
  addr: string;
}

export type Notify =
  | { PeerOnline: { peer_id: string; peer_name: string; addr: string } }
  | { PeerOffline: { peer_id: string } }
  | { MessageReceived: { conv_id: string; msg: MessageInfo } }
  | { MessageAck: { msg_id: string; status: "Sent" | { Failed: string } } }
  | { PeerList: { peers: PeerInfo[] } }
  | { History: { conv_id: string; messages: MessageInfo[] } }
  | { Notice: { level: "Info" | "Error"; content: string } };

export interface SetupResult {
  listen_addr: string;
  lan_ip: string;
}

// ---------------------------------------------------------------------------
// Tauri invoke wrappers
// ---------------------------------------------------------------------------

export async function setup(
  nodeId: string,
  username: string,
  port: number
): Promise<SetupResult> {
  return invoke("setup", { nodeId, username, port });
}

export async function connect(addr: string): Promise<void> {
  return invoke("connect", { addr });
}

export async function disconnect(peerId: string): Promise<void> {
  return invoke("disconnect", { peerId });
}

export async function sendMessage(
  convId: string,
  msgId: string,
  content: string
): Promise<void> {
  return invoke("send_message", { convId, msgId, content });
}

export async function getHistory(
  convId: string,
  before: number | null,
  limit: number
): Promise<void> {
  return invoke("get_history", { convId, before, limit });
}

export async function listPeers(): Promise<void> {
  return invoke("list_peers");
}

export async function shutdown(): Promise<void> {
  return invoke("shutdown");
}

// ---------------------------------------------------------------------------
// Event listener
// ---------------------------------------------------------------------------

export function onNotify(callback: (event: Notify) => void): Promise<UnlistenFn> {
  return listen<Notify>("notify", (e) => callback(e.payload));
}
