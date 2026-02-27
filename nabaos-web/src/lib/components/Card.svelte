<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props { title?: string; hoverable?: boolean; children: Snippet; }
  let { title, hoverable = false, children }: Props = $props();
</script>

<div class="card" class:hoverable>
  {#if title}
    <div class="card-header">{title}</div>
  {/if}
  {@render children()}
</div>

<style>
  .card { background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-md); padding: 1.25rem; box-shadow: var(--shadow-sm); }
  .hoverable { transition: transform var(--transition-fast), box-shadow var(--transition-fast); cursor: pointer; }
  .hoverable:hover { transform: translateY(-2px); box-shadow: var(--shadow-md); }
  .card-header { font-weight: 600; margin-bottom: 0.75rem; font-size: 0.95rem; }
</style>
