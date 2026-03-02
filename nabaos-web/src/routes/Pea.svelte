<script lang="ts">
  import { onMount } from 'svelte';
  import { getAbilities, type Ability, sendQueryStream } from '../lib/api';
  import { Card, Badge, Button, EmptyState, Skeleton, Modal } from '../lib/components';
  import { currentPage } from '../lib/stores';

  let agents = $state<Ability[]>([]);
  let loading = $state(true);
  let error = $state('');
  let showCreateModal = $state(false);
  let createPrompt = $state('');
  let creating = $state(false);
  let createResult = $state('');

  onMount(async () => {
    try {
      agents = await getAbilities();
    } catch (e: any) {
      error = e.message;
    }
    loading = false;
  });

  async function createPea() {
    if (!createPrompt.trim()) return;
    creating = true;
    createResult = '';
    try {
      await sendQueryStream(`Create a PEA agent: ${createPrompt}`, {
        onDelta(text) { createResult += text; },
        onDone() { creating = false; },
        onError(err) { createResult = `Error: ${err}`; creating = false; },
      });
    } catch (e: any) {
      createResult = `Error: ${e.message}`;
      creating = false;
    }
  }

  function goToChat() {
    currentPage.set('chat');
  }

  function getSourceBadge(source: string): 'success' | 'info' | 'warning' {
    if (source.includes('builtin') || source.includes('core')) return 'success';
    if (source.includes('plugin') || source.includes('mcp')) return 'info';
    return 'warning';
  }
</script>

<div class="pea-page">
  <div class="page-header">
    <div>
      <h1>Persistent Execution Agents</h1>
      <p class="subtitle">Autonomous agents that run continuously</p>
    </div>
    <Button variant="primary" onclick={() => showCreateModal = true}>Create PEA</Button>
  </div>

  {#if loading}
    <div class="agents-grid">
      {#each [1, 2, 3, 4] as _}
        <Card><Skeleton height="120px" /></Card>
      {/each}
    </div>
  {:else if error}
    <Card><p class="error-text">{error}</p></Card>
  {:else if agents.length === 0}
    <EmptyState
      icon="🤖"
      title="No agents running"
      description="Create a PEA to automate tasks continuously."
    >
      <div style="display: flex; gap: 0.5rem; justify-content: center; margin-top: 1rem;">
        <Button variant="primary" onclick={() => showCreateModal = true}>Create PEA</Button>
        <Button onclick={goToChat}>Create via Chat</Button>
      </div>
    </EmptyState>
  {:else}
    <div class="agent-stats">
      <div class="stat-pill">
        <span class="stat-pill-value">{agents.length}</span>
        <span class="stat-pill-label">Total Agents</span>
      </div>
      <div class="stat-pill">
        <span class="stat-pill-value success">{agents.filter(a => a.source.includes('builtin')).length}</span>
        <span class="stat-pill-label">Built-in</span>
      </div>
      <div class="stat-pill">
        <span class="stat-pill-value accent">{agents.filter(a => !a.source.includes('builtin')).length}</span>
        <span class="stat-pill-label">Custom</span>
      </div>
    </div>

    <div class="agents-grid">
      {#each agents as agent}
        <div class="agent-card">
          <div class="agent-card-header">
            <div class="agent-icon">🤖</div>
            <div class="agent-info">
              <span class="agent-name">{agent.name}</span>
              <Badge variant={getSourceBadge(agent.source)}>{agent.source}</Badge>
            </div>
          </div>
          <p class="agent-desc">{agent.description || 'No description available'}</p>
        </div>
      {/each}
    </div>
  {/if}
</div>

<Modal open={showCreateModal} onclose={() => { showCreateModal = false; createResult = ''; createPrompt = ''; }} title="Create PEA Agent">
  <div class="create-form">
    <div class="form-group">
      <label>What should this agent do?</label>
      <textarea
        bind:value={createPrompt}
        placeholder="e.g., Monitor my server logs and alert me when errors spike..."
        rows="4"
      ></textarea>
    </div>
    {#if createResult}
      <div class="create-result">
        <pre class="code-block">{createResult}</pre>
      </div>
    {/if}
    <div style="display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1rem;">
      <Button onclick={() => { showCreateModal = false; createResult = ''; createPrompt = ''; }}>Cancel</Button>
      <Button variant="primary" loading={creating} onclick={createPea}>
        {creating ? 'Creating...' : 'Create Agent'}
      </Button>
    </div>
  </div>
</Modal>

<style>
  .pea-page { max-width: 1000px; }
  .page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 1.5rem; }
  .page-header h1 { margin: 0; }
  .subtitle { color: var(--text-dim); font-size: 0.9rem; margin-top: 0.25rem; }

  .agent-stats { display: flex; gap: 1rem; margin-bottom: 1.5rem; }
  .stat-pill {
    display: flex; flex-direction: column; align-items: center;
    padding: 0.75rem 1.5rem; background: var(--bg-card); border: 1px solid var(--border);
    border-radius: var(--radius-md); min-width: 100px;
  }
  .stat-pill-value { font-size: 1.5rem; font-weight: 700; font-family: var(--font-mono, 'JetBrains Mono', monospace); }
  .stat-pill-value.success { color: var(--success); }
  .stat-pill-value.accent { color: var(--accent); }
  .stat-pill-label { font-size: 0.75rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.05em; }

  .agents-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 1rem; }
  .agent-card {
    background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-md);
    padding: 1.25rem; transition: transform 0.15s ease, box-shadow 0.15s ease;
  }
  .agent-card:hover { transform: translateY(-2px); box-shadow: var(--shadow-md); }
  .agent-card-header { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.75rem; }
  .agent-icon { font-size: 1.5rem; width: 40px; height: 40px; display: flex; align-items: center; justify-content: center; background: var(--bg-hover); border-radius: var(--radius-sm); }
  .agent-info { display: flex; flex-direction: column; gap: 0.25rem; }
  .agent-name { font-weight: 700; font-size: 0.95rem; }
  .agent-desc { color: var(--text-dim); font-size: 0.85rem; line-height: 1.5; }

  .create-form textarea { width: 100%; min-height: 100px; }
  .create-result { margin-top: 1rem; max-height: 200px; overflow-y: auto; }
</style>
