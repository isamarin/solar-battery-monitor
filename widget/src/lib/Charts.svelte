<script lang="ts">
  import Chart from "./Chart.svelte";
  import type { Sample } from "./bms";

  let { samples }: { samples: Sample[] } = $props();

  const RANGES = [
    { label: "15m", s: 15 * 60 },
    { label: "1h", s: 3600 },
    { label: "6h", s: 6 * 3600 },
    { label: "24h", s: 24 * 3600 },
  ];
  let range = $state(3600);

  const slice = $derived.by(() => {
    const from = Date.now() / 1000 - range;
    return samples.filter((s) => s.t / 1000 >= from);
  });
  const x = $derived(slice.map((s) => s.t / 1000));
  const cellCount = $derived(Math.max(0, ...slice.map((s) => s.cells.length)));
  const tempCount = $derived(Math.max(0, ...slice.map((s) => s.temps.length)));
  // categorical slots in fixed order, never cycled (max 4 cells on this board)
  const slot = (i: number) => `--s${(i % 4) + 1}`;
</script>

<div class="filters" role="radiogroup" aria-label="Time range">
  {#each RANGES as r}
    <button role="radio" aria-checked={range === r.s} class:on={range === r.s} onclick={() => (range = r.s)}>{r.label}</button>
  {/each}
  <span class="count">{slice.length} pts · every 5 s</span>
</div>

{#if slice.length < 2}
  <section class="empty">Collecting data… charts appear after a few samples.</section>
{:else}
  <Chart title="State of charge" unit="%" decimals={0} yRange={[0, 100]} series={[{ label: "SOC", color: "--s1" }]} {x} ys={[slice.map((s) => s.soc)]} />
  <Chart title="Power · + charge / − discharge" unit="W" decimals={1} zeroLine series={[{ label: "Power", color: "--s1" }]} {x} ys={[slice.map((s) => s.v * s.i)]} />
  <Chart
    title="Cell voltage"
    unit="V"
    decimals={3}
    series={Array.from({ length: cellCount }, (_, i) => ({ label: `${i + 1}`, color: slot(i) }))}
    {x}
    ys={Array.from({ length: cellCount }, (_, i) => slice.map((s) => s.cells[i]))}
  />
  <Chart title="Cell imbalance Δ" unit="mV" decimals={0} series={[{ label: "Δ", color: "--s1" }]} {x} ys={[slice.map((s) => (s.cells.length ? Math.round((Math.max(...s.cells) - Math.min(...s.cells)) * 1000) : 0))]} />
  <Chart
    title="Temperature"
    unit="°C"
    decimals={1}
    series={Array.from({ length: tempCount }, (_, i) => ({ label: `T${i + 1}`, color: slot(i) }))}
    {x}
    ys={Array.from({ length: tempCount }, (_, i) => slice.map((s) => s.temps[i]))}
  />
{/if}

<style>
  .filters { display: flex; align-items: center; gap: 4px; }
  .filters button {
    border: 1px solid var(--line);
    background: var(--card);
    color: var(--muted);
    font: 600 11px -apple-system, sans-serif;
    padding: 3px 9px;
    border-radius: 6px;
  }
  .filters button.on { background: var(--text); color: var(--bg); border-color: transparent; }
  .count { margin-left: auto; font-size: 10px; color: var(--muted); }
  .empty { text-align: center; color: var(--muted); padding: 30px 12px; }
</style>
