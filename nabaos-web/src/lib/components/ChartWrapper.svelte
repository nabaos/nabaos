<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Chart, registerables } from 'chart.js';

  Chart.register(...registerables);

  interface Props {
    type: 'line' | 'bar' | 'doughnut';
    data: any;
    options?: any;
    height?: string;
  }
  let { type, data, options = {}, height = '240px' }: Props = $props();

  let canvas: HTMLCanvasElement;
  let chart: Chart | null = null;

  function getThemeColors() {
    const style = getComputedStyle(document.documentElement);
    return {
      text: style.getPropertyValue('--text-dim').trim(),
      border: style.getPropertyValue('--border').trim(),
      accent: style.getPropertyValue('--accent').trim(),
      success: style.getPropertyValue('--success').trim(),
      warning: style.getPropertyValue('--warning').trim(),
      danger: style.getPropertyValue('--danger').trim(),
    };
  }

  // Clone data to strip Svelte 5 $state proxy — Chart.js uses Object.defineProperty
  // on data objects, which throws state_descriptors_fixed on proxied state.
  function cloneData(d: any): any {
    return JSON.parse(JSON.stringify(d));
  }

  onMount(() => {
    const colors = getThemeColors();
    const defaultOptions: any = {
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { labels: { color: colors.text, font: { size: 12 } } },
      },
    };
    if (type !== 'doughnut') {
      defaultOptions.scales = {
        x: { ticks: { color: colors.text }, grid: { color: colors.border + '40' } },
        y: { ticks: { color: colors.text }, grid: { color: colors.border + '40' } },
      };
    }
    chart = new Chart(canvas, {
      type,
      data: cloneData(data),
      options: { ...defaultOptions, ...options },
    });
  });

  onDestroy(() => { chart?.destroy(); });

  $effect(() => {
    if (chart && data) {
      chart.data = cloneData(data);
      chart.update();
    }
  });
</script>

<div class="chart-container" style="height: {height};">
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .chart-container { position: relative; width: 100%; }
</style>
