<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getCosts, getCostsDashboard, type CostData, type CostsDashboard, getSystemStatus, type SystemStatus } from '../lib/api';
  import { Card, StatCard, Badge, ChartWrapper, Skeleton, EmptyState } from '../lib/components';

  let status = $state<SystemStatus | null>(null);
  let costs = $state<CostData | null>(null);
  let costsDash = $state<CostsDashboard | null>(null);
  let loading = $state(true);
  let chartData = $state<any>(null);
  let intervalId: ReturnType<typeof setInterval> | null = null;

  let totalSaved = $derived(costs?.total_saved_usd ?? 0);
  let totalSpent = $derived(costs?.total_spent_usd ?? 0);
  let cacheHits = $derived(costs?.total_cache_hits ?? 0);
  let llmCalls = $derived(costs?.total_llm_calls ?? 0);
  let savingsPercent = $derived(totalSpent + totalSaved > 0 ? (totalSaved / (totalSpent + totalSaved)) * 100 : 0);
  let cacheHitRate = $derived(cacheHits + llmCalls > 0 ? (cacheHits / (cacheHits + llmCalls)) * 100 : 0);

  async function fetchData() {
    try { status = await getSystemStatus(); } catch {}
    try { costs = await getCosts(); } catch {}
    try { costsDash = await getCostsDashboard(); } catch {}
    loading = false;
  }

  $effect(() => {
    if (costsDash) {
      chartData = {
        labels: ['Daily', 'Weekly', 'Monthly', 'All Time'],
        datasets: [{
          label: 'Saved ($)',
          data: [costsDash.daily.total_saved, costsDash.weekly.total_saved, costsDash.monthly.total_saved, costsDash.all_time.total_saved],
          backgroundColor: [
            getComputedStyle(document.documentElement).getPropertyValue('--success').trim(),
            'rgba(52, 211, 153, 0.7)',
            'rgba(52, 211, 153, 0.5)',
            'rgba(52, 211, 153, 0.3)',
          ],
          borderWidth: 0,
          borderRadius: 6,
        }]
      };
    }
  });

  onMount(() => {
    fetchData();
    intervalId = setInterval(fetchData, 30000);
  });

  onDestroy(() => {
    if (intervalId) clearInterval(intervalId);
  });

  function formatMoney(n: number): string {
    if (n >= 1) return `$${n.toFixed(2)}`;
    if (n >= 0.01) return `$${n.toFixed(4)}`;
    return `$${n.toFixed(6)}`;
  }

  function formatUptime(secs: number): string {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    if (secs < 86400) return `${h}h ${m}m`;
    const d = Math.floor(secs / 86400);
    return `${d}d ${h % 24}h`;
  }
</script>

