<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    getDashboard, getCostsDashboard, getSystemStatus,
    type DashboardData, type CostsDashboard, type SystemStatus,
  } from '../lib/api';
  import { StatCard, Card, ChartWrapper, Skeleton, EmptyState, Button } from '../lib/components';
  import { currentPage } from '../lib/stores';

  // ── State ──────────────────────────────────────────────────────────────

  let data = $state<DashboardData | null>(null);
  let costsDash = $state<CostsDashboard | null>(null);
  let system = $state<SystemStatus | null>(null);
  let error = $state('');
  let loading = $state(true);
  let refreshing = $state(false);
  let chartData = $state<any>(null);
  let intervalId: ReturnType<typeof setInterval> | null = null;

  // ── Derived values ─────────────────────────────────────────────────────

  let spent = $derived(data?.costs.total_spent_usd ?? 0);
  let saved = $derived(data?.costs.total_saved_usd ?? 0);
  let cacheHits = $derived(data?.costs.total_cache_hits ?? 0);
  let llmCalls = $derived(data?.costs.total_llm_calls ?? 0);
  let totalQueries = $derived(cacheHits + llmCalls);
  let cachePercent = $derived(totalQueries > 0 ? (cacheHits / totalQueries) * 100 : 0);
  let savingsPercent = $derived(data?.costs.savings_percent ?? 0);
  let workflows = $derived(data?.total_workflows ?? 0);
  let abilities = $derived(data?.total_abilities ?? 0);

  let cacheColor = $derived<'success' | 'warning' | 'danger'>(
    cachePercent > 80 ? 'success' : cachePercent > 50 ? 'warning' : 'danger'
  );

  function isEmpty(d: DashboardData): boolean {
    return d.costs.total_cache_hits === 0
      && d.costs.total_llm_calls === 0
      && d.costs.total_spent_usd === 0
      && d.costs.total_saved_usd === 0
      && d.total_workflows === 0
      && d.total_scheduled_jobs === 0
      && d.total_abilities === 0;
  }

  function formatUsd(n: number): string {
    if (n >= 1) return '$' + n.toFixed(2);
    if (n >= 0.01) return '$' + n.toFixed(3);
    return '$' + n.toFixed(4);
  }

  function formatUptime(secs: number): string {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
    const days = Math.floor(secs / 86400);
    const hours = Math.floor((secs % 86400) / 3600);
    return `${days}d ${hours}h`;
  }

  // ── Chart effect ───────────────────────────────────────────────────────

  $effect(() => {
    if (data) {
      const s = spent;
      const sv = saved;
      // Ensure chart has at least some data to render
      if (s === 0 && sv === 0) {
        chartData = null;
      } else {
        chartData = {
          labels: ['Spent', 'Saved'],
          datasets: [{
            data: [s, sv],
            backgroundColor: [
              getComputedStyle(document.documentElement).getPropertyValue('--warning').trim() || '#f59e0b',
              getComputedStyle(document.documentElement).getPropertyValue('--success').trim() || '#10b981',
            ],
            borderWidth: 0,
            hoverOffset: 6,
          }],
        };
      }
    }
  });

  // ── Fetch ──────────────────────────────────────────────────────────────

  async function fetchAll(isRefresh = false) {
    if (isRefresh) refreshing = true;
    try {
      const [dash, costs, status] = await Promise.all([
        getDashboard(),
        getCostsDashboard(),
        getSystemStatus(),
      ]);
      data = dash;
      costsDash = costs;
      system = status;
      error = '';
    } catch (e: any) {
      error = e.message || 'Failed to load dashboard';
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  onMount(() => {
    fetchAll();
    intervalId = setInterval(() => fetchAll(true), 30000);
  });

  onDestroy(() => {
    if (intervalId) clearInterval(intervalId);
  });

  // ── Navigation ─────────────────────────────────────────────────────────

  function navigate(page: 'chat' | 'pea' | 'workflows' | 'watcher') {
    currentPage.set(page);
  }

  // ── Period breakdown helper ────────────────────────────────────────────

  interface PeriodEntry { label: string; cost: number; calls: number; cacheHits: number; saved: number }

  let periods = $derived<PeriodEntry[]>(costsDash ? [
    { label: 'Daily',   cost: costsDash.daily.total_cost,    calls: costsDash.daily.total_calls,    cacheHits: costsDash.daily.cache_hits,    saved: costsDash.daily.total_saved },
    { label: 'Weekly',  cost: costsDash.weekly.total_cost,   calls: costsDash.weekly.total_calls,   cacheHits: costsDash.weekly.cache_hits,   saved: costsDash.weekly.total_saved },
    { label: 'Monthly', cost: costsDash.monthly.total_cost,  calls: costsDash.monthly.total_calls,  cacheHits: costsDash.monthly.cache_hits,  saved: costsDash.monthly.total_saved },
    { label: 'All Time', cost: costsDash.all_time.total_cost, calls: costsDash.all_time.total_calls, cacheHits: costsDash.all_time.cache_hits, saved: costsDash.all_time.total_saved },
  ] : []);

  // ── Quick links config ─────────────────────────────────────────────────

  const quickLinks = [
    { icon: '\uD83D\uDCAC', title: 'Chat with Agent', desc: 'Ask questions, run tasks, build workflows', page: 'chat' as const },
    { icon: '\uD83E\uDD16', title: 'Manage PEAs',     desc: 'Configure autonomous agents and schedules', page: 'pea' as const },
    { icon: '\u26A1',       title: 'View Workflows',  desc: 'Monitor and manage active workflows', page: 'workflows' as const },
    { icon: '\uD83D\uDC41', title: 'Watcher Dashboard', desc: 'File monitoring, alerts, and triggers', page: 'watcher' as const },
  ];
</script>

<!-- ── Header ────────────────────────────────────────────────────────── -->
<div class="dash-header">
  <h1>
    Command Center
    {#if refreshing}<span class="refresh-dot"></span>{/if}
  </h1>
  {#if system}
    <span class="version-badge">v{system.version}</span>
  {/if}
</div>

{#if loading}
  <!-- Loading skeleton -->
  <div class="hero-grid">
    <Skeleton height="110px" />
    <Skeleton height="110px" />
    <Skeleton height="110px" />
    <Skeleton height="110px" />
  </div>
  <div class="section-skeleton">
    <Skeleton height="200px" />
  </div>
  <div class="hero-grid">
    <Skeleton height="90px" />
    <Skeleton height="90px" />
    <Skeleton height="90px" />
    <Skeleton height="90px" />
  </div>

{:else if error}
  <Card>
    <p class="error-text">{error}</p>
    <div style="margin-top: 0.75rem;">
      <Button variant="primary" onclick={() => fetchAll()}>Retry</Button>
    </div>
  </Card>

{:else if data && isEmpty(data)}
  <EmptyState
    icon="\uD83D\uDCCA"
    title="No activity yet"
    description="Start chatting with your agent to see stats here."
  >
    <Button variant="primary" onclick={() => navigate('chat')}>Open Chat</Button>
  </EmptyState>

{:else if data}
  <!-- ── Hero Stats ──────────────────────────────────────────────────── -->
  <section class="hero-grid">
    <div class="hero-card hero-saved">
      <div class="hero-label">Total Saved</div>
      <div class="hero-value saved-value">{formatUsd(saved)}</div>
      <div class="hero-sub">{savingsPercent.toFixed(0)}% free via cache</div>
    </div>

    <div class="hero-card">
      <div class="hero-label">Cache Hit Rate</div>
      <div class="hero-value cache-rate" class:cache-green={cachePercent > 80} class:cache-yellow={cachePercent > 50 && cachePercent <= 80} class:cache-red={cachePercent <= 50}>
        {cachePercent.toFixed(1)}%
      </div>
      <div class="hero-sub">{cacheHits.toLocaleString()} hits / {totalQueries.toLocaleString()} total</div>
    </div>

    <button class="hero-card hero-clickable" onclick={() => navigate('pea')}>
      <div class="hero-label">Active Agents</div>
      <div class="hero-value">{abilities.toLocaleString()}</div>
      <div class="hero-sub hero-link">View PEAs &rarr;</div>
    </button>

    <div class="hero-card">
      <div class="hero-label">Total Queries</div>
      <div class="hero-value">{totalQueries.toLocaleString()}</div>
      <div class="hero-sub">{llmCalls.toLocaleString()} LLM + {cacheHits.toLocaleString()} cached</div>
    </div>
  </section>

  <!-- ── Cost Analysis ───────────────────────────────────────────────── -->
  <section class="cost-section">
    <h2 class="section-title">Cost Analysis</h2>

    <div class="cost-row">
      <!-- Doughnut chart -->
      <div class="doughnut-wrap">
        {#if chartData}
          <ChartWrapper
            type="doughnut"
            data={chartData}
            height="180px"
            options={{
              cutout: '72%',
              plugins: {
                legend: { display: false },
                tooltip: {
                  callbacks: {
                    label: (ctx: any) => ` ${ctx.label}: $${ctx.parsed.toFixed(4)}`
                  }
                }
              },
              responsive: true,
              maintainAspectRatio: false,
            }}
          />
          <div class="doughnut-center">
            <span class="doughnut-pct">{savingsPercent.toFixed(0)}%</span>
            <span class="doughnut-lbl">saved</span>
          </div>
        {:else}
          <div class="no-chart-data">No cost data yet</div>
        {/if}
      </div>

      <!-- Summary + Legend -->
      <div class="cost-details">
        <p class="cost-sentence">
          Spent <strong class="spent-num">{formatUsd(spent)}</strong>, saved <strong class="saved-num">{formatUsd(saved)}</strong>
          <span class="free-tag">({savingsPercent.toFixed(0)}% free via cache)</span>
        </p>
        <div class="chart-legend">
          <span class="legend-item legend-spent">Spent</span>
          <span class="legend-item legend-saved">Saved</span>
        </div>
        <div class="token-info">
          <span>{(data.costs.total_input_tokens ?? 0).toLocaleString()} input tokens</span>
          <span class="token-sep">/</span>
          <span>{(data.costs.total_output_tokens ?? 0).toLocaleString()} output tokens</span>
        </div>
      </div>
    </div>

    <!-- Period breakdown cards -->
    {#if periods.length > 0}
      <div class="period-grid">
        {#each periods as p}
          <div class="period-card">
            <div class="period-label">{p.label}</div>
            <div class="period-row">
              <div class="period-stat">
                <span class="period-val spent-num">{formatUsd(p.cost)}</span>
                <span class="period-key">spent</span>
              </div>
              <div class="period-stat">
                <span class="period-val saved-num">{formatUsd(p.saved)}</span>
                <span class="period-key">saved</span>
              </div>
            </div>
            <div class="period-meta">
              {p.calls.toLocaleString()} calls &middot; {p.cacheHits.toLocaleString()} cached
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <!-- ── Quick Links ─────────────────────────────────────────────────── -->
  <section class="quick-section">
    <h2 class="section-title">Quick Actions</h2>
    <div class="quick-grid">
      {#each quickLinks as link}
        <button class="quick-card" onclick={() => navigate(link.page)}>
          <span class="quick-icon">{link.icon}</span>
          <span class="quick-title">{link.title}</span>
          <span class="quick-desc">{link.desc}</span>
        </button>
      {/each}
    </div>
  </section>

  <!-- ── System Status Footer ────────────────────────────────────────── -->
  {#if system}
    <footer class="sys-footer">
      <div class="sys-item">
        <span class="sys-dot sys-dot-on"></span>
        NabaOS v{system.version}
      </div>
      <div class="sys-item">
        Uptime: <strong>{formatUptime(system.uptime_secs)}</strong>
      </div>
      <div class="sys-item">
        Channels: <strong>{system.channels.length > 0 ? system.channels.join(', ') : 'none'}</strong>
      </div>
      <div class="sys-item">
        Watcher:
        {#if system.watcher_enabled}
          <span class="sys-dot sys-dot-on"></span> Active
          {#if system.watcher_alerts > 0}
            <span class="sys-alert-count">{system.watcher_alerts} alert{system.watcher_alerts === 1 ? '' : 's'}</span>
          {/if}
        {:else}
          <span class="sys-dot sys-dot-off"></span> Off
        {/if}
      </div>
      <div class="sys-item">
        {workflows} workflow{workflows === 1 ? '' : 's'} &middot; {data.total_scheduled_jobs} scheduled job{data.total_scheduled_jobs === 1 ? '' : 's'}
      </div>
    </footer>
  {/if}
{/if}

<style>
  /* ── Header ───────────────────────────────────────────────────────── */
  .dash-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 1.5rem;
  }
  .dash-header h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 700;
    color: var(--text);
  }
  .version-badge {
    font-size: 0.72rem;
    padding: 0.2rem 0.55rem;
    border-radius: 20px;
    background: var(--bg-elevated, var(--surface));
    color: var(--text-dim);
    border: 1px solid var(--border);
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
  }
  .refresh-dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    background: var(--accent);
    border-radius: 50%;
    margin-left: 8px;
    animation: pulse 1s ease infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  /* ── Hero Stats Grid ──────────────────────────────────────────────── */
  .hero-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1rem;
    margin-bottom: 1.75rem;
  }
  @media (max-width: 900px) {
    .hero-grid { grid-template-columns: repeat(2, 1fr); }
  }
  @media (max-width: 500px) {
    .hero-grid { grid-template-columns: 1fr; }
  }

  .hero-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 12px);
    padding: 1.25rem 1rem;
    text-align: center;
    box-shadow: var(--shadow-sm);
    transition: transform 0.15s ease, box-shadow 0.15s ease;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .hero-card:hover {
    transform: translateY(-1px);
    box-shadow: var(--shadow-md, 0 4px 12px rgba(0,0,0,0.15));
  }
  .hero-saved {
    border-color: var(--success);
    border-width: 1.5px;
  }
  .hero-clickable {
    cursor: pointer;
    font: inherit;
    color: inherit;
  }
  .hero-clickable:hover {
    border-color: var(--accent);
  }
  .hero-label {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    font-weight: 500;
  }
  .hero-value {
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    font-size: 2rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    color: var(--text);
    line-height: 1.2;
  }
  .saved-value {
    color: var(--success) !important;
  }
  .cache-green { color: var(--success) !important; }
  .cache-yellow { color: var(--warning) !important; }
  .cache-red { color: var(--danger) !important; }
  .hero-sub {
    font-size: 0.78rem;
    color: var(--text-dim);
    margin-top: 0.15rem;
  }
  .hero-link {
    color: var(--accent);
    font-weight: 500;
  }

  /* ── Section titles ───────────────────────────────────────────────── */
  .section-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text);
    margin: 0 0 1rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* ── Cost Analysis ────────────────────────────────────────────────── */
  .cost-section {
    margin-bottom: 1.75rem;
  }
  .cost-row {
    display: flex;
    align-items: center;
    gap: 2rem;
    margin-bottom: 1.5rem;
    flex-wrap: wrap;
  }
  .doughnut-wrap {
    width: 180px;
    height: 180px;
    flex-shrink: 0;
    position: relative;
  }
  .doughnut-center {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    text-align: center;
    pointer-events: none;
  }
  .doughnut-pct {
    display: block;
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    font-size: 1.4rem;
    font-weight: 700;
    color: var(--success);
  }
  .doughnut-lbl {
    display: block;
    font-size: 0.7rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .no-chart-data {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    font-size: 0.85rem;
    border: 1px dashed var(--border);
    border-radius: 50%;
  }
  .cost-details {
    flex: 1;
    min-width: 240px;
  }
  .cost-sentence {
    font-size: 1.05rem;
    color: var(--text);
    margin: 0 0 0.75rem;
    line-height: 1.6;
  }
  .spent-num {
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    color: var(--warning);
    font-weight: 700;
  }
  .saved-num {
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    color: var(--success);
    font-weight: 700;
  }
  .free-tag {
    color: var(--success);
    font-weight: 500;
  }
  .chart-legend {
    display: flex;
    gap: 1.25rem;
    margin-bottom: 0.75rem;
  }
  .legend-item {
    font-size: 0.82rem;
    color: var(--text-dim);
  }
  .legend-item::before {
    content: '';
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    margin-right: 0.4rem;
    vertical-align: middle;
  }
  .legend-spent::before { background: var(--warning); }
  .legend-saved::before { background: var(--success); }
  .token-info {
    font-size: 0.8rem;
    color: var(--text-dim);
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
  }
  .token-sep {
    margin: 0 0.35rem;
    opacity: 0.5;
  }

  /* ── Period breakdown ─────────────────────────────────────────────── */
  .period-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1rem;
  }
  @media (max-width: 900px) {
    .period-grid { grid-template-columns: repeat(2, 1fr); }
  }
  @media (max-width: 500px) {
    .period-grid { grid-template-columns: 1fr; }
  }
  .period-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 12px);
    padding: 1rem;
    box-shadow: var(--shadow-sm);
  }
  .period-label {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    font-weight: 600;
    margin-bottom: 0.6rem;
  }
  .period-row {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .period-stat {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .period-val {
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    font-size: 1.1rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .period-key {
    font-size: 0.68rem;
    text-transform: uppercase;
    color: var(--text-dim);
    letter-spacing: 0.04em;
  }
  .period-meta {
    font-size: 0.75rem;
    color: var(--text-dim);
    border-top: 1px solid var(--border);
    padding-top: 0.45rem;
  }

  /* ── Quick Actions ────────────────────────────────────────────────── */
  .quick-section {
    margin-bottom: 1.75rem;
  }
  .quick-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1rem;
  }
  @media (max-width: 900px) {
    .quick-grid { grid-template-columns: repeat(2, 1fr); }
  }
  @media (max-width: 500px) {
    .quick-grid { grid-template-columns: 1fr; }
  }
  .quick-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.35rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 12px);
    padding: 1.15rem 1rem;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
    box-shadow: var(--shadow-sm);
    font: inherit;
    color: inherit;
    text-align: left;
  }
  .quick-card:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow-md, 0 4px 12px rgba(0,0,0,0.15));
    border-color: var(--accent);
  }
  .quick-icon {
    font-size: 1.5rem;
    line-height: 1;
  }
  .quick-title {
    font-weight: 600;
    font-size: 0.92rem;
    color: var(--text);
  }
  .quick-desc {
    font-size: 0.78rem;
    color: var(--text-dim);
    line-height: 1.4;
  }

  /* ── System Status Footer ─────────────────────────────────────────── */
  .sys-footer {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem 1.5rem;
    padding: 1rem 0;
    border-top: 1px solid var(--border);
    font-size: 0.78rem;
    color: var(--text-dim);
  }
  .sys-item {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .sys-item strong {
    color: var(--text);
    font-weight: 600;
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
  }
  .sys-dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .sys-dot-on {
    background: var(--success);
    box-shadow: 0 0 4px var(--success);
  }
  .sys-dot-off {
    background: var(--text-dim);
    opacity: 0.5;
  }
  .sys-alert-count {
    background: var(--danger);
    color: #fff;
    padding: 0.1rem 0.45rem;
    border-radius: 10px;
    font-size: 0.68rem;
    font-weight: 600;
    margin-left: 0.25rem;
  }

  /* ── Utility ──────────────────────────────────────────────────────── */
  .error-text {
    color: var(--danger);
    font-weight: 500;
  }
  .section-skeleton {
    margin-bottom: 1.75rem;
  }
</style>
