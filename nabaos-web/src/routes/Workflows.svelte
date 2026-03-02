<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getWorkflows, getScheduledJobs, scheduleWorkflow, disableJob,
    sendQueryStream,
    type Workflow, type ScheduledJob
  } from '../lib/api';
  import { Card, Badge, Modal, Button, EmptyState, Skeleton, StatCard } from '../lib/components';
  import { navigateTo } from '../lib/stores.svelte';

  // ---------- State ----------
  let workflows = $state<Workflow[]>([]);
  let jobs = $state<ScheduledJob[]>([]);
  let error = $state('');
  let loading = $state(true);
  let toast = $state('');
  let toastTimeout: ReturnType<typeof setTimeout> | undefined;

  // Schedule modal
  let showScheduleModal = $state(false);
  let schedWorkflowId = $state('');
  let schedInterval = $state('');
  let schedCustom = $state('');
  let schedError = $state('');
  let schedSuccess = $state('');
  let scheduling = $state(false);

  // Create workflow modal
  let showCreateModal = $state(false);
  let createPrompt = $state('');
  let createOutput = $state('');
  let creating = $state(false);
  let createDone = $state(false);

  // Disable confirmation
  let confirmDisableId = $state('');

  // Interval presets
  const intervalPresets = [
    { label: 'Every 5 min', value: '5m' },
    { label: 'Every hour', value: '1h' },
    { label: 'Every day', value: '1d' },
    { label: 'Every week', value: '7d' },
  ];

  // ---------- Lifecycle ----------
  onMount(async () => {
    await loadData();
  });

  // ---------- Data ----------
  async function loadData() {
    loading = true;
    error = '';
    try { workflows = await getWorkflows(); } catch (e: any) { error = e.message; workflows = []; }
    try { jobs = await getScheduledJobs(); } catch { jobs = []; }
    loading = false;
  }

  // ---------- Derived ----------
  function successRate(w: Workflow): number {
    return w.run_count === 0 ? 0 : Math.round((w.success_count / w.run_count) * 100);
  }

  function trustBadge(level: number): { text: string; variant: 'success' | 'warning' | 'info' } {
    if (level >= 3) return { text: 'Trusted', variant: 'success' };
    if (level >= 2) return { text: 'Learning', variant: 'warning' };
    return { text: 'New', variant: 'info' };
  }

  function formatInterval(secs: number): string {
    if (secs < 60) return `${secs} seconds`;
    if (secs < 3600) {
      const m = Math.floor(secs / 60);
      return m === 1 ? 'Every minute' : `Every ${m} minutes`;
    }
    if (secs < 86400) {
      const h = Math.floor(secs / 3600);
      return h === 1 ? 'Every hour' : `Every ${h} hours`;
    }
    const d = Math.floor(secs / 86400);
    return d === 1 ? 'Every day' : d === 7 ? 'Every week' : `Every ${d} days`;
  }

  function formatDate(iso: string): string {
    try {
      const d = new Date(iso);
      return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
    } catch {
      return iso;
    }
  }

  function workflowNameById(id: string): string {
    const w = workflows.find(w => w.workflow_id === id);
    return w ? w.name : id.slice(0, 12) + '...';
  }

  // Stats
  let totalWorkflows = $derived(workflows.length);
  let totalScheduled = $derived(jobs.filter(j => j.enabled).length);
  let avgSuccessRate = $derived(
    workflows.length === 0 ? 0 :
    Math.round(workflows.reduce((acc, w) => acc + successRate(w), 0) / workflows.length)
  );
  let totalRuns = $derived(workflows.reduce((acc, w) => acc + w.run_count, 0));

  // ---------- Actions ----------
  function showToast(msg: string) {
    toast = msg;
    if (toastTimeout) clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => { toast = ''; }, 3000);
  }

  function handleRunNow(w: Workflow) {
    showToast(`Workflow "${w.name}" started`);
  }

  function selectPreset(value: string) {
    schedInterval = value;
    schedCustom = '';
  }

  function handleCustomInterval() {
    schedInterval = schedCustom;
  }

  async function handleSchedule() {
    schedError = '';
    schedSuccess = '';
    const interval = schedCustom || schedInterval;
    if (!schedWorkflowId) { schedError = 'Please select a workflow.'; return; }
    if (!interval) { schedError = 'Please select or enter an interval.'; return; }
    scheduling = true;
    try {
      await scheduleWorkflow(schedWorkflowId, interval);
      schedSuccess = 'Job scheduled successfully!';
      schedWorkflowId = '';
      schedInterval = '';
      schedCustom = '';
      await loadData();
      setTimeout(() => {
        showScheduleModal = false;
        schedSuccess = '';
      }, 1200);
    } catch (e: any) {
      schedError = e.message;
    }
    scheduling = false;
  }

  async function handleDisable(id: string) {
    try {
      await disableJob(id);
      confirmDisableId = '';
      showToast('Job disabled');
      await loadData();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function handleCreateWorkflow() {
    if (!createPrompt.trim()) return;
    creating = true;
    createOutput = '';
    createDone = false;
    try {
      await sendQueryStream(`Create a workflow: ${createPrompt}`, {
        onDelta: (text) => { createOutput += text; },
        onDone: () => {
          createDone = true;
          creating = false;
          loadData();
        },
        onError: (err) => {
          createOutput += `\n\nError: ${err}`;
          creating = false;
        }
      });
    } catch (e: any) {
      createOutput = `Error: ${e.message}`;
      creating = false;
    }
  }

  function resetCreateModal() {
    showCreateModal = false;
    createPrompt = '';
    createOutput = '';
    createDone = false;
  }

  function goToChat() {
    navigateTo('chat');
  }
</script>

<!-- Toast -->
{#if toast}
  <div class="toast fade-in">{toast}</div>
{/if}

<!-- Page header -->
<div class="page-header">
  <h1>Workflows</h1>
  <div class="header-actions">
    <Button variant="secondary" onclick={() => showCreateModal = true}>Create Workflow</Button>
    <Button variant="primary" onclick={() => showScheduleModal = true}>Schedule Workflow</Button>
  </div>
</div>

<!-- Error banner -->
{#if error}
  <div class="error-banner">
    <span>{error}</span>
    <button class="error-dismiss" onclick={() => error = ''}>x</button>
  </div>
{/if}

<!-- Loading skeleton -->
{#if loading}
  <div class="stats-row">
    {#each [1, 2, 3, 4] as _}
      <div class="stat-skeleton"><Skeleton width="100%" height="70px" /></div>
    {/each}
  </div>
  <div class="wf-grid">
    {#each [1, 2, 3] as _}
      <Card>
        <Skeleton width="40%" height="16px" />
        <div style="margin-top: 0.75rem;"><Skeleton width="90%" height="14px" /></div>
        <div style="margin-top: 0.75rem;"><Skeleton width="100%" height="6px" /></div>
        <div style="margin-top: 0.75rem;"><Skeleton width="50%" height="14px" /></div>
      </Card>
    {/each}
  </div>
{:else}

  <!-- Stats header -->
  <div class="stats-row">
    <StatCard value={totalWorkflows} label="Workflows" color="accent" />
    <StatCard value={totalScheduled} label="Scheduled" color="success" />
    <StatCard value={totalRuns} label="Total Runs" color="default" />
    <StatCard value="{avgSuccessRate}%" label="Avg Success" color={avgSuccessRate >= 80 ? 'success' : avgSuccessRate >= 50 ? 'warning' : 'danger'} />
  </div>

  <!-- Workflows section -->
  <h2 class="section-title">Workflows</h2>

  {#if workflows.length === 0}
    <EmptyState icon="🔗" title="No workflows yet" description="Workflows are reusable chains of actions the agent can perform. Create one by describing what you want automated, or they will build automatically as you use the chat.">
      <Button variant="primary" onclick={goToChat}>Create via Chat</Button>
      <Button variant="secondary" onclick={() => showCreateModal = true}>Describe a Workflow</Button>
    </EmptyState>
  {:else}
    <div class="wf-grid">
      {#each workflows as workflow}
        {@const rate = successRate(workflow)}
        {@const trust = trustBadge(workflow.trust_level)}
        <Card hoverable>
          <div class="wf-card-header">
            <div class="wf-icon">
              {#if trust.variant === 'success'}
                <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><path d="M10 1l2.5 5.5L18 7.5l-4 4 1 5.5L10 14.5 4.5 17l1-5.5-4-4 5.5-1z" fill="var(--success)" opacity="0.85"/></svg>
              {:else if trust.variant === 'warning'}
                <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="7" stroke="var(--warning)" stroke-width="2" fill="none"/><path d="M10 6v5M10 13v1" stroke="var(--warning)" stroke-width="2" stroke-linecap="round"/></svg>
              {:else}
                <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><rect x="3" y="3" width="14" height="14" rx="3" stroke="var(--info, #60a5fa)" stroke-width="2" fill="none"/><path d="M7 10h6M10 7v6" stroke="var(--info, #60a5fa)" stroke-width="2" stroke-linecap="round"/></svg>
              {/if}
            </div>
            <div class="wf-title-group">
              <div class="wf-name">{workflow.name}</div>
              <Badge variant={trust.variant}>{trust.text}</Badge>
            </div>
          </div>

          <p class="wf-desc">{workflow.description}</p>

          <!-- Success rate bar -->
          <div class="wf-rate-row">
            <span class="wf-rate-label">Success rate</span>
            <span class="wf-rate-value">{rate}%</span>
          </div>
          <div class="wf-progress-track">
            <div
              class="wf-progress-fill"
              class:fill-good={rate >= 80}
              class:fill-mid={rate >= 50 && rate < 80}
              class:fill-low={rate < 50}
              style="width: {rate}%"
            ></div>
          </div>

          <!-- Stats row -->
          <div class="wf-stats-row">
            <span class="wf-stat">{workflow.run_count} runs</span>
            <span class="wf-stat-sep"></span>
            <span class="wf-stat">{workflow.success_count} successes</span>
          </div>

          <!-- Footer -->
          <div class="wf-footer">
            <span class="wf-date">{formatDate(workflow.created_at)}</span>
            <Button variant="secondary" size="sm" onclick={() => handleRunNow(workflow)}>Run Now</Button>
          </div>
        </Card>
      {/each}
    </div>
  {/if}

  <!-- Scheduled Jobs section -->
  <h2 class="section-title" style="margin-top: 2rem;">Scheduled Jobs</h2>

  {#if jobs.length === 0}
    <EmptyState icon="📅" title="No scheduled jobs" description="Schedule a workflow to run on a recurring interval. Choose from presets or define a custom cadence.">
      <Button variant="primary" onclick={() => showScheduleModal = true}>Schedule a Workflow</Button>
    </EmptyState>
  {:else}
    <div class="wf-grid">
      {#each jobs as job}
        <Card hoverable>
          <div class="job-header">
            <div class="job-name">{workflowNameById(job.workflow_id)}</div>
            {#if job.enabled}
              <Badge variant="success">Active</Badge>
            {:else}
              <Badge variant="neutral">Disabled</Badge>
            {/if}
          </div>

          <div class="job-details">
            <div class="job-detail-item">
              <span class="job-detail-label">Interval</span>
              <span class="job-detail-value">{formatInterval(job.interval_secs)}</span>
            </div>
            <div class="job-detail-item">
              <span class="job-detail-label">Runs</span>
              <span class="job-detail-value">{job.run_count}</span>
            </div>
            <div class="job-detail-item">
              <span class="job-detail-label">Created</span>
              <span class="job-detail-value">{formatDate(job.created_at)}</span>
            </div>
          </div>

          {#if job.enabled}
            <div class="job-actions">
              {#if confirmDisableId === job.id}
                <span class="confirm-text">Disable this job?</span>
                <Button variant="danger" size="sm" onclick={() => handleDisable(job.id)}>Confirm</Button>
                <Button variant="secondary" size="sm" onclick={() => confirmDisableId = ''}>Cancel</Button>
              {:else}
                <Button variant="danger" size="sm" onclick={() => confirmDisableId = job.id}>Disable</Button>
              {/if}
            </div>
          {/if}
        </Card>
      {/each}
    </div>
  {/if}
{/if}

<!-- Schedule Modal -->
<Modal open={showScheduleModal} onclose={() => { showScheduleModal = false; schedError = ''; schedSuccess = ''; }} title="Schedule a Workflow">
  <form onsubmit={(e) => { e.preventDefault(); handleSchedule(); }}>
    <div class="form-group">
      <label for="sched-wf">Workflow</label>
      <select id="sched-wf" bind:value={schedWorkflowId} class="form-select">
        <option value="">Select a workflow...</option>
        {#each workflows as w}
          <option value={w.workflow_id}>{w.name}</option>
        {/each}
      </select>
    </div>

    <div class="form-group">
      <label>Interval</label>
      <div class="interval-chips">
        {#each intervalPresets as preset}
          <button
            type="button"
            class="chip"
            class:chip-active={schedInterval === preset.value && !schedCustom}
            onclick={() => selectPreset(preset.value)}
          >{preset.label}</button>
        {/each}
      </div>
      <div class="custom-interval">
        <input
          type="text"
          bind:value={schedCustom}
          oninput={handleCustomInterval}
          placeholder="Or enter custom: 30s, 15m, 2h, 3d"
          class="form-input"
        />
      </div>
    </div>

    {#if schedError}<p class="form-error">{schedError}</p>{/if}
    {#if schedSuccess}<p class="form-success">{schedSuccess}</p>{/if}

    <div class="modal-actions">
      <Button variant="secondary" onclick={() => showScheduleModal = false}>Cancel</Button>
      <Button variant="primary" type="submit" disabled={scheduling}>
        {scheduling ? 'Scheduling...' : 'Schedule'}
      </Button>
    </div>
  </form>
</Modal>

<!-- Create Workflow Modal -->
<Modal open={showCreateModal} onclose={resetCreateModal} title="Create a Workflow">
  {#if !createOutput}
    <form onsubmit={(e) => { e.preventDefault(); handleCreateWorkflow(); }}>
      <div class="form-group">
        <label for="create-desc">Describe what this workflow should do</label>
        <textarea
          id="create-desc"
          bind:value={createPrompt}
          placeholder="e.g. Every morning, check my email for urgent messages and summarize them into a daily briefing..."
          rows="4"
          class="form-textarea"
        ></textarea>
      </div>
      <div class="modal-actions">
        <Button variant="secondary" onclick={resetCreateModal}>Cancel</Button>
        <Button variant="primary" type="submit" disabled={creating || !createPrompt.trim()}>
          {creating ? 'Creating...' : 'Create Workflow'}
        </Button>
      </div>
    </form>
  {:else}
    <div class="create-output">
      <div class="create-output-text">{createOutput}</div>
      {#if creating}
        <div class="create-spinner">Generating...</div>
      {/if}
    </div>
    <div class="modal-actions">
      {#if createDone}
        <Button variant="primary" onclick={resetCreateModal}>Done</Button>
      {:else}
        <Button variant="secondary" disabled={creating} onclick={resetCreateModal}>Cancel</Button>
      {/if}
    </div>
  {/if}
</Modal>

<style>
  /* ---------- Layout ---------- */
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1.5rem;
    flex-wrap: wrap;
    gap: 0.75rem;
  }
  .page-header h1 { margin: 0; }
  .header-actions { display: flex; gap: 0.5rem; }

  .stats-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 0.75rem;
    margin-bottom: 1.75rem;
  }
  .stat-skeleton {
    border-radius: var(--radius-md, 8px);
    overflow: hidden;
  }

  .section-title {
    font-size: 1rem;
    font-weight: 600;
    margin-bottom: 1rem;
    color: var(--text);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.8;
  }

  .wf-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 1rem;
  }

  /* ---------- Workflow card ---------- */
  .wf-card-header {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    margin-bottom: 0.5rem;
  }
  .wf-icon {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 1px;
  }
  .wf-title-group {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
    min-width: 0;
  }
  .wf-name {
    font-weight: 700;
    font-size: 0.95rem;
    font-family: var(--font-mono, monospace);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .wf-desc {
    color: var(--text-dim);
    font-size: 0.85rem;
    line-height: 1.4;
    margin: 0 0 0.75rem 0;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* Success rate */
  .wf-rate-row {
    display: flex;
    justify-content: space-between;
    font-size: 0.78rem;
    margin-bottom: 0.3rem;
  }
  .wf-rate-label { color: var(--text-dim); }
  .wf-rate-value { font-weight: 600; font-variant-numeric: tabular-nums; }

  .wf-progress-track {
    width: 100%;
    height: 5px;
    background: var(--border, rgba(255,255,255,0.08));
    border-radius: 3px;
    overflow: hidden;
    margin-bottom: 0.75rem;
  }
  .wf-progress-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.4s ease;
  }
  .fill-good { background: var(--success, #34d399); }
  .fill-mid  { background: var(--warning, #fbbf24); }
  .fill-low  { background: var(--danger, #f87171); }

  /* Stats */
  .wf-stats-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.82rem;
    color: var(--text-dim);
    margin-bottom: 0.5rem;
  }
  .wf-stat { font-variant-numeric: tabular-nums; }
  .wf-stat-sep {
    width: 3px; height: 3px;
    border-radius: 50%;
    background: var(--text-dim);
    opacity: 0.5;
  }

  /* Footer */
  .wf-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border, rgba(255,255,255,0.06));
  }
  .wf-date {
    font-size: 0.78rem;
    color: var(--text-dim);
  }

  /* ---------- Job card ---------- */
  .job-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }
  .job-name {
    font-weight: 600;
    font-family: var(--font-mono, monospace);
    font-size: 0.9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job-details {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-bottom: 0.75rem;
  }
  .job-detail-item {
    display: flex;
    justify-content: space-between;
    font-size: 0.83rem;
  }
  .job-detail-label { color: var(--text-dim); }
  .job-detail-value { font-weight: 500; font-variant-numeric: tabular-nums; }
  .job-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border, rgba(255,255,255,0.06));
  }
  .confirm-text {
    font-size: 0.83rem;
    color: var(--danger, #f87171);
    margin-right: auto;
  }

  /* ---------- Error ---------- */
  .error-banner {
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: rgba(248, 113, 113, 0.1);
    border: 1px solid var(--danger, #f87171);
    border-radius: var(--radius-md, 8px);
    padding: 0.75rem 1rem;
    margin-bottom: 1rem;
    color: var(--danger, #f87171);
    font-size: 0.9rem;
  }
  .error-dismiss {
    background: none;
    border: none;
    color: var(--danger, #f87171);
    cursor: pointer;
    font-size: 1rem;
    padding: 0 0.25rem;
    opacity: 0.7;
  }
  .error-dismiss:hover { opacity: 1; }

  /* ---------- Toast ---------- */
  .toast {
    position: fixed;
    bottom: 1.5rem;
    right: 1.5rem;
    background: var(--bg-card, #1e1e2a);
    border: 1px solid var(--border, rgba(255,255,255,0.1));
    color: var(--text, #e5e5ea);
    padding: 0.75rem 1.25rem;
    border-radius: var(--radius-md, 8px);
    font-size: 0.88rem;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
    z-index: 1000;
    animation: slide-in 0.25s ease;
  }
  @keyframes slide-in {
    from { transform: translateY(12px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }

  /* ---------- Forms (modal) ---------- */
  .form-group {
    margin-bottom: 1.25rem;
  }
  .form-group label {
    display: block;
    font-size: 0.83rem;
    font-weight: 600;
    color: var(--text-dim);
    margin-bottom: 0.4rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .form-select,
  .form-input,
  .form-textarea {
    width: 100%;
    padding: 0.6rem 0.75rem;
    background: var(--bg, #16161e);
    border: 1px solid var(--border, rgba(255,255,255,0.1));
    border-radius: var(--radius-sm, 6px);
    color: var(--text, #e5e5ea);
    font-size: 0.9rem;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s;
    box-sizing: border-box;
  }
  .form-select:focus,
  .form-input:focus,
  .form-textarea:focus {
    border-color: var(--accent, #ffaf5f);
  }
  .form-textarea {
    resize: vertical;
    min-height: 80px;
    line-height: 1.5;
  }

  .interval-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.6rem;
  }
  .chip {
    padding: 0.4rem 0.85rem;
    border-radius: 999px;
    border: 1px solid var(--border, rgba(255,255,255,0.1));
    background: transparent;
    color: var(--text-dim);
    font-size: 0.82rem;
    cursor: pointer;
    transition: all 0.15s;
  }
  .chip:hover {
    border-color: var(--accent, #ffaf5f);
    color: var(--text);
  }
  .chip-active {
    background: var(--accent, #ffaf5f);
    color: #16161e;
    border-color: var(--accent, #ffaf5f);
    font-weight: 600;
  }
  .custom-interval {
    margin-top: 0.25rem;
  }

  .form-error {
    color: var(--danger, #f87171);
    font-size: 0.85rem;
    margin: 0 0 0.75rem 0;
  }
  .form-success {
    color: var(--success, #34d399);
    font-size: 0.85rem;
    margin: 0 0 0.75rem 0;
  }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  /* Create modal output */
  .create-output {
    background: var(--bg, #16161e);
    border: 1px solid var(--border, rgba(255,255,255,0.08));
    border-radius: var(--radius-sm, 6px);
    padding: 1rem;
    margin-bottom: 1rem;
    max-height: 300px;
    overflow-y: auto;
  }
  .create-output-text {
    white-space: pre-wrap;
    font-size: 0.88rem;
    line-height: 1.5;
    color: var(--text);
  }
  .create-spinner {
    margin-top: 0.75rem;
    font-size: 0.82rem;
    color: var(--accent, #ffaf5f);
    animation: pulse 1.2s infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
</style>
