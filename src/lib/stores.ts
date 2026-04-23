import { writable, derived } from "svelte/store";
import type { PeerInfo, MessageInfo, Notify } from "./api";

// ---------------------------------------------------------------------------
// Peers
// ---------------------------------------------------------------------------

export interface Peer {
  peer_id: string;
  peer_name: string;
  addr: string;
  online: boolean;
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
      const existing = ps.findIndex((p) => p.peer_id === peer_id);
      if (existing >= 0) {
        // Update existing peer to online
        const updated = [...ps];
        updated[existing] = { ...updated[existing], peer_name, addr, online: true };
        return updated;
      }
      return [...ps, { peer_id, peer_name, addr, online: true }];
    });
    addNotice("Info", `${peer_name} (${peer_id}) connected from ${addr}`);
  } else if ("PeerOffline" in event) {
    const { peer_id } = event.PeerOffline;
    // Mark as offline instead of removing
    peers.update((ps) =>
      ps.map((p) => p.peer_id === peer_id ? { ...p, online: false } : p)
    );
    addNotice("Info", `${peer_id} disconnected`);
  } else if ("MessageReceived" in event) {
    const { conv_id, msg } = event.MessageReceived;
    const text = typeof msg.content === "object" && "Text" in msg.content ? msg.content.Text : "";
    const dm: DisplayMessage = {
      msg_id: msg.msg_id,
      from: msg.from,
      content: text,
      timestamp: msg.timestamp,
      direction: "incoming",
    };
    pushMessage(conv_id, dm);
    notifyIncoming(conv_id, msg.from, text);
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
      online: p.online,
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

/** Unread message counts per conversation */
export const unreadCounts = writable<Record<string, number>>({});

/** Mark a conversation as read (clear unread count). */
export function markRead(convId: string): void {
  unreadCounts.update((c) => {
    const { [convId]: _, ...rest } = c;
    return rest;
  });
}

/** Show a desktop notification for an incoming message. */
function notifyIncoming(convId: string, from: string, text: string): void {
  // Find peer display name
  let peerName = from;
  const unsub = peers.subscribe((ps) => {
    const p = ps.find((p) => p.peer_id === from);
    if (p) peerName = p.peer_name;
  });
  unsub();

  // Increment unread count if not viewing this conversation
  let currentConv: string | null = null;
  const unsub2 = activeConv.subscribe((v) => { currentConv = v; });
  unsub2();

  if (currentConv !== convId || document.hidden) {
    unreadCounts.update((c) => ({ ...c, [convId]: (c[convId] ?? 0) + 1 }));
  }

  // Desktop notification when window is not focused or viewing a different conversation
  if (document.hidden || currentConv !== convId) {
    if (Notification.permission === "granted") {
      new Notification(peerName, {
        body: text.length > 100 ? text.slice(0, 100) + "..." : text,
        tag: `msg-${convId}`,
      });
    } else if (Notification.permission !== "denied") {
      Notification.requestPermission();
    }
  }
}

function addNotice(level: "Info" | "Error", content: string): void {
  notices.update((ns) => [...ns.slice(-99), { level, content, timestamp: Date.now() }]);
}
