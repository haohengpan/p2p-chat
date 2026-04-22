<script lang="ts">
  import { tick } from "svelte";
  import { activeMessages, activeConv, peers } from "../lib/stores";

  let container: HTMLDivElement;
  let autoScroll = $state(true);

  // Auto-scroll when new messages arrive
  $effect(() => {
    // Track message count to trigger scroll
    const _len = $activeMessages.length;
    if (autoScroll && container) {
      tick().then(() => {
        container.scrollTop = container.scrollHeight;
      });
    }
  });

  function handleScroll() {
    if (!container) return;
    const atBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
    autoScroll = atBottom;
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  // Find peer display name
  function peerName(convId: string): string {
    const p = $peers.find((p) => p.peer_id === convId);
    return p ? p.peer_name : convId;
  }
</script>

<div class="message-header">
  Chat with <strong>{peerName($activeConv ?? "")}</strong>
</div>

<div class="message-list" bind:this={container} onscroll={handleScroll}>
  {#if $activeMessages.length === 0}
    <div class="empty">No messages yet. Say hello!</div>
  {:else}
    {#each $activeMessages as msg}
      <div class="message" class:outgoing={msg.direction === "outgoing"} class:system={msg.direction === "system"}>
        <div class="msg-meta">
          <span class="msg-from">{msg.direction === "outgoing" ? "You" : msg.from}</span>
          <span class="msg-time">{formatTime(msg.timestamp)}</span>
        </div>
        <div class="msg-content">{msg.content}</div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .message-header {
    padding: 10px 20px;
    background: #162032;
    border-bottom: 1px solid #2a3a4a;
    font-size: 14px;
    color: #b0c4de;
    flex-shrink: 0;
  }

  .message-header strong {
    color: #4fc3f7;
  }

  .message-list {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }

  .empty {
    text-align: center;
    color: #555;
    margin-top: 40px;
  }

  .message {
    margin-bottom: 12px;
    max-width: 75%;
  }

  .message.outgoing {
    margin-left: auto;
    text-align: right;
  }

  .msg-meta {
    font-size: 11px;
    color: #666;
    margin-bottom: 3px;
  }

  .msg-from {
    font-weight: 500;
    color: #4fc3f7;
  }

  .message.outgoing .msg-from {
    color: #66bb6a;
  }

  .msg-time {
    margin-left: 8px;
  }

  .msg-content {
    display: inline-block;
    padding: 8px 14px;
    border-radius: 12px;
    background: #1e3050;
    color: #e0e0e0;
    font-size: 14px;
    line-height: 1.4;
    word-break: break-word;
    text-align: left;
  }

  .message.outgoing .msg-content {
    background: #1b5e20;
    color: #e8f5e9;
  }

  .message.system .msg-content {
    background: transparent;
    color: #888;
    font-style: italic;
    padding: 4px 0;
  }
</style>
