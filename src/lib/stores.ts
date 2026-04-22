import { writable, derived } from "svelte/store";
import type { PeerInfo, MessageInfo, Notify } from "./api";

// ---------------------------------------------------------------------------
// Peers
// ---------------------------------------------------------------------------

export interface Peer {
  peer_id: string;
  peer_name: string;
  addr: string;
}

export const peers = writable<Peer[]>([]);

// ---------------------------------------------------------------------------
// Messages — per-conversation
// ---------------------------------------------------------------------------

export interface DisplayMessage {
  msg_id: string;
  from: string;
  content: string;
  timestamp: number;
  direction: "incoming" | "outgoing" | "system";
  failed?: boolean;
}

/** All messages keyed by conv_id */
export const conversations = writable<Record<string, DisplayMessage[]>>({});

/** Currently selected conversation (peer_id) */
export const activeConv = writable<string | null>(null);

/** Messages for the active conversation */
export const activeMessages = derived(
  [conversations, activeConv],
  ([$conversations, $activeConv]) => {
    if (!$activeConv) return [];
    return $conversations[$activeConv] ?? [];
  }
);

// ---------------------------------------------------------------------------
// System notices
// ---------------------------------------------------------------------------

export interface Notice {
  level: "Info" | "Error";
  content: string;
  timestamp: number;
}

export const notices = writable<Notice[]>([]);

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

export const listenAddr = writable<string>("");
export const lanIp = writable<string>("");
export const nodeId = writable<string>("");
export const username = writable<string>("");

// ---------------------------------------------------------------------------
// Message ID counter
// ---------------------------------------------------------------------------

let msgSeq = 0;
export function nextMsgId(): string {
  msgSeq += 1;
  return `msg-${msgSeq}`;
}

// ---------------------------------------------------------------------------
// Handle a Notify event from the backend
// ---------------------------------------------------------------------------

export function handleNotify(event: Notify): void {
  if ("PeerOnline" in event) {
    const { peer_id, peer_name, addr } = event.PeerOnline;
    peers.update((ps) => {
      if (ps.find((p) => p.peer_id === peer_id)) return ps;
      return [...ps, { peer_id, peer_name, addr }];
    });
    addNotice("Info", `${peer_name} (${peer_id}) connected from ${addr}`);
  } else if ("PeerOffline" in event) {
    const { peer_id } = event.PeerOffline;
    peers.update((ps) => ps.filter((p) => p.peer_id !== peer_id));
    activeConv.update((cur) => (cur === peer_id ? null : cur));
    addNotice("Info", `${peer_id} disconnected`);
  } else if ("MessageReceived" in event) {
    const { conv_id, msg } = event.MessageReceived;
    const dm: DisplayMessage = {
      msg_id: msg.msg_id,
      from: msg.from,
      content: typeof msg.content === "object" && "Text" in msg.content ? msg.content.Text : "",
      timestamp: msg.timestamp,
      direction: "incoming",
    };
    pushMessage(conv_id, dm);
  } else if ("MessageAck" in event) {
    const { msg_id, status } = event.MessageAck;
    if (typeof status === "object" && "Failed" in status) {
      addNotice("Error", `Send failed [${msg_id}]: ${status.Failed}`);
    }
  } else if ("PeerList" in event) {
    peers.set(event.PeerList.peers.map((p) => ({
      peer_id: p.peer_id,
      peer_name: p.peer_name,
      addr: p.addr,
    })));
  } else if ("History" in event) {
    const { conv_id, messages } = event.History;
    const dms: DisplayMessage[] = messages.map((msg) => ({
      msg_id: msg.msg_id,
      from: msg.from,
      content: typeof msg.content === "object" && "Text" in msg.content ? msg.content.Text : "",
      timestamp: msg.timestamp,
      direction: "incoming" as const,
    }));
    conversations.update((c) => ({ ...c, [conv_id]: dms }));
  } else if ("Notice" in event) {
    addNotice(event.Notice.level, event.Notice.content);
  }
}

function pushMessage(convId: string, msg: DisplayMessage): void {
  conversations.update((c) => {
    const msgs = c[convId] ?? [];
    return { ...c, [convId]: [...msgs, msg] };
  });
}

export function addOutgoingMessage(convId: string, msgId: string, content: string): void {
  const dm: DisplayMessage = {
    msg_id: msgId,
    from: "me",
    content,
    timestamp: Math.floor(Date.now() / 1000),
    direction: "outgoing",
  };
  pushMessage(convId, dm);
}

function addNotice(level: "Info" | "Error", content: string): void {
  notices.update((ns) => [...ns.slice(-99), { level, content, timestamp: Date.now() }]);
}
