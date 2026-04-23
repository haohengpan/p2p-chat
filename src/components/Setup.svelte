<script lang="ts">
  import { onMount } from "svelte";
  import { setup, login, listProfiles, type Profile } from "../lib/api";
  import { nodeId, username, listenAddr, lanIp } from "../lib/stores";

  let { onDone }: { onDone: () => void } = $props();

  let profiles = $state<Profile[]>([]);
  let inputNodeId = $state("");
  let inputUsername = $state("");
  let inputPassword = $state("");
  let inputPasswordConfirm = $state("");
  let inputPort = $state(0);
  let error = $state("");
  let loading = $state(false);
  let mode = $state<"select" | "login" | "register">("select");
  let selectedProfile = $state<Profile | null>(null);

  onMount(async () => {
    try {
      profiles = await listProfiles();
      if (profiles.length === 0) {
        mode = "register";
      }
    } catch (_) {
      mode = "register";
    }
  });

  function selectProfile(p: Profile) {
    selectedProfile = p;
    inputPassword = "";
    error = "";
    mode = "login";
  }

  async function handleLogin() {
    if (!selectedProfile) return;
    if (!inputPassword) { error = "Please enter your password"; return; }

    loading = true;
    error = "";
    try {
      const result = await login(selectedProfile.node_id, inputPassword);
      nodeId.set(selectedProfile.node_id);
      username.set(selectedProfile.username);
      listenAddr.set(result.listen_addr);
      lanIp.set(result.lan_ip);
      onDone();
    } catch (e: any) {
      error = String(e);
      loading = false;
    }
  }

  function switchToRegister() {
    mode = "register";
    inputNodeId = "";
    inputUsername = "";
    inputPassword = "";
    inputPasswordConfirm = "";
    inputPort = 0;
    error = "";
    selectedProfile = null;
  }

  async function handleRegister() {
    const nid = inputNodeId.trim();
    const uname = inputUsername.trim();
    if (!nid) { error = "Node ID cannot be empty"; return; }
    if (!uname) { error = "Username cannot be empty"; return; }
    if (!inputPassword) { error = "Password cannot be empty"; return; }
    if (inputPassword.length < 4) { error = "Password must be at least 4 characters"; return; }
    if (inputPassword !== inputPasswordConfirm) { error = "Passwords do not match"; return; }

    loading = true;
    error = "";
    try {
      const result = await setup(nid, uname, inputPassword, inputPort);
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
    if (e.key === "Enter") {
      if (mode === "login") handleLogin();
      else if (mode === "register") handleRegister();
    }
  }
</script>

<div class="setup-container">
  <div class="setup-card">
    <h1>P2P Chat</h1>

    {#if mode === "select" && profiles.length > 0}
      <p class="subtitle">Select an account to login</p>

      <div class="profile-list">
        {#each profiles as p}
          <button class="profile-item" onclick={() => selectProfile(p)} disabled={loading}>
            <span class="profile-name">{p.username}</span>
            <span class="profile-id">{p.node_id} · port {p.port || "auto"}</span>
          </button>
        {/each}
      </div>

      <button class="register-link" onclick={switchToRegister} disabled={loading}>
        + Register new account
      </button>

    {:else if mode === "login" && selectedProfile}
      <p class="subtitle">Login as <strong>{selectedProfile.username}</strong></p>

      <div class="field">
        <label for="login-password">Password</label>
        <input
          id="login-password"
          type="password"
          bind:value={inputPassword}
          onkeydown={handleKeydown}
          placeholder="Enter your password"
          disabled={loading}
        />
      </div>

      <button onclick={handleLogin} disabled={loading}>
        {loading ? "Logging in..." : "Login"}
      </button>

      <button class="register-link" onclick={() => { mode = "select"; error = ""; }} disabled={loading}>
        Back to account list
      </button>

    {:else}
      <p class="subtitle">Register a new identity</p>

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
        <label for="password">Password</label>
        <input
          id="password"
          type="password"
          bind:value={inputPassword}
          onkeydown={handleKeydown}
          placeholder="At least 4 characters"
          disabled={loading}
        />
      </div>

      <div class="field">
        <label for="password-confirm">Confirm Password</label>
        <input
          id="password-confirm"
          type="password"
          bind:value={inputPasswordConfirm}
          onkeydown={handleKeydown}
          placeholder="Re-enter password"
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

      <button onclick={handleRegister} disabled={loading}>
        {loading ? "Starting..." : "Register & Start"}
      </button>

      {#if profiles.length > 0}
        <button class="register-link" onclick={() => { mode = "select"; error = ""; }} disabled={loading}>
          Back to login
        </button>
      {/if}
    {/if}

    {#if error}
      <div class="error">{error}</div>
    {/if}
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
    margin-bottom: 24px;
    font-size: 14px;
  }

  .subtitle strong {
    color: #4fc3f7;
  }

  .profile-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }

  .profile-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 14px 18px;
    background: #0d1b2a;
    border: 1px solid #2a3a4a;
    border-radius: 8px;
    color: #e0e0e0;
    cursor: pointer;
    transition: border-color 0.2s, background 0.2s;
    width: 100%;
  }

  .profile-item:hover:not(:disabled) {
    border-color: #4fc3f7;
    background: #162840;
  }

  .profile-name {
    font-size: 16px;
    font-weight: 600;
    color: #e0e0e0;
  }

  .profile-id {
    font-size: 12px;
    color: #666;
    margin-top: 2px;
  }

  .register-link {
    display: block;
    width: 100%;
    padding: 10px;
    background: none;
    border: 1px dashed #2a3a4a;
    border-radius: 8px;
    color: #4fc3f7;
    font-size: 14px;
    cursor: pointer;
    text-align: center;
    transition: border-color 0.2s;
    margin-top: 12px;
  }

  .register-link:hover:not(:disabled) {
    border-color: #4fc3f7;
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
    margin-top: 12px;
    padding: 8px 12px;
    background: rgba(239, 83, 80, 0.1);
    border-radius: 6px;
  }

  button:not(.profile-item):not(.register-link) {
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

  button:not(.profile-item):not(.register-link):hover:not(:disabled) {
    background: #29b6f6;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
