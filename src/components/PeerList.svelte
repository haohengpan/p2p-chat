<script lang="ts">
  import { peers, activeConv } from "../lib/stores";
  import { disconnect } from "../lib/api";

  function selectPeer(peerId: string) {
    activeConv.set(peerId);
  }
</script>

<div class="peer-list">
  <div class="section-title">Peers ({$peers.length})</div>

  {#if $peers.length === 0}
    <div class="empty">No peers connected</div>
  {:else}
    {#each $peers as peer}
      <button
        class="peer-item"
        class:active={$activeConv === peer.peer_id}
        onclick={() => selectPeer(peer.peer_id)}
      >
        <div class="peer-name">{peer.peer_name}</div>
        <div class="peer-id">{peer.peer_id}</div>
      </button>
    {/each}
  {/if}
</div>

<style>
  .peer-list {
    padding: 8px 0;
  }

  .section-title {
    padding: 8px 16px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: #4fc3f7;
    letter-spacing: 0.5px;
  }

  .empty {
    padding: 16px;
    color: #555;
    font-size: 13px;
    text-align: center;
  }

  .peer-item {
    display: block;
    width: 100%;
    padding: 10px 16px;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    color: #b0c4de;
    transition: background 0.15s;
  }

  .peer-item:hover {
    background: #1e3050;
  }

  .peer-item.active {
    background: #1e3a5f;
    border-left: 3px solid #4fc3f7;
  }

  .peer-name {
    font-size: 14px;
    font-weight: 500;
    color: #e0e0e0;
  }

  .peer-id {
    font-size: 11px;
    color: #666;
    margin-top: 2px;
  }
</style>
