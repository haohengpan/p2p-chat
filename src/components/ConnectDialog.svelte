<script lang="ts">
  import { connect } from "../lib/api";

  let { onClose }: { onClose: () => void } = $props();

  let addr = $state("");
  let error = $state("");
  let loading = $state(false);

  async function handleConnect() {
    const a = addr.trim();
    if (!a) { error = "Address cannot be empty"; return; }

    loading = true;
    error = "";
    try {
      await connect(a);
      error = "Command sent, waiting for response...";
      loading = false;
      // Don't close immediately — let user see the notice bar for result
      setTimeout(() => onClose(), 2000);
    } catch (e: any) {
      error = String(e);
      loading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleConnect();
    if (e.key === "Escape") onClose();
  }

  function handleBackdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="overlay" onclick={handleBackdrop}>
  <div class="dialog">
    <h3>Connect to Peer</h3>
    <p class="hint">Enter IP:port, IP (auto-probe ports), or node ID</p>

    <input
      type="text"
      bind:value={addr}
      onkeydown={handleKeydown}
      placeholder="e.g. 192.168.1.100:9000"
      disabled={loading}
    />

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <div class="buttons">
      <button class="cancel" onclick={onClose} disabled={loading}>Cancel</button>
      <button class="confirm" onclick={handleConnect} disabled={loading}>
        {loading ? "Connecting..." : "Connect"}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: #1e2a3a;
    border: 1px solid #2a3a4a;
    border-radius: 12px;
    padding: 28px;
    width: 380px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  h3 {
    margin: 0 0 4px;
    color: #4fc3f7;
    font-size: 18px;
  }

  .hint {
    color: #666;
    font-size: 13px;
    margin-bottom: 16px;
  }

  input {
    width: 100%;
    padding: 10px 14px;
    background: #0d1b2a;
    border: 1px solid #2a3a4a;
    border-radius: 6px;
    color: #e0e0e0;
    font-size: 15px;
    outline: none;
    margin-bottom: 12px;
  }

  input:focus {
    border-color: #4fc3f7;
  }

  .error {
    color: #ef5350;
    font-size: 13px;
    margin-bottom: 12px;
  }

  .buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  button {
    padding: 8px 20px;
    border: none;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
  }

  .cancel {
    background: #2a3a4a;
    color: #b0c4de;
  }

  .cancel:hover {
    background: #3a4a5a;
  }

  .confirm {
    background: #4fc3f7;
    color: #0d1b2a;
  }

  .confirm:hover:not(:disabled) {
    background: #29b6f6;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
