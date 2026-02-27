<script lang="ts">
  interface Props { width?: string; height?: string; rounded?: boolean; }
  let { width = '100%', height = '20px', rounded = false }: Props = $props();
</script>

<div class="skeleton" class:rounded style="width: {width}; height: {height};" />

<style>
  .skeleton { background: linear-gradient(90deg, var(--bg-hover) 25%, var(--bg-elevated) 50%, var(--bg-hover) 75%); background-size: 200% 100%; animation: shimmer 1.5s ease-in-out infinite; border-radius: var(--radius-sm); }
  .rounded { border-radius: 50%; }
  @keyframes shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
</style>
