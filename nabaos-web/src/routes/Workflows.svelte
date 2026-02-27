<script lang="ts">
  import { onMount } from 'svelte';
  import { getWorkflows, getScheduledJobs, scheduleWorkflow, disableJob, type Workflow, type ScheduledJob } from '../lib/api';
  import { Card, Badge, Modal, Button, EmptyState, Skeleton } from '../lib/components';

  let workflows = $state<Workflow[]>([]);
  let jobs = $state<ScheduledJob[]>([]);
  let error = $state('');
  let loading = $state(true);

  let showScheduleModal = $state(false);
  let schedWorkflowId = $state('');
  let schedInterval = $state('');
  let schedError = $state('');

  onMount(async () => {
    await loadData();
  });

  async function loadData() {
    loading = true;
    error = '';
    try {
      [workflows, jobs] = await Promise.all([getWorkflows(), getScheduledJobs()]);
    } catch (e: any) {
      error = e.message;
    }
    loading = false;
  }

  function successRate(workflow: Workflow): number {
    return (workflow.success_count / Math.max(workflow.run_count, 1)) * 100;
  }

  function trustLabel(rate: number): { text: string; variant: 'success' | 'warning' | 'info' } {
    if (rate >= 80) return { text: '[trusted]', variant: 'success' };
    if (rate >= 50) return { text: '[learning]', variant: 'warning' };
    return { text: '[new]', variant: 'info' };
  }

  function formatInterval(secs: number): string {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
    return `${Math.floor(secs / 86400)}d`;
  }

  async function handleSchedule() {
    schedError = '';
    try {
      await scheduleWorkflow(schedWorkflowId, schedInterval);
      schedWorkflowId = '';
      schedInterval = '';
      showScheduleModal = false;
      await loadData();
    } catch (e: any) {
      schedError = e.message;
    }
  }

  async function handleDisable(id: string) {
    try {
      await disableJob(id);
      await loadData();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<div class="page-header">
  <h1>Workflows</h1>
  <Button variant="primary" onclick={() => showScheduleModal = true}>Schedule Workflow</Button>
</div>

{#if error}
  <p class="error-text">{error}</p>
{/if}

{#if loading}
  <div class="chains-grid">
    {#each [1, 2, 3] as _}
      <Card>
        <Skeleton width="60%" height="18px" />
        <div style="margin-top: 0.75rem;">
          <Skeleton width="100%" height="14px" />
        </div>
        <div style="margin-top: 0.75rem;">
          <Skeleton width="100%" height="6px" />
        </div>
        <div style="margin-top: 0.75rem;">
          <Skeleton width="50%" height="14px" />
        </div>
      </Card>
    {/each}
  </div>
{:else}
  <h2 class="section-title">Workflows</h2>

  {#if workflows.length === 0}
    <EmptyState icon="🔗" title="No workflows yet" description="Workflows build automatically as you use the agent." />
  {:else}
    <div class="chains-grid">
      {#each workflows as workflow}
        {@const rate = successRate(workflow)}
        {@const trust = trustLabel(rate)}
        <Card hoverable>
          <div class="chain-name">{workflow.name}</div>
          <div class="chain-desc">{workflow.description}</div>
          <div class="trust-bar">
            <Badge variant={trust.variant}>{trust.text}</Badge>
          </div>
          <div class="chain-stats">
            <Badge variant="info">{workflow.run_count} runs</Badge>
            <span class="stats-sep">&middot;</span>
            <Badge variant="info">{workflow.success_count} successes</Badge>
          </div>
          <div class="chain-date">{workflow.created_at}</div>
        </Card>
      {/each}
    </div>
  {/if}

  <h2 class="section-title" style="margin-top: 2rem;">Scheduled Jobs</h2>

  {#if jobs.length === 0}
    <EmptyState icon="📅" title="No scheduled jobs" description="Schedule a workflow to run it on a recurring interval." />
  {:else}
    <div class="chains-grid">
      {#each jobs as job}
        <Card>
          <div class="job-row">
            <span class="mono">{job.workflow_id}</span>
            {#if job.enabled}
              <Badge variant="success">Enabled</Badge>
            {:else}
              <Badge variant="danger">Disabled</Badge>
            {/if}
          </div>
          <div class="job-detail">Interval: <strong>{formatInterval(job.interval_secs)}</strong></div>
          <div class="job-detail">Runs: <strong>{job.run_count}</strong></div>
          {#if job.enabled}
            <div style="margin-top: 0.75rem;">
              <Button variant="danger" size="sm" onclick={() => handleDisable(job.id)}>Disable</Button>
            </div>
          {/if}
        </Card>
      {/each}
    </div>
  {/if}
{/if}

<Modal open={showScheduleModal} onclose={() => showScheduleModal = false} title="Schedule a Workflow">
  <form onsubmit={(e) => { e.preventDefault(); handleSchedule(); }}>
    <div class="form-group" style="margin-bottom: 1rem;">
      <label for="sched-chain">Workflow</label>
      <select id="sched-chain" bind:value={schedWorkflowId}>
        <option value="">Select a workflow...</option>
        {#each workflows as w}
          <option value={w.workflow_id}>{w.name}</option>
        {/each}
      </select>
    </div>
    <div class="form-group" style="margin-bottom: 1rem;">
      <label for="sched-interval">Interval</label>
      <input id="sched-interval" type="text" bind:value={schedInterval} placeholder="e.g. 30s, 5m, 1h, 1d" />
    </div>
    {#if schedError}<p class="error-text">{schedError}</p>{/if}
    <Button variant="primary" type="submit">Schedule</Button>
  </form>
</Modal>

<style>
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1.5rem;
  }
  .page-header h1 {
    margin: 0;
  }
  .chains-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 1rem;
  }
  .chain-name {
    font-weight: 700;
    font-family: var(--font-mono, monospace);
    font-size: 0.95rem;
    margin-bottom: 0.25rem;
  }
  .chain-desc {
    color: var(--text-dim);
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trust-bar {
    margin: 0.5rem 0;
  }
  .chain-stats {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    margin-bottom: 0.35rem;
  }
  .stats-sep {
    color: var(--text-dim);
  }
  .chain-date {
    color: var(--text-dim);
    font-size: 0.8rem;
  }
  .job-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
  }
  .job-detail {
    color: var(--text-dim);
    font-size: 0.85rem;
    margin-bottom: 0.25rem;
  }
  .mono {
    font-family: var(--font-mono, monospace);
    font-size: 0.85rem;
  }
</style>
