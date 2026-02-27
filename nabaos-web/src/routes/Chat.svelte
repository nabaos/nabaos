<script lang="ts">
  import { sendQuery, sendQueryStream, type QueryResponse } from '../lib/api';
  import { Badge, EmptyState } from '../lib/components';

  interface Message {
    role: 'user' | 'agent';
    text: string;
    response?: QueryResponse;
    showDetails?: boolean;
  }

  const STORAGE_KEY = 'nyaya-chat-history';

  let messages = $state<Message[]>(
    JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]')
  );
  let input = $state('');
  let loading = $state(false);
  let messagesEl: HTMLDivElement;

  function persist() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(messages.slice(-100)));
  }

  $effect(() => {
    if (messagesEl && messages.length > 0) {
      messagesEl.scrollTo({ top: messagesEl.scrollHeight, behavior: 'smooth' });
    }
  });

  async function send() {
    const query = input.trim();
    if (!query || loading) return;
    input = '';
    messages = [...messages, { role: 'user', text: query }];
    persist();
    loading = true;

    // Create an empty assistant message for progressive rendering
    const agentMsg: Message = { role: 'agent', text: '' };
    messages = [...messages, agentMsg];
    const agentIdx = messages.length - 1;

    try {
      await sendQueryStream(query, {
        onDelta(text: string) {
          messages[agentIdx].text += text;
          messages = [...messages]; // trigger reactivity
        },
        onTier(info: { tier: string; confidence: number }) {
          // Tier info received; will be finalized in onDone
        },
        onDone(meta: QueryResponse) {
          messages[agentIdx].response = meta;
          messages[agentIdx].text = messages[agentIdx].text || meta.response_text || meta.description;
          messages = [...messages]; // trigger reactivity
        },
        onError(error: string) {
          messages[agentIdx].text = `Error: ${error}`;
          messages = [...messages];
        },
      });
    } catch {
      // Fall back to non-streaming on failure
      try {
        const res = await sendQuery(query);
        messages[agentIdx].text = res.response_text || res.description;
        messages[agentIdx].response = res;
        messages = [...messages];
      } catch (e2: any) {
        messages[agentIdx].text = messages[agentIdx].text || `Error: ${e2.message}`;
        messages = [...messages];
      }
    }
    loading = false;
    persist();
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
      <EmptyState icon="💬" title="Start a conversation" description="Ask your agent anything.">
        <div class="suggestions">
          <button class="btn btn-ghost btn-sm" onclick={() => sendSuggestion('What can you do?')}>What can you do?</button>
          <button class="btn btn-ghost btn-sm" onclick={() => sendSuggestion('Show my costs')}>Show my costs</button>
          <button class="btn btn-ghost btn-sm" onclick={() => sendSuggestion('Show my workflows')}>Show my workflows</button>
        </div>
      </EmptyState>
    {/if}

    {#each messages as msg}
      <div class="chat-msg" style="align-self: {msg.role === 'user' ? 'flex-end' : 'flex-start'};">
        <div class="chat-msg-label {msg.role}">{msg.role}</div>
        <div class="chat-bubble" style="background: {msg.role === 'user' ? 'var(--accent-subtle)' : 'var(--bg-card)'};">
          {msg.text}
        </div>
        {#if msg.response}
          <button class="details-toggle" onclick={() => msg.showDetails = !msg.showDetails}>
            {msg.showDetails ? 'Hide details' : 'Details'}
          </button>
          {#if msg.showDetails}
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
        {/if}
      </div>
    {/each}

    {#if loading}
      <div class="chat-msg" style="align-self: flex-start;">
        <div class="chat-msg-label agent">agent</div>
        <div class="chat-bubble">
          <span class="typing-dots">
            <span></span><span></span><span></span>
          </span>
        </div>
      </div>
    {/if}
  </div>

  <form class="chat-input-row" onsubmit={(e) => { e.preventDefault(); send(); }}>
    <textarea
      class="chat-textarea"
      placeholder="Ask something..."
      bind:value={input}
      onkeydown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          send();
        }
      }}
      rows="1"
    ></textarea>
    <button class="btn btn-primary" type="submit" disabled={loading}>Send</button>
  </form>
</div>

<style>
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.5rem;
  }
  .chat-header h1 {
    margin: 0;
  }
  .chat-messages {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .chat-textarea {
    min-height: 40px;
    max-height: 120px;
    resize: none;
    flex: 1;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-input, var(--bg-card));
    color: var(--text);
    font: inherit;
    line-height: 1.5;
  }
  .chat-textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-subtle, rgba(99, 102, 241, 0.15));
  }
  .details-toggle {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 0.75rem;
    cursor: pointer;
    padding: 0.125rem 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .details-toggle:hover {
    color: var(--text);
  }
  .suggestions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    justify-content: center;
  }
</style>
