<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getDashboard, getCosts, type DashboardData, type CostData } from '../lib/api';
  import { StatCard, Card, ChartWrapper, Skeleton, EmptyState } from '../lib/components';

  let data = $state<DashboardData | null>(null);
  let costs = $state<CostData | null>(null);
  let error = $state('');
  let loading = $state(true);
  let refreshing = $state(false);
  let chartData = $state<any>(null);
  let intervalId: ReturnType<typeof setInterval> | null = null;

  type Period = 'all' | '24h' | '7d';
  let period = $state<Period>('all');

  const PERIOD_MS: Record<Period, number | undefined> = {
    all: undefined,
    '24h': 24 * 60 * 60 * 1000,
    '7d': 7 * 24 * 60 * 60 * 1000,
  };

  function isEmpty(d: DashboardData): boolean {
    return d.total_cache_hits === 0
      && d.total_llm_calls === 0
      && d.total_spent_usd === 0
      && d.total_saved_usd === 0
      && d.workflow_count === 0
      && d.scheduled_jobs === 0
      && d.ability_count === 0;
  }

  /* Derived helpers */
  let workflowCount = $derived(data?.workflow_count ?? 0);
  let spent = $derived(costs?.total_spent_usd ?? 0);
  let saved = $derived(costs?.total_saved_usd ?? 0);
  let cacheHits = $derived(costs?.total_cache_hits ?? 0);
  let llmCalls = $derived(costs?.total_llm_calls ?? 0);
  let totalQueries = $derived(cacheHits + llmCalls);
  let cachePercent = $derived(totalQueries > 0 ? ((cacheHits / totalQueries) * 100) : 0);
  let freePercent = $derived(spent + saved > 0 ? ((saved / (spent + saved)) * 100) : 0);

  let statusLine = $derived(
    data && !isEmpty(data)
      ? workflowCount > 0
        ? `All good. ${workflowCount} workflow${workflowCount === 1 ? '' : 's'} running.`
        : 'Idle. Start chatting to build workflows.'
      : ''
  );

  async function fetchAll(isRefresh = false) {
    if (isRefresh) refreshing = true;
    try {
      const [dash, cost] = await Promise.all([
        getDashboard(),
        getCosts(PERIOD_MS[period]),
      ]);
      data = dash;
      costs = cost;
      error = '';
    } catch (e: any) {
      error = e.message || 'Failed to load dashboard';
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  $effect(() => {
    if (costs) {
      chartData = {
        labels: ['Spent', 'Saved'],
        datasets: [{
          data: [costs.total_spent_usd, costs.total_saved_usd],
          backgroundColor: [
            getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(),
            getComputedStyle(document.documentElement).getPropertyValue('--success').trim(),
          ],
          borderWidth: 0,
        }]
      };
    }
  });

  /* Re-fetch when period changes (skip initial) */
  let mounted = false;
  $effect(() => {
    // subscribe to period
    void period;
    if (mounted) fetchAll(true);
  });

  onMount(() => {
    fetchAll();
    mounted = true;
    intervalId = setInterval(() => fetchAll(true), 30000);
  });

  onDestroy(() => {
    if (intervalId) clearInterval(intervalId);
  });

  function selectPeriod(p: Period) {
    period = p;
  }
</script>

<h1>
  Dashboard
  {#if refreshing}<span class="refresh-dot"></span>{/if}
</h1>

{#if loading}
  <div class="stats-grid">
    <Skeleton height="80px" />
    <Skeleton height="80px" />
    <Skeleton height="80px" />
  </div>
  <div class="chart-skeleton">
    <Skeleton height="180px" />
  </div>
{:else if error}
  <Card>
    <p class="error-text">{error}</p>
  </Card>
{:else if data && isEmpty(data)}
  <EmptyState icon="📊" title="No activity yet" description="Start chatting with your agent to see stats here." />
{:else if data && costs}
  <!-- Status line -->
  <p class="status-line">{statusLine}</p>

  <!-- Contextualized cost summary -->
  <section class="cost-summary">
    <div class="period-selector">
      <button class:active={period === 'all'} onclick={() => selectPeriod('all')}>All Time</button>
      <button class:active={period === '24h'} onclick={() => selectPeriod('24h')}>24h</button>
      <button class:active={period === '7d'} onclick={() => selectPeriod('7d')}>7 Days</button>
    </div>

    <p class="cost-sentence">
      Spent <strong>${spent.toFixed(4)}</strong> today, saved <strong>${saved.toFixed(4)}</strong>
      <span class="free-tag">({freePercent.toFixed(0)}% free via cache)</span>
    </p>
    <p class="cache-sentence">
      Cache: {cacheHits.toLocaleString()}/{totalQueries.toLocaleString()} queries handled locally
    </p>

    <!-- Inline doughnut chart -->
    <div class="inline-chart-row">
      <div class="inline-chart">
        {#if chartData}
          <ChartWrapper
            type="doughnut"
            data={chartData}
            options={{ cutout: '70%', plugins: { legend: { display: false } }, responsive: true, maintainAspectRatio: true }}
          />
        {/if}
      </div>
      <div class="chart-legend">
        <span class="legend-spent">Spent</span>
        <span class="legend-saved">Saved</span>
      </div>
    </div>
  </section>

  <!-- Simplified stat cards -->
  <div class="stats-grid">
    <StatCard value={cachePercent.toFixed(1) + '%'} label="Cache Hit Rate" color="success" />
    <StatCard value={'$' + spent.toFixed(4)} label="Total Spent" color="warning" />
    <StatCard value={'$' + saved.toFixed(4)} label="Total Saved" color="success" />
  </div>

  <p class="quick-info">
    {workflowCount} workflow{workflowCount === 1 ? '' : 's'} &middot; {data.scheduled_jobs} scheduled jobs &middot; {data.ability_count} abilities
  </p>
{/if}

<style>
  .status-line {
    font-size: 1.15rem;
    font-weight: 600;
    color: var(--text);
    margin: 0.5rem 0 1.25rem;
  }

  .cost-summary {
    margin-bottom: 1.5rem;
  }

  .period-selector {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .period-selector button {
    padding: 0.35rem 0.9rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: var(--text-dim);
    font-size: 0.82rem;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .period-selector button.active {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .cost-sentence {
    font-size: 1rem;
    color: var(--text);
    margin: 0 0 0.25rem;
    line-height: 1.5;
  }

  .cost-sentence strong {
    font-weight: 700;
  }

  .free-tag {
    color: var(--success);
    font-weight: 500;
  }

  .cache-sentence {
    font-size: 0.88rem;
    color: var(--text-dim);
    margin: 0 0 1rem;
  }

  .inline-chart-row {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .inline-chart {
    width: 120px;
    height: 120px;
    flex-shrink: 0;
  }

  .chart-legend {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    font-size: 0.82rem;
    color: var(--text-dim);
  }

  .legend-spent::before,
  .legend-saved::before {
    content: '';
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 2px;
    margin-right: 0.4rem;
    vertical-align: middle;
  }

  .legend-spent::before {
    background: var(--accent);
  }

  .legend-saved::before {
    background: var(--success);
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 1rem;
    margin-top: 1.25rem;
  }

  .chart-skeleton {
    margin-top: 1.25rem;
  }

  .quick-info {
    text-align: center;
    color: var(--text-dim);
    font-size: 0.85rem;
    margin-top: 1.25rem;
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
</style>
