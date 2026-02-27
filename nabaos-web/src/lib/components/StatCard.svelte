<script lang="ts">
  interface Props {
    value: string | number;
    label: string;
    trend?: 'up' | 'down' | 'flat';
    color?: 'default' | 'success' | 'warning' | 'danger' | 'accent';
  }
  let { value, label, trend, color = 'default' }: Props = $props();
</script>

<div class="stat-card fade-in">
  <div class="stat-value" class:color-success={color === 'success'} class:color-warning={color === 'warning'} class:color-danger={color === 'danger'} class:color-accent={color === 'accent'}>
    {value}
  </div>
  <div class="stat-label">
    {label}
    {#if trend === 'up'}<span class="trend-up">↑</span>{/if}
    {#if trend === 'down'}<span class="trend-down">↓</span>{/if}
  </div>
</div>

<style>
  .stat-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 1.25rem; text-align: center; box-shadow: var(--shadow-sm); }
  .stat-value { font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace; font-size: 1.8rem; font-weight: 700; font-variant-numeric: tabular-nums; color: var(--text); }
  .color-success { color: var(--success) !important; }
  .color-warning { color: var(--warning) !important; }
  .color-danger { color: var(--danger) !important; }
  .color-accent { color: var(--accent) !important; }
  .stat-label { font-size: 0.78rem; color: var(--text-dim); margin-top: 0.4rem; text-transform: uppercase; letter-spacing: 0.05em; }
  .trend-up { color: var(--success); margin-left: 4px; }
  .trend-down { color: var(--danger); margin-left: 4px; }
</style>