<div class="watcher-page">
  <div class="page-header">
    <div>
      <h1>Watcher Dashboard</h1>
      <p class="subtitle">Security monitoring, cost tracking, and savings</p>
    </div>
    {#if status?.watcher_enabled}
      <Badge variant="success">Watcher Active</Badge>
    {:else}
      <Badge variant="neutral">Watcher Inactive</Badge>
    {/if}
  </div>

  {#if loading}
    <div class="stats-grid">
      {#each [1, 2, 3, 4] as _}
        <Skeleton height="100px" />
      {/each}
    </div>
  {:else}
    <!-- Hero: Total Savings -->
    <div class="savings-hero">
      <div class="savings-amount">{formatMoney(totalSaved)}</div>
      <div class="savings-label">Total Saved to Date</div>
      <div class="savings-detail">
        {savingsPercent.toFixed(1)}% of all queries handled for free via intelligent caching
      </div>
    </div>

    <!-- Stats Grid -->
    <div class="stats-grid">
      <StatCard value={formatMoney(totalSpent)} label="Total Spent" color="warning" />
      <StatCard value={formatMoney(totalSaved)} label="Total Saved" color="success" />
      <StatCard value={cacheHitRate.toFixed(1) + '%'} label="Cache Hit Rate" color="accent" />
      <StatCard value={status ? formatUptime(status.uptime_secs) : '—'} label="Uptime" color="info" />
    </div>

    <!-- Savings Breakdown -->
    {#if costsDash}
      <h2 class="section-heading">Savings Breakdown</h2>
      <div class="breakdown-grid">
        <Card>
          <div class="breakdown-header">Today</div>
          <div class="breakdown-value success">{formatMoney(costsDash.daily.total_saved)}</div>
          <div class="breakdown-detail">{costsDash.daily.cache_hits} cache hits &middot; {costsDash.daily.total_calls} calls</div>
        </Card>
        <Card>
          <div class="breakdown-header">This Week</div>
          <div class="breakdown-value success">{formatMoney(costsDash.weekly.total_saved)}</div>
          <div class="breakdown-detail">{costsDash.weekly.cache_hits} cache hits &middot; {costsDash.weekly.total_calls} calls</div>
        </Card>
        <Card>
          <div class="breakdown-header">This Month</div>
          <div class="breakdown-value success">{formatMoney(costsDash.monthly.total_saved)}</div>
          <div class="breakdown-detail">{costsDash.monthly.cache_hits} cache hits &middot; {costsDash.monthly.total_calls} calls</div>
        </Card>
        <Card>
          <div class="breakdown-header">All Time</div>
          <div class="breakdown-value success">{formatMoney(costsDash.all_time.total_saved)}</div>
          <div class="breakdown-detail">{costsDash.all_time.cache_hits} cache hits &middot; {costsDash.all_time.total_calls} calls</div>
        </Card>
      </div>

      <!-- Chart -->
      <div class="chart-section">
        <h2 class="section-heading">Savings Over Time</h2>
        <div class="chart-container">
          {#if chartData}
            <ChartWrapper
              type="bar"
              data={chartData}
              options={{
                plugins: { legend: { display: false } },
                responsive: true,
                maintainAspectRatio: true,
                scales: {
                  y: { beginAtZero: true, ticks: { callback: (v: number) => '$' + v.toFixed(2) } },
                  x: { grid: { display: false } }
                }
              }}
            />
          {/if}
        </div>
      </div>
    {/if}

    <!-- Watcher Status -->
    {#if status?.watcher_enabled}
      <h2 class="section-heading">Watcher Status</h2>
      <div class="watcher-status-grid">
        <Card>
          <div class="watcher-metric">
            <div class="watcher-metric-label">Alerts</div>
            <div class="watcher-metric-value" class:danger={status.watcher_alerts > 0}>
              {status.watcher_alerts}
            </div>
          </div>
        </Card>
        <Card>
          <div class="watcher-metric">
            <div class="watcher-metric-label">Paused Components</div>
            <div class="watcher-metric-value" class:warning={status.watcher_paused > 0}>
              {status.watcher_paused}
            </div>
          </div>
        </Card>
        <Card>
          <div class="watcher-metric">
            <div class="watcher-metric-label">Active Channels</div>
            <div class="watcher-metric-value">{status.channels.length}</div>
          </div>
        </Card>
      </div>
    {:else}
      <div class="watcher-inactive-notice">
        <Card>
          <div style="text-align: center; padding: 1.5rem;">
            <div style="font-size: 2rem; margin-bottom: 0.75rem;">🛡️</div>
            <h3>Watcher is not enabled</h3>
            <p class="subtitle">Enable the watcher via the setup wizard to monitor anomalies, auto-pause risky components, and receive alerts.</p>
            <p class="subtitle" style="margin-top: 0.5rem;"><code>nabaos setup</code> → Step 12: Runtime Watcher</p>
          </div>
        </Card>
      </div>
    {/if}

    <!-- Token Usage -->
    {#if costs}
      <h2 class="section-heading">Token Usage</h2>
      <div class="stats-grid">
        <StatCard value={(costs.total_input_tokens / 1000).toFixed(1) + 'K'} label="Input Tokens" />
        <StatCard value={(costs.total_output_tokens / 1000).toFixed(1) + 'K'} label="Output Tokens" />
        <StatCard value={((costs.total_input_tokens + costs.total_output_tokens) / 1000).toFixed(1) + 'K'} label="Total Tokens" />
      </div>
    {/if}
  {/if}
</div>

<style>
  .watcher-page { max-width: 1000px; }
  .page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 1.5rem; }
  .page-header h1 { margin: 0; }
  .subtitle { color: var(--text-dim); font-size: 0.9rem; margin-top: 0.25rem; }

  .savings-hero {
    text-align: center; padding: 2.5rem 2rem; margin-bottom: 1.5rem;
    background: linear-gradient(135deg, var(--bg-card) 0%, rgba(34, 197, 94, 0.04) 50%, var(--bg-card) 100%);
    border: 1px solid rgba(34, 197, 94, 0.2); border-radius: var(--radius-lg);
    position: relative; overflow: hidden;
  }
  .savings-hero::before {
    content: '';
    position: absolute;
    top: 0; left: 0; right: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--success), transparent);
    opacity: 0.5;
  }
  .savings-amount {
    font-size: 3rem; font-weight: 800; color: var(--success);
    font-family: var(--font-mono, 'JetBrains Mono', monospace); font-variant-numeric: tabular-nums;
  }
  .savings-label { font-size: 1rem; font-weight: 600; color: var(--text); margin-top: 0.25rem; }
  .savings-detail { font-size: 0.88rem; color: var(--text-dim); margin-top: 0.5rem; }

  .section-heading { font-size: 1.1rem; font-weight: 600; margin: 2rem 0 1rem; color: var(--text); }

  .breakdown-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; }
  .breakdown-header { font-size: 0.8rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 0.5rem; }
  .breakdown-value { font-size: 1.5rem; font-weight: 700; font-family: 'SF Mono', monospace; }
  .breakdown-value.success { color: var(--success); }
  .breakdown-detail { font-size: 0.82rem; color: var(--text-dim); margin-top: 0.35rem; }

  .chart-section { margin: 2rem 0; }
  .chart-container { max-width: 600px; margin: 0 auto; }

  .watcher-status-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }
  .watcher-metric { text-align: center; padding: 0.5rem; }
  .watcher-metric-label { font-size: 0.8rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 0.5rem; }
  .watcher-metric-value { font-size: 2rem; font-weight: 700; font-family: 'SF Mono', monospace; }
  .watcher-metric-value.danger { color: var(--danger); }
  .watcher-metric-value.warning { color: var(--warning); }

  .watcher-inactive-notice { margin: 2rem 0; }

  @media (max-width: 768px) {
    .savings-amount { font-size: 2rem; }
    .watcher-status-grid { grid-template-columns: 1fr; }
    .breakdown-grid { grid-template-columns: 1fr 1fr; }
  }
</style>
