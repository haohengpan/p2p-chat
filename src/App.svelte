<script lang="ts">
  import { onMount } from "svelte";
  import { onNotify, getSavedPeers, loadSavedHistory } from "./lib/api";
  import { handleNotify, peers, conversations, nodeId, type DisplayMessage } from "./lib/stores";
  import Setup from "./components/Setup.svelte";
  import ChatWindow from "./components/ChatWindow.svelte";
  import { get } from "svelte/store";

  let screen: "setup" | "chat" = $state("setup");

  async function onSetupDone() {
    screen = "chat";
    // Load saved peers (all start as offline; backend will emit PeerOnline for any that reconnect)
    try {
      const savedPeers = await getSavedPeers();
      if (savedPeers.length > 0) {
        peers.set(savedPeers.map((p) => ({
          peer_id: p.peer_id,
          peer_name: p.peer_name,
          addr: p.addr,
          online: false,
        })));
        // Load history for each saved peer
        for (const p of savedPeers) {
          const msgs = await loadSavedHistory(p.peer_id);
          if (msgs.length > 0) {
            const myId = get(nodeId);
            const dms: DisplayMessage[] = msgs.map((msg) => ({
              msg_id: msg.msg_id,
              from: msg.from,
              content: typeof msg.content === "object" && "Text" in msg.content ? msg.content.Text : "",
              timestamp: msg.timestamp,
              direction: msg.from === myId ? "outgoing" as const : "incoming" as const,
            }));
            conversations.update((c) => ({ ...c, [p.peer_id]: dms }));
          }
        }
      }
    } catch (_) {
      // No saved data
    }
  }

  onMount(() => {
    const unlistenPromise = onNotify((event) => {
      handleNotify(event);
    });
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  });
</script>

{#if screen === "setup"}
  <Setup onDone={onSetupDone} />
{:else}
  <ChatWindow />
{/if}

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", sans-serif;
    background: #1a1a2e;
    color: #e0e0e0;
    height: 100vh;
    overflow: hidden;
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(#app) {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
</style>
