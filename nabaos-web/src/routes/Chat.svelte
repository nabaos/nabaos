<script lang="ts">
  import { sendQuery, sendQueryStream, type QueryResponse } from '../lib/api';
  import { Badge, EmptyState } from '../lib/components';

  interface Message {
    role: 'user' | 'agent';
    text: string;
    timestamp: number;
    response?: QueryResponse;
    showDetails?: boolean;
    error?: boolean;
  }

  const STORAGE_KEY = 'nabaos-chat-history';

  let messages = $state<Message[]>(
    JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]')
  );
  let input = $state('');
  let loading = $state(false);
  let messagesEl: HTMLDivElement;
  let textareaEl: HTMLTextAreaElement;
  let copiedIdx = $state<number | null>(null);

  // Quick action chips
  const quickActions = [
    { label: 'Create Workflow', text: 'Create a workflow that ' },
    { label: 'Create PEA', text: 'Create a PEA agent that ' },
    { label: 'Schedule Task', text: 'Schedule a task to ' },
    { label: 'System Status', text: 'Show system status' },
  ];

  // Empty state suggestions
  const suggestions = [
    'What can you do?',
    'Create a workflow to monitor my server',
    'Show system status',
    'Scan text for security issues',
    'How much have I saved?',
  ];

  function persist() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(messages.slice(-100)));
  }

  function formatTime(ts: number): string {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  }

  function formatDate(ts: number): string {
    const d = new Date(ts);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    if (d.toDateString() === today.toDateString()) return 'Today';
    if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
    return d.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' });
  }

  function shouldShowDateSep(idx: number): boolean {
    if (idx === 0) return true;
    const prev = new Date(messages[idx - 1].timestamp).toDateString();
    const curr = new Date(messages[idx].timestamp).toDateString();
    return prev !== curr;
  }

  /** Format message text: inline `code` and ```code blocks``` */
  function formatText(text: string): string {
    // Escape HTML first
    let s = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    // Code blocks: ```...```
    s = s.replace(/```([\s\S]*?)```/g, '<pre class="code-block">$1</pre>');
    // Inline code: `...`
    s = s.replace(/`([^`]+)`/g, '<span class="code-inline">$1</span>');
    // Newlines to <br> (but not inside <pre>)
    s = s.replace(/\n/g, '<br>');
    return s;
  }

  function autoGrow() {
    if (!textareaEl) return;
    textareaEl.style.height = 'auto';
    textareaEl.style.height = Math.min(textareaEl.scrollHeight, 160) + 'px';
  }

  async function copyText(text: string, idx: number) {
    try {
      await navigator.clipboard.writeText(text);
      copiedIdx = idx;
      setTimeout(() => { copiedIdx = null; }, 1500);
    } catch { /* clipboard not available */ }
  }

  $effect(() => {
    if (messagesEl && messages.length > 0) {
      messagesEl.scrollTo({ top: messagesEl.scrollHeight, behavior: 'smooth' });
    }
  });

  function applyQuickAction(text: string) {
    input = text;
    if (textareaEl) {
      textareaEl.focus();
      // For actions that end with a space (templates), keep cursor in input
      // For complete actions like "Show system status", send immediately
      if (!text.endsWith(' ')) {
        send();
      }
    }
  }

  async function send() {
    const query = input.trim();
    if (!query || loading) return;
    input = '';
    if (textareaEl) { textareaEl.style.height = 'auto'; }
    messages = [...messages, { role: 'user', text: query, timestamp: Date.now() }];
    persist();
    loading = true;

    const agentMsg: Message = { role: 'agent', text: '', timestamp: Date.now() };
    messages = [...messages, agentMsg];
    const agentIdx = messages.length - 1;

    try {
      await sendQueryStream(query, {
        onDelta(text: string) {
          messages[agentIdx].text += text;
          messages = [...messages];
        },
        onTier(_info: { tier: string; confidence: number }) {
          // Tier info received; finalized in onDone
        },
        onDone(meta: QueryResponse) {
          messages[agentIdx].response = meta;
          messages[agentIdx].text = messages[agentIdx].text || meta.response_text || meta.description;
          messages[agentIdx].timestamp = Date.now();
          messages = [...messages];
        },
        onError(error: string) {
          messages[agentIdx].text = error;
          messages[agentIdx].error = true;
          messages = [...messages];
        },
      });
    } catch {
      try {
        const res = await sendQuery(query);
        messages[agentIdx].text = res.response_text || res.description;
        messages[agentIdx].response = res;
        messages[agentIdx].timestamp = Date.now();
        messages = [...messages];
      } catch (e2: any) {
        messages[agentIdx].text = messages[agentIdx].text || e2.message;
        messages[agentIdx].error = true;
        messages = [...messages];
      }
    }
    loading = false;
    persist();
  }

  function retry(idx: number) {
    // Find the user message right before this agent error
    const userMsg = messages[idx - 1];
    if (!userMsg || userMsg.role !== 'user') return;
    // Remove the errored agent message
    messages = messages.filter((_, i) => i !== idx);
    persist();
    input = userMsg.text;
    // Remove the original user message too so send() re-adds it
    messages = messages.filter((_, i) => i !== idx - 1);
    persist();
    send();
  }

  function sendSuggestion(text: string) {
    input = text;
    send();
  }

  function clearHistory() {
    messages = [];
    localStorage.removeItem(STORAGE_KEY);
  }
</script>

<div class="chat-container">
  <div class="chat-header">
    <h1>Chat</h1>
    {#if messages.length > 0}
      <button class="btn btn-ghost btn-sm" onclick={clearHistory}>Clear history</button>
    {/if}
  </div>

  <div class="chat-messages" bind:this={messagesEl}>
    {#if messages.length === 0 && !loading}
      <EmptyState icon="💬" title="Start a conversation" description="Ask NabaOS anything — create workflows, manage PEAs, check costs, or scan for threats.">
        <div class="suggestions">
          {#each suggestions as s}
            <button class="suggestion-chip" onclick={() => sendSuggestion(s)}>{s}</button>
          {/each}
        </div>
      </EmptyState>
    {/if}

    {#each messages as msg, idx}
      {#if shouldShowDateSep(idx)}
        <div class="date-separator">
          <span class="date-label">{formatDate(msg.timestamp)}</span>
        </div>
      {/if}

      <div class="chat-msg {msg.role}">
        <div class="avatar {msg.role}">
          {msg.role === 'user' ? 'U' : 'N'}
        </div>
        <div class="msg-content">
          <div class="msg-header">
            <span class="msg-sender">{msg.role === 'user' ? 'You' : 'NabaOS'}</span>
            <span class="msg-time">{formatTime(msg.timestamp)}</span>
          </div>

          {#if msg.error}
            <div class="error-card">
              <div class="error-text">{msg.text}</div>
              <button class="retry-btn" onclick={() => retry(idx)}>Retry</button>
            </div>
          {:else}
            <div class="chat-bubble {msg.role}">
              {@html formatText(msg.text)}
            </div>
          {/if}

          {#if msg.role === 'agent' && !msg.error && msg.text}
            <div class="msg-actions">
              <button
                class="copy-btn"
                title="Copy to clipboard"
                onclick={() => copyText(msg.text, idx)}
              >
                {copiedIdx === idx ? 'Copied!' : 'Copy'}
              </button>
              {#if msg.response}
                <button class="details-toggle" onclick={() => msg.showDetails = !msg.showDetails}>
                  {msg.showDetails ? 'Hide details' : 'Details'}
                </button>
              {/if}
            </div>
          {/if}

          {#if msg.response && msg.showDetails}
            <div class="chat-meta fade-in">
              <Badge variant="info">{msg.response.tier}</Badge>
              <Badge variant="info">{msg.response.latency_ms.toFixed(0)}ms</Badge>
              <Badge variant="info">{(msg.response.confidence * 100).toFixed(0)}%</Badge>
              {#if msg.response.security.injection_detected}
                <Badge variant="danger">Injection</Badge>
              {/if}
              {#if msg.response.security.credentials_found > 0}
                <Badge variant="warning">{msg.response.security.credentials_found} cred(s)</Badge>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/each}

    {#if loading}
      <div class="chat-msg agent">
        <div class="avatar agent">N</div>
        <div class="msg-content">
          <div class="msg-header">
            <span class="msg-sender">NabaOS</span>
          </div>
          <div class="chat-bubble agent">
            <span class="typing-indicator">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </span>
          </div>
        </div>
      </div>
    {/if}
  </div>

  <div class="chat-input-area">
    <div class="quick-actions">
      {#each quickActions as qa}
        <button class="action-chip" onclick={() => applyQuickAction(qa.text)}>
          {qa.label}
        </button>
      {/each}
    </div>

    <form class="chat-input-row" onsubmit={(e) => { e.preventDefault(); send(); }}>
      <textarea
        class="chat-textarea"
        placeholder="Ask NabaOS something..."
        bind:value={input}
        bind:this={textareaEl}
        oninput={autoGrow}
        onkeydown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
        rows="1"
      ></textarea>
      <button class="send-btn" type="submit" disabled={loading || !input.trim()}>
        {#if loading}
          <span class="btn-spinner"></span>
        {:else}
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="22" y1="2" x2="11" y2="13"></line>
            <polygon points="22 2 15 22 11 13 2 9 22 2"></polygon>
          </svg>
        {/if}
      </button>
    </form>
  </div>
</div>

<style>
  /* ── Layout ────────────────────────────────────── */
  .chat-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    max-height: calc(100vh - 80px);
  }

  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.5rem;
    flex-shrink: 0;
  }

  .chat-header h1 {
    margin: 0;
  }

  .chat-messages {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 0;
    scroll-behavior: smooth;
  }

  /* ── Date Separator ────────────────────────────── */
  .date-separator {
    display: flex;
    align-items: center;
    justify-content: center;
    margin: 1rem 0 0.5rem;
  }

  .date-label {
    font-size: 0.72rem;
    color: var(--text-dim);
    background: var(--bg-card);
    padding: 0.2rem 0.75rem;
    border-radius: 999px;
    border: 1px solid var(--border);
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  /* ── Message Row ───────────────────────────────── */
  .chat-msg {
    display: flex;
    gap: 0.6rem;
    padding: 0.5rem 0;
    max-width: 85%;
  }

  .chat-msg.user {
    align-self: flex-end;
    flex-direction: row-reverse;
  }

  .chat-msg.agent {
    align-self: flex-start;
  }

  /* ── Avatars ───────────────────────────────────── */
  .avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.8rem;
    font-weight: 700;
    flex-shrink: 0;
    margin-top: 0.15rem;
  }

  .avatar.user {
    background: var(--accent-subtle, rgba(99, 102, 241, 0.15));
    color: var(--accent, #6366f1);
  }

  .avatar.agent {
    background: rgba(34, 197, 94, 0.12);
    color: #22c55e;
  }

  /* ── Message Content ───────────────────────────── */
  .msg-content {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    min-width: 0;
  }

  .msg-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .msg-sender {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-dim);
  }

  .msg-time {
    font-size: 0.65rem;
    color: var(--text-dim);
    opacity: 0.6;
  }

  /* ── Bubbles ───────────────────────────────────── */
  .chat-bubble {
    padding: 0.6rem 0.85rem;
    border-radius: 12px;
    font-size: 0.9rem;
    line-height: 1.55;
    word-break: break-word;
  }

  .chat-bubble.user {
    background: var(--accent-subtle, rgba(99, 102, 241, 0.1));
    color: var(--text);
    border: 1px solid rgba(99, 102, 241, 0.15);
    border-radius: 12px 12px 2px 12px;
  }

  .chat-bubble.agent {
    background: var(--bg-card);
    color: var(--text);
    border: 1px solid var(--border);
    border-left: 3px solid #22c55e;
    border-radius: 12px 12px 12px 2px;
    box-shadow: var(--shadow-sm, 0 1px 2px rgba(0,0,0,0.05));
  }

  /* ── Code formatting ───────────────────────────── */
  :global(.code-inline) {
    background: rgba(99, 102, 241, 0.1);
    color: var(--accent, #6366f1);
    padding: 0.1rem 0.35rem;
    border-radius: 4px;
    font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
    font-size: 0.82em;
  }

  :global(.code-block) {
    background: rgba(0, 0, 0, 0.25);
    color: var(--text);
    padding: 0.75rem 1rem;
    border-radius: 8px;
    font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
    font-size: 0.82em;
    overflow-x: auto;
    margin: 0.4rem 0;
    white-space: pre-wrap;
    word-break: break-all;
    display: block;
  }

  /* ── Message Actions ───────────────────────────── */
  .msg-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-top: 0.15rem;
  }

  .copy-btn, .details-toggle {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 0.7rem;
    cursor: pointer;
    padding: 0.15rem 0.35rem;
    border-radius: 4px;
    transition: background 0.15s, color 0.15s;
  }

  .copy-btn:hover, .details-toggle:hover {
    background: var(--accent-subtle, rgba(99, 102, 241, 0.1));
    color: var(--text);
  }

  /* ── Error Card ────────────────────────────────── */
  .error-card {
    background: rgba(239, 68, 68, 0.06);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 10px;
    padding: 0.7rem 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .error-text {
    color: #ef4444;
    font-size: 0.85rem;
    line-height: 1.45;
  }

  .retry-btn {
    align-self: flex-start;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.25);
    color: #ef4444;
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .retry-btn:hover {
    background: rgba(239, 68, 68, 0.2);
  }

  /* ── Details Meta ──────────────────────────────── */
  .chat-meta {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
    margin-top: 0.25rem;
  }

  /* ── Typing Indicator ──────────────────────────── */
  .typing-indicator {
    display: flex;
    gap: 4px;
    align-items: center;
    padding: 0.2rem 0;
  }

  .typing-indicator .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #22c55e;
    opacity: 0.4;
    animation: typing-bounce 1.4s infinite ease-in-out;
  }

  .typing-indicator .dot:nth-child(1) { animation-delay: 0s; }
  .typing-indicator .dot:nth-child(2) { animation-delay: 0.2s; }
  .typing-indicator .dot:nth-child(3) { animation-delay: 0.4s; }

  @keyframes typing-bounce {
    0%, 60%, 100% { transform: translateY(0); opacity: 0.4; }
    30% { transform: translateY(-5px); opacity: 1; }
  }

  /* ── Input Area ────────────────────────────────── */
  .chat-input-area {
    flex-shrink: 0;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
    margin-top: 0.5rem;
  }

  .quick-actions {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    margin-bottom: 0.5rem;
  }

  .action-chip {
    background: var(--bg-card);
    border: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 0.72rem;
    font-weight: 500;
    padding: 0.3rem 0.65rem;
    border-radius: 999px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    white-space: nowrap;
  }

  .action-chip:hover {
    background: var(--accent-subtle, rgba(99, 102, 241, 0.1));
    border-color: var(--accent, #6366f1);
    color: var(--text);
  }

  .chat-input-row {
    display: flex;
    gap: 0.5rem;
    align-items: flex-end;
  }

  .chat-textarea {
    min-height: 40px;
    max-height: 160px;
    resize: none;
    flex: 1;
    padding: 0.55rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-input, var(--bg-card));
    color: var(--text);
    font: inherit;
    font-size: 0.9rem;
    line-height: 1.5;
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .chat-textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-subtle, rgba(99, 102, 241, 0.15));
  }

  .send-btn {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: none;
    background: var(--accent, #6366f1);
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: opacity 0.15s, transform 0.1s;
    flex-shrink: 0;
  }

  .send-btn:hover:not(:disabled) {
    opacity: 0.9;
    transform: scale(1.05);
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .btn-spinner {
    display: inline-block;
    width: 16px;
    height: 16px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  /* ── Suggestions ───────────────────────────────── */
  .suggestions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    justify-content: center;
  }

  .suggestion-chip {
    background: var(--bg-card);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 0.8rem;
    padding: 0.4rem 0.85rem;
    border-radius: 999px;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }

  .suggestion-chip:hover {
    background: var(--accent-subtle, rgba(99, 102, 241, 0.1));
    border-color: var(--accent, #6366f1);
  }
</style>
