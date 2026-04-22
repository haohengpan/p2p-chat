<script lang="ts">
  import { setup } from "../lib/api";
  import { nodeId, username, listenAddr, lanIp } from "../lib/stores";

  let { onDone }: { onDone: () => void } = $props();

  let inputNodeId = $state("");
  let inputUsername = $state("");
  let inputPort = $state(0);
  let error = $state("");
  let loading = $state(false);

  async function handleSubmit() {
    const nid = inputNodeId.trim();
    const uname = inputUsername.trim();
    if (!nid) { error = "Node ID cannot be empty"; return; }
    if (!uname) { error = "Username cannot be empty"; return; }

    loading = true;
    error = "";
    try {
      const result = await setup(nid, uname, inputPort);
      nodeId.set(nid);
      username.set(uname);
      listenAddr.set(result.listen_addr);
      lanIp.set(result.lan_ip);
      onDone();
    } catch (e: any) {
      error = String(e);
      loading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleSubmit();
  }
</script>

<div class="setup-container">
  <div class="setup-card">
    <h1>P2P Chat</h1>
    <p class="subtitle">Setup your identity to start chatting</p>

    <div class="field">
      <label for="node-id">Node ID <span class="hint">(unique, no spaces)</span></label>
      <input
        id="node-id"
        type="text"
        bind:value={inputNodeId}
        onkeydown={handleKeydown}
        placeholder="e.g. alice"
        disabled={loading}
      />
    </div>

    <div class="field">
      <label for="username">Username <span class="hint">(display name)</span></label>
      <input
        id="username"
        type="text"
        bind:value={inputUsername}
        onkeydown={handleKeydown}
        placeholder="e.g. Alice"
        disabled={loading}
      />
    </div>

    <div class="field">
      <label for="port">Port <span class="hint">(0 = auto-detect)</span></label>
      <input
        id="port"
        type="number"
        bind:value={inputPort}
        onkeydown={handleKeydown}
        placeholder="0"
        min="0"
        max="65535"
        disabled={loading}
      />
    </div>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <button onclick={handleSubmit} disabled={loading}>
      {loading ? "Starting..." : "Start Chat"}
    </button>
  </div>
</div>

<style>
  .setup-container {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
  }

  .setup-card {
    background: #1e2a3a;
    border: 1px solid #2a3a4a;
    border-radius: 12px;
    padding: 40px;
    width: 420px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  }

  h1 {
    text-align: center;
    color: #4fc3f7;
    margin-bottom: 4px;
    font-size: 28px;
  }

  .subtitle {
    text-align: center;
    color: #888;
    margin-bottom: 28px;
    font-size: 14px;
  }

  .field {
    margin-bottom: 18px;
  }

  label {
    display: block;
    margin-bottom: 6px;
    color: #b0c4de;
    font-size: 14px;
    font-weight: 500;
  }

  .hint {
    color: #666;
    font-weight: normal;
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
    transition: border-color 0.2s;
  }

  input:focus {
    border-color: #4fc3f7;
  }

  input:disabled {
    opacity: 0.5;
  }

  .error {
    color: #ef5350;
    font-size: 13px;
    margin-bottom: 12px;
    padding: 8px 12px;
    background: rgba(239, 83, 80, 0.1);
    border-radius: 6px;
  }

  button {
    width: 100%;
    padding: 12px;
    background: #4fc3f7;
    color: #0d1b2a;
    border: none;
    border-radius: 6px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  button:hover:not(:disabled) {
    background: #29b6f6;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
