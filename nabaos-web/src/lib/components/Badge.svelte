<script lang="ts">
  import type { Snippet } from 'svelte';
  interface Props { variant?: 'success' | 'warning' | 'danger' | 'info' | 'neutral'; children: Snippet; }
  let { variant = 'info', children }: Props = $props();
</script>

<span class="badge badge-{variant}">{@render children()}</span>
