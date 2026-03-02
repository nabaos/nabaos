<script lang="ts">
  import { onMount } from 'svelte';
  import {
    sendQuery, sendQueryStream, type QueryResponse,
    getPersonas, setActivePersona, type PersonaList,
    getStyle, setStyle, clearStyle,
  } from '../lib/api';
  import { Badge, EmptyState, Modal, Button } from '../lib/components';
  import { navigateTo, showToast } from '../lib/stores.svelte';

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

  // Agent/Persona selector
  let personas = $state<string[]>([]);
  let activePersona = $state('');
  let showPersonaDropdown = $state(false);
  let switchingPersona = $state('');

  // Style selector
  let currentStyle = $state('');
  let showStyleDropdown = $state(false);
  const styleOptions = [
    { id: '', label: 'Default', desc: 'Standard responses' },
    { id: 'concise', label: 'Concise', desc: 'Short & direct' },
    { id: 'detailed', label: 'Detailed', desc: 'Thorough explanations' },
    { id: 'technical', label: 'Technical', desc: 'Developer-focused' },
    { id: 'creative', label: 'Creative', desc: 'Imaginative & bold' },
    { id: 'formal', label: 'Formal', desc: 'Professional tone' },
  ];

  // PEA mode
  let peaMode = $state(false);

  // Build Workflow modal
  let showWorkflowModal = $state(false);
  let workflowPrompt = $state('');
  let workflowOutput = $state('');
  let workflowBuilding = $state(false);
  let workflowDone = $state(false);

  // Quick action chips
  const quickActions = [
    { label: 'Create Workflow', icon: '\u26A1', text: 'Create a workflow that ' },
    { label: 'Create PEA', icon: '\uD83E\uDD16', text: 'Create a PEA agent that ' },
    { label: 'Schedule Task', icon: '\uD83D\uDCC5', text: 'Schedule a task to ' },
    { label: 'System Status', icon: '\uD83D\uDCCA', text: 'Show system status' },
  ];

  // Empty state suggestions
  const suggestions = [
    { icon: '\uD83D\uDCA1', text: 'What can you do?' },
    { icon: '\uD83D\uDD27', text: 'Create a workflow to monitor my server' },
    { icon: '\uD83D\uDCCA', text: 'Show system status' },
    { icon: '\uD83D\uDD12', text: 'Scan text for security issues' },
    { icon: '\uD83D\uDCB0', text: 'How much have I saved?' },
  ];

  onMount(async () => {
    try {
      const data = await getPersonas();
      personas = data.personas || [];
      activePersona = data.active || '';
    } catch {}
    try {
      const s = await getStyle();
      currentStyle = s.style || '';
    } catch {}
  });

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

  /** Format message text with markdown-like formatting */
  function formatText(text: string): string {
    let s = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    // Bold: **text**
    s = s.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    // Code blocks: ```...```
    s = s.replace(/```([\s\S]*?)```/g, '<pre class="code-block">$1</pre>');
    // Inline code: `...`
    s = s.replace(/`([^`]+)`/g, '<span class="code-inline">$1</span>');
    // Bullet lists: lines starting with - or *
    s = s.replace(/^[\-\*]\s+(.+)$/gm, '<span class="list-item">\u2022 $1</span>');
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
    } catch {}
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
      if (!text.endsWith(' ')) {
        send();
      }
    }
  }

  async function selectPersona(id: string) {
    if (id === activePersona || switchingPersona) return;
    switchingPersona = id;
    try {
      const result = await setActivePersona(id);
      activePersona = result.active;
      showToast(`Switched to: ${result.active}`, 'success');
    } catch {
      showToast('Failed to switch persona', 'error');
    }
    switchingPersona = '';
    showPersonaDropdown = false;
  }

  async function selectStyle(id: string) {
    try {
      if (id) {
        await setStyle(id);
      } else {
        await clearStyle();
      }
      currentStyle = id;
      showToast(`Style: ${id || 'Default'}`, 'success');
    } catch {
      showToast('Failed to set style', 'error');
    }
    showStyleDropdown = false;
  }

  async function send() {
    const query = input.trim();
    if (!query || loading) return;
    input = '';
    if (textareaEl) { textareaEl.style.height = 'auto'; }

    // Wrap with PEA mode prefix if enabled
    const finalQuery = peaMode ? `[PEA MODE] ${query}` : query;

    messages = [...messages, { role: 'user', text: query, timestamp: Date.now() }];
    persist();
    loading = true;

    const agentMsg: Message = { role: 'agent', text: '', timestamp: Date.now() };
    messages = [...messages, agentMsg];
    const agentIdx = messages.length - 1;

    try {
      await sendQueryStream(finalQuery, {
        onDelta(text: string) {
          messages[agentIdx].text += text;
          messages = [...messages];
        },
        onTier(_info: { tier: string; confidence: number }) {},
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
        const res = await sendQuery(finalQuery);
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
    const userMsg = messages[idx - 1];
    if (!userMsg || userMsg.role !== 'user') return;
    messages = messages.filter((_, i) => i !== idx);
    persist();
    input = userMsg.text;
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

  async function buildWorkflow() {
    if (!workflowPrompt.trim()) return;
    workflowBuilding = true;
    workflowOutput = '';
    workflowDone = false;
    try {
      await sendQueryStream(`Create a workflow: ${workflowPrompt}`, {
        onDelta: (text) => { workflowOutput += text; },
        onDone: () => { workflowDone = true; workflowBuilding = false; },
        onError: (err) => { workflowOutput += `\nError: ${err}`; workflowBuilding = false; },
      });
    } catch (e: any) {
      workflowOutput = `Error: ${e.message}`;
      workflowBuilding = false;
    }
  }

  function resetWorkflowModal() {
    showWorkflowModal = false;
    workflowPrompt = '';
    workflowOutput = '';
    workflowDone = false;
  }

  // Close dropdowns on outside click
  function handleWindowClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest('.persona-dropdown-container')) showPersonaDropdown = false;
    if (!target.closest('.style-dropdown-container')) showStyleDropdown = false;
  }
</script>

<svelte:window onclick={handleWindowClick} />

<div class="chat-container">
  <!-- Header with controls -->
  <div class="chat-header">
    <div class="header-left">
      <h1>Chat</h1>
      {#if peaMode}
        <span class="pea-indicator">PEA</span>
      {/if}
    </div>

    <div class="header-controls">
      <!-- Agent/Persona selector -->
      {#if personas.length > 0}
        <div class="persona-dropdown-container">
          <button
            class="control-btn"
            onclick={(e) => { e.stopPropagation(); showPersonaDropdown = !showPersonaDropdown; showStyleDropdown = false; }}
            title="Switch agent persona"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
            <span class="control-label">{activePersona || 'Agent'}</span>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
          {#if showPersonaDropdown}
            <div class="dropdown-menu fade-in">
              <div class="dropdown-header">Select Agent</div>
              {#each personas as p}
                <button
                  class="dropdown-item"
                  class:active={p === activePersona}
                  class:switching={p === switchingPersona}
                  onclick={(e) => { e.stopPropagation(); selectPersona(p); }}
                >
                  <span class="dropdown-item-dot" class:dot-active={p === activePersona}></span>
                  <span>{p}</span>
                  {#if p === activePersona}
                    <span class="active-label">Active</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Style selector -->
      <div class="style-dropdown-container">
        <button
          class="control-btn"
          onclick={(e) => { e.stopPropagation(); showStyleDropdown = !showStyleDropdown; showPersonaDropdown = false; }}
          title="Response style"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>
          <span class="control-label">{styleOptions.find(s => s.id === currentStyle)?.label || 'Style'}</span>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        {#if showStyleDropdown}
          <div class="dropdown-menu fade-in">
            <div class="dropdown-header">Response Style</div>
            {#each styleOptions as s}
              <button
                class="dropdown-item"
                class:active={s.id === currentStyle}
                onclick={(e) => { e.stopPropagation(); selectStyle(s.id); }}
              >
                <span class="dropdown-item-dot" class:dot-active={s.id === currentStyle}></span>
                <div class="dropdown-item-content">
                  <span class="dropdown-item-label">{s.label}</span>
                  <span class="dropdown-item-desc">{s.desc}</span>
                </div>
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <!-- PEA mode toggle -->
      <button
        class="control-btn pea-toggle"
        class:pea-active={peaMode}
        onclick={() => { peaMode = !peaMode; showToast(peaMode ? 'PEA mode enabled' : 'PEA mode disabled', 'info'); }}
        title="Toggle PEA (autonomous execution) mode"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="12" cy="5" r="4"/><line x1="8" y1="16" x2="8" y2="16.01"/><line x1="16" y1="16" x2="16" y2="16.01"/></svg>
        <span class="control-label">PEA</span>
      </button>

      <!-- Build Workflow button -->
      <button
        class="control-btn workflow-btn"
        onclick={() => showWorkflowModal = true}
        title="Build a new workflow"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>
        <span class="control-label">Build</span>
      </button>

      {#if messages.length > 0}
        <button class="control-btn clear-btn" onclick={clearHistory} title="Clear history">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      {/if}
    </div>
  </div>

  <!-- Messages area -->
  <div class="chat-messages" bind:this={messagesEl}>
    {#if messages.length === 0 && !loading}
      <div class="empty-chat">
        <div class="empty-logo">
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>
          </svg>
        </div>
        <h2 class="empty-title">How can I help you?</h2>
        <p class="empty-desc">Ask anything, create workflows, manage agents, or check your system.</p>
        <div class="suggestions">
          {#each suggestions as s}
            <button class="suggestion-chip" onclick={() => sendSuggestion(s.text)}>
              <span class="suggestion-icon">{s.icon}</span>
              <span>{s.text}</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    {#each messages as msg, idx}
      {#if shouldShowDateSep(idx)}
        <div class="date-separator">
          <span class="date-label">{formatDate(msg.timestamp)}</span>
        </div>
      {/if}

      <div class="chat-msg {msg.role}">
        <div class="avatar {msg.role}">
          {#if msg.role === 'user'}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
          {:else}
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
          {/if}
        </div>
        <div class="msg-content">
          <div class="msg-header">
            <span class="msg-sender">{msg.role === 'user' ? 'You' : 'NabaOS'}</span>
            <span class="msg-time">{formatTime(msg.timestamp)}</span>
          </div>

          {#if msg.error}
            <div class="error-card">
              <div class="error-text">{msg.text}</div>
              <button class="retry-btn" onclick={() => retry(idx)}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
                Retry
              </button>
            </div>
          {:else}
            <div class="chat-bubble {msg.role}">
              {@html formatText(msg.text)}
            </div>
          {/if}

          {#if msg.role === 'agent' && !msg.error && msg.text}
            <div class="msg-actions">
              <button
                class="action-btn"
                title="Copy to clipboard"
                onclick={() => copyText(msg.text, idx)}
              >
                {#if copiedIdx === idx}
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--success)" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>
                {:else}
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                {/if}
              </button>
              {#if msg.response}
                <button class="action-btn" onclick={() => msg.showDetails = !msg.showDetails}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
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
        <div class="avatar agent">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
        </div>
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

  <!-- Input area -->
  <div class="chat-input-area">
    <div class="quick-actions">
      {#each quickActions as qa}
        <button class="action-chip" onclick={() => applyQuickAction(qa.text)}>
          <span class="chip-icon">{qa.icon}</span>
          {qa.label}
        </button>
      {/each}
    </div>

    <form class="chat-input-row" onsubmit={(e) => { e.preventDefault(); send(); }}>
      <div class="input-wrapper">
        {#if peaMode}
          <span class="input-pea-badge">PEA</span>
        {/if}
        <textarea
          class="chat-textarea"
          class:pea-textarea={peaMode}
          placeholder={peaMode ? 'PEA mode: autonomous execution enabled...' : 'Message NabaOS...'}
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
      </div>
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

<!-- Build Workflow Modal -->
<Modal open={showWorkflowModal} onclose={resetWorkflowModal} title="Build Workflow">
  {#if !workflowOutput}
    <div class="wf-modal-intro">
      <p>Describe what your workflow should do and NabaOS will build it for you.</p>
    </div>
    <form onsubmit={(e) => { e.preventDefault(); buildWorkflow(); }}>
      <div class="form-group">
        <label for="wf-desc">Workflow Description</label>
        <textarea
          id="wf-desc"
          bind:value={workflowPrompt}
          placeholder="e.g. Every morning, check for critical security alerts and send a summary to Telegram..."
          rows="4"
        ></textarea>
      </div>
      <div class="wf-modal-actions">
        <Button onclick={resetWorkflowModal}>Cancel</Button>
        <Button variant="primary" type="submit" disabled={workflowBuilding || !workflowPrompt.trim()}>
          {workflowBuilding ? 'Building...' : 'Build Workflow'}
        </Button>
      </div>
    </form>
  {:else}
    <div class="wf-output">
      <div class="wf-output-header">
        <span class="wf-output-title">Workflow Output</span>
        {#if workflowDone}
          <Badge variant="success">Complete</Badge>
        {:else}
          <Badge variant="warning">Building...</Badge>
        {/if}
      </div>
      <div class="wf-output-text">{@html formatText(workflowOutput)}</div>
    </div>
    <div class="wf-modal-actions">
      {#if workflowDone}
        <Button onclick={() => { navigateTo('workflows'); resetWorkflowModal(); }}>View Workflows</Button>
        <Button variant="primary" onclick={resetWorkflowModal}>Done</Button>
      {:else}
        <Button disabled={workflowBuilding} onclick={resetWorkflowModal}>Cancel</Button>
      {/if}
    </div>
  {/if}
</Modal>

<style>
  /* ── Layout ────────────────────────────────────── */
  .chat-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    max-height: calc(100vh - 80px);
  }

  /* ── Header ────────────────────────────────────── */
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.5rem;
    flex-shrink: 0;
    gap: 0.75rem;
    flex-wrap: wrap;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .header-left h1 { margin: 0; }

  .pea-indicator {
    font-size: 0.65rem;
    font-weight: 700;
    padding: 0.15rem 0.45rem;
    border-radius: 4px;
    background: rgba(34, 197, 94, 0.15);
    color: var(--success);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    border: 1px solid rgba(34, 197, 94, 0.3);
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  /* ── Control buttons ───────────────────────────── */
  .control-btn {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-dim);
    font-size: 0.78rem;
    cursor: pointer;
    transition: all 0.15s;
    white-space: nowrap;
  }

  .control-btn:hover {
    background: var(--bg-hover);
    color: var(--text);
    border-color: var(--accent);
  }

  .control-label {
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pea-toggle.pea-active {
    background: rgba(34, 197, 94, 0.1);
    border-color: var(--success);
    color: var(--success);
  }

  .workflow-btn:hover {
    border-color: var(--warning);
    color: var(--warning);
  }

  .clear-btn {
    padding: 0.35rem;
  }

  .clear-btn:hover {
    color: var(--danger);
    border-color: var(--danger);
  }

  /* ── Dropdowns ─────────────────────────────────── */
  .persona-dropdown-container,
  .style-dropdown-container {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    min-width: 200px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow-lg);
    z-index: 100;
    overflow: hidden;
  }

  .dropdown-header {
    padding: 0.5rem 0.75rem;
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    border-bottom: 1px solid var(--border-subtle);
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: none;
    border: none;
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .dropdown-item:hover { background: var(--bg-hover); }
  .dropdown-item.active { background: var(--accent-subtle); }
  .dropdown-item.switching { opacity: 0.5; }

  .dropdown-item-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--border);
    flex-shrink: 0;
  }

  .dropdown-item-dot.dot-active {
    background: var(--success);
    box-shadow: 0 0 4px var(--success);
  }

  .dropdown-item-content {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .dropdown-item-label { font-weight: 500; }
  .dropdown-item-desc { font-size: 0.72rem; color: var(--text-dim); }

  .active-label {
    margin-left: auto;
    font-size: 0.68rem;
    color: var(--success);
    font-weight: 500;
  }

  /* ── Messages ──────────────────────────────────── */
  .chat-messages {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 0;
    scroll-behavior: smooth;
  }

  /* ── Empty state ───────────────────────────────── */
  .empty-chat {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    text-align: center;
    padding: 3rem 1rem;
  }

  .empty-logo {
    width: 72px;
    height: 72px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-subtle);
    border-radius: 20px;
    margin-bottom: 1.25rem;
  }

  .empty-title {
    font-size: 1.25rem;
    font-weight: 600;
    margin: 0 0 0.4rem;
    color: var(--text);
  }

  .empty-desc {
    color: var(--text-dim);
    font-size: 0.9rem;
    margin: 0 0 1.5rem;
    max-width: 400px;
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

  .chat-msg.user { align-self: flex-end; flex-direction: row-reverse; }
  .chat-msg.agent { align-self: flex-start; }

  /* ── Avatars ───────────────────────────────────── */
  .avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-top: 0.15rem;
  }

  .avatar.user {
    background: var(--accent-subtle);
    color: var(--accent);
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
    padding: 0.65rem 0.9rem;
    border-radius: 12px;
    font-size: 0.9rem;
    line-height: 1.6;
    word-break: break-word;
  }

  .chat-bubble.user {
    background: var(--accent-subtle);
    color: var(--text);
    border: 1px solid rgba(124, 111, 255, 0.15);
    border-radius: 12px 12px 2px 12px;
  }

  .chat-bubble.agent {
    background: var(--bg-card);
    color: var(--text);
    border: 1px solid var(--border);
    border-left: 3px solid var(--success);
    border-radius: 12px 12px 12px 2px;
    box-shadow: var(--shadow-sm);
  }

  /* ── Code formatting ───────────────────────────── */
  :global(.code-inline) {
    background: rgba(124, 111, 255, 0.1);
    color: var(--accent);
    padding: 0.1rem 0.35rem;
    border-radius: 4px;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.82em;
  }

  :global(.code-block) {
    background: rgba(0, 0, 0, 0.25);
    color: var(--text);
    padding: 0.75rem 1rem;
    border-radius: 8px;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.82em;
    overflow-x: auto;
    margin: 0.4rem 0;
    white-space: pre-wrap;
    word-break: break-all;
    display: block;
  }

  :global(.list-item) {
    display: block;
    padding-left: 0.25rem;
  }

  /* ── Message Actions ───────────────────────────── */
  .msg-actions {
    display: flex;
    gap: 0.25rem;
    align-items: center;
    margin-top: 0.15rem;
  }

  .action-btn {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    padding: 0.2rem;
    border-radius: 4px;
    display: flex;
    align-items: center;
    transition: color 0.15s, background 0.15s;
  }

  .action-btn:hover {
    background: var(--accent-subtle);
    color: var(--text);
  }

  /* ── Error Card ────────────────────────────────── */
  .error-card {
    background: rgba(248, 113, 113, 0.06);
    border: 1px solid rgba(248, 113, 113, 0.3);
    border-radius: 10px;
    padding: 0.7rem 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .error-text {
    color: var(--danger);
    font-size: 0.85rem;
    line-height: 1.45;
  }

  .retry-btn {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    background: rgba(248, 113, 113, 0.1);
    border: 1px solid rgba(248, 113, 113, 0.25);
    color: var(--danger);
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .retry-btn:hover { background: rgba(248, 113, 113, 0.2); }

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
    background: var(--success);
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
    display: flex;
    align-items: center;
    gap: 0.3rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 0.72rem;
    font-weight: 500;
    padding: 0.3rem 0.65rem;
    border-radius: 999px;
    cursor: pointer;
    transition: all 0.15s;
    white-space: nowrap;
  }

  .chip-icon { font-size: 0.8rem; }

  .action-chip:hover {
    background: var(--accent-subtle);
    border-color: var(--accent);
    color: var(--text);
  }

  .chat-input-row {
    display: flex;
    gap: 0.5rem;
    align-items: flex-end;
  }

  .input-wrapper {
    flex: 1;
    position: relative;
  }

  .input-pea-badge {
    position: absolute;
    left: 0.65rem;
    top: 50%;
    transform: translateY(-50%);
    font-size: 0.6rem;
    font-weight: 700;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    background: rgba(34, 197, 94, 0.15);
    color: var(--success);
    letter-spacing: 0.05em;
    pointer-events: none;
    z-index: 1;
  }

  .chat-textarea {
    min-height: 40px;
    max-height: 160px;
    resize: none;
    width: 100%;
    padding: 0.55rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-card);
    color: var(--text);
    font: inherit;
    font-size: 0.9rem;
    line-height: 1.5;
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .chat-textarea.pea-textarea {
    padding-left: 3rem;
    border-color: rgba(34, 197, 94, 0.3);
  }

  .chat-textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-subtle);
  }

  .chat-textarea.pea-textarea:focus {
    border-color: var(--success);
    box-shadow: 0 0 0 2px rgba(34, 197, 94, 0.15);
  }

  .send-btn {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: none;
    background: var(--accent);
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: opacity 0.15s, transform 0.1s;
    flex-shrink: 0;
  }

  .send-btn:hover:not(:disabled) { opacity: 0.9; transform: scale(1.05); }
  .send-btn:disabled { opacity: 0.4; cursor: not-allowed; }

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
    max-width: 500px;
  }

  .suggestion-chip {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 0.82rem;
    padding: 0.45rem 0.9rem;
    border-radius: 999px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .suggestion-icon { font-size: 0.9rem; }

  .suggestion-chip:hover {
    background: var(--accent-subtle);
    border-color: var(--accent);
  }

  /* ── Workflow Modal ────────────────────────────── */
  .wf-modal-intro {
    margin-bottom: 1rem;
  }

  .wf-modal-intro p {
    color: var(--text-dim);
    font-size: 0.88rem;
    margin: 0;
  }

  .wf-modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .wf-output {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 0.5rem;
    max-height: 350px;
    overflow-y: auto;
  }

  .wf-output-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }

  .wf-output-title {
    font-weight: 600;
    font-size: 0.85rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .wf-output-text {
    font-size: 0.88rem;
    line-height: 1.6;
    color: var(--text);
  }

  /* ── Responsive ────────────────────────────────── */
  @media (max-width: 768px) {
    .control-label { display: none; }
    .header-controls { gap: 0.25rem; }
    .control-btn { padding: 0.3rem 0.4rem; }
    .dropdown-menu { min-width: 180px; }
  }
</style>
