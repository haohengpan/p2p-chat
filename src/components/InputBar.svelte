<script lang="ts">
  import { activeConv, nextMsgId, addOutgoingMessage } from "../lib/stores";
  import { sendMessage } from "../lib/api";

  let input = $state("");

  async function send() {
    const text = input.trim();
    if (!text || !$activeConv) return;

    const convId = $activeConv;
    const msgId = nextMsgId();

    // Optimistic update
    addOutgoingMessage(convId, msgId, text);
    input = "";

    try {
      await sendMessage(convId, msgId, text);
    } catch (e) {
      // MessageAck will handle the error via store
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }
</script>

<div class="input-bar">
  <input
    type="text"
    bind:value={input}
    onkeydown={handleKeydown}
    placeholder="Type a message..."
  />
  <button onclick={send} disabled={!input.trim()}>Send</button>
</div>

<style>
  .input-bar {
    display: flex;
    gap: 8px;
    padding: 12px 20px;
    background: #162032;
    border-top: 1px solid #2a3a4a;
    flex-shrink: 0;
  }

  input {
    flex: 1;
    padding: 10px 14px;
    background: #0d1b2a;
    border: 1px solid #2a3a4a;
    border-radius: 8px;
    color: #e0e0e0;
    font-size: 14px;
    outline: none;
    transition: border-color 0.2s;
  }

  input:focus {
    border-color: #4fc3f7;
  }

  input::placeholder {
    color: #555;
  }

  button {
    padding: 10px 20px;
    background: #4fc3f7;
    color: #0d1b2a;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  button:hover:not(:disabled) {
    background: #29b6f6;
  }

  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
