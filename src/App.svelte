<script lang="ts">
  import { onMount } from "svelte";
  import { onNotify } from "./lib/api";
  import { handleNotify, nodeId, username, listenAddr, lanIp } from "./lib/stores";
  import Setup from "./components/Setup.svelte";
  import ChatWindow from "./components/ChatWindow.svelte";

  let screen: "setup" | "chat" = $state("setup");

  function onSetupDone() {
    screen = "chat";
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
