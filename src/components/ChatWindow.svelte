<script lang="ts">
  import { peers, activeConv, notices, listenAddr, lanIp, nodeId } from "../lib/stores";
  import PeerList from "./PeerList.svelte";
  import MessageList from "./MessageList.svelte";
  import InputBar from "./InputBar.svelte";
  import ConnectDialog from "./ConnectDialog.svelte";

  let showConnect = $state(false);
</script>

<div class="chat-layout">
  <!-- Header -->
  <div class="header">
    <div class="header-left">
      <h2>P2P Chat</h2>
      <span class="header-info">
        {$nodeId} · {$listenAddr}
      </span>
    </div>
    <button class="connect-btn" onclick={() => showConnect = true}>
      + Connect
    </button>
  </div>

  <div class="main">
    <!-- Sidebar: peer list -->
    <div class="sidebar">
      <PeerList />
    </div>

    <!-- Chat area -->
    <div class="chat-area">
      {#if $activeConv}
        <MessageList />
        <InputBar />
      {:else}
        <div class="no-chat">
          <div class="no-chat-content">
            <h3>Welcome to P2P Chat</h3>
            <p>Connect to a peer and select them from the list to start chatting.</p>
            <p class="info-line">LAN: {$lanIp}:{$listenAddr.split(':')[1] ?? ''}</p>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Notices bar -->
  {#if $notices.length > 0}
    {@const lastNotice = $notices[$notices.length - 1]}
    <div class="notice-bar" class:error={lastNotice.level === "Error"}>
      {lastNotice.content}
    </div>
  {/if}

  <!-- Connect dialog -->
  {#if showConnect}
    <ConnectDialog onClose={() => showConnect = false} />
  {/if}
</div>

<style>
  .chat-layout {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 20px;
    background: #16213e;
    border-bottom: 1px solid #2a3a4a;
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  .header h2 {
    margin: 0;
    color: #4fc3f7;
    font-size: 18px;
  }

  .header-info {
    color: #666;
    font-size: 12px;
  }

  .connect-btn {
    padding: 6px 16px;
    background: #4fc3f7;
    color: #0d1b2a;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .connect-btn:hover {
    background: #29b6f6;
  }

  .main {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .sidebar {
    width: 220px;
    flex-shrink: 0;
    background: #162032;
    border-right: 1px solid #2a3a4a;
    overflow-y: auto;
  }

  .chat-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .no-chat {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .no-chat-content {
    text-align: center;
    color: #666;
  }

  .no-chat-content h3 {
    color: #4fc3f7;
    margin-bottom: 8px;
  }

  .info-line {
    margin-top: 16px;
    font-family: monospace;
    color: #4fc3f7;
    font-size: 13px;
  }

  .notice-bar {
    padding: 6px 20px;
    background: #1e3a5f;
    color: #b0c4de;
    font-size: 13px;
    border-top: 1px solid #2a3a4a;
    flex-shrink: 0;
  }

  .notice-bar.error {
    background: #3a1e1e;
    color: #ef5350;
  }
</style>
