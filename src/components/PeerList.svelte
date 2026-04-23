<script lang="ts">
  import { peers, activeConv } from "../lib/stores";
  import { connect } from "../lib/api";

  function selectPeer(peerId: string) {
    activeConv.set(peerId);
  }

  async function reconnect(peer: { peer_id: string; addr: string }) {
    try {
      await connect(peer.addr);
    } catch (_) {
      // error will be shown via Notice
    }
  }

  const onlinePeers = $derived($peers.filter((p) => p.online));
  const offlinePeers = $derived($peers.filter((p) => !p.online));
</script>

<div class="peer-list">
  <div class="section-title">Online ({onlinePeers.length})</div>

  {#if onlinePeers.length === 0}
    <div class="empty">No peers online</div>
  {/if}

  {#each onlinePeers as peer}
    <button
      class="peer-item"
      class:active={$activeConv === peer.peer_id}
      onclick={() => selectPeer(peer.peer_id)}
    >
      <div class="peer-name">
        <span class="status-dot online"></span>
        {peer.peer_name}
      </div>
      <div class="peer-id">{peer.peer_id}</div>
    </button>
  {/each}

  {#if offlinePeers.length > 0}
    <div class="section-title">Offline ({offlinePeers.length})</div>

    {#each offlinePeers as peer}
      <button
        class="peer-item"
        class:active={$activeConv === peer.peer_id}
        onclick={() => selectPeer(peer.peer_id)}
      >
        <div class="peer-name">
          <span class="status-dot offline"></span>
          {peer.peer_name}
        </div>
        <div class="peer-meta">
          <span class="peer-id">{peer.peer_id}</span>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <span class="reconnect" onclick={(e: MouseEvent) => { e.stopPropagation(); reconnect(peer); }}>reconnect</span>
        </div>
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
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-dot.online {
    background: #4caf50;
  }

  .status-dot.offline {
    background: #666;
  }

  .peer-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 2px;
  }

  .peer-id {
    font-size: 11px;
    color: #666;
    margin-top: 2px;
  }

  .reconnect {
    font-size: 11px;
    color: #4fc3f7;
    cursor: pointer;
  }

  .reconnect:hover {
    text-decoration: underline;
  }
</style>
