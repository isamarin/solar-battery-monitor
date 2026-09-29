<script lang="ts">
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { onMount } from "svelte";

  export interface Series {
    label: string;
    /** CSS custom property holding the color, e.g. "--s1" */
    color: string;
  }

  interface Props {
    title: string;
    unit: string;
    decimals: number;
    series: Series[];
    /** unix seconds */
    x: number[];
    ys: number[][];
    zeroLine?: boolean;
    yRange?: [number, number];
  }

  let { title, unit, decimals, series, x, ys, zeroLine = false, yRange }: Props = $props();

  let el: HTMLDivElement;
  let plot: uPlot | undefined;
  let idx = $state<number | null>(null);

  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  // values at cursor, or the latest ones
  const shown = $derived.by(() => {
    const i = idx ?? x.length - 1;
    return series.map((s, k) => ({ ...s, value: ys[k]?.[i] }));
  });
  const at = $derived.by(() => {
    const i = idx ?? x.length - 1;
    return idx === null || x[i] === undefined ? null : new Date(x[i] * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  });

  function build() {
    plot?.destroy();
    const axis = { stroke: css("--muted"), grid: { stroke: css("--line"), width: 1 }, ticks: { show: false }, font: "10px -apple-system, sans-serif", size: 26 };
    plot = new uPlot(
      {
        width: el.clientWidth,
        height: 92,
        legend: { show: false },
        padding: [6, 4, 0, 0],
        cursor: { sync: { key: "bms" }, points: { size: 7, width: 2, fill: css("--card") }, drag: { x: false, y: false } },
        scales: { x: { time: true }, y: yRange ? { range: yRange } : { range: (_u, lo, hi) => pad(lo, hi) } },
        axes: [
          { ...axis, space: 60 },
          { ...axis, size: 34, values: (_u, vals) => vals.map((v) => +v.toFixed(decimals > 1 ? 2 : decimals)) },
        ],
        series: [
          {},
          ...series.map((s) => ({ label: s.label, stroke: css(s.color), width: 2, points: { show: false } })),
        ],
        hooks: {
          setCursor: [(u) => (idx = u.cursor.idx ?? null)],
          draw: zeroLine
            ? [
                (u) => {
                  const y = u.valToPos(0, "y", true);
                  if (y < u.bbox.top || y > u.bbox.top + u.bbox.height) return;
                  const c = u.ctx;
                  c.save();
                  c.strokeStyle = css("--muted");
                  c.lineWidth = 1;
                  c.setLineDash([3, 3]);
                  c.beginPath();
                  c.moveTo(u.bbox.left, y);
                  c.lineTo(u.bbox.left + u.bbox.width, y);
                  c.stroke();
                  c.restore();
                },
              ]
            : [],
        },
      },
      [x, ...ys],
      el,
    );
  }

  // keep a flat line visible and give the data some headroom
  function pad(lo: number, hi: number): [number, number] {
    const span = hi - lo || Math.abs(hi) * 0.02 || 1;
    return [lo - span * 0.15, hi + span * 0.15];
  }

  onMount(() => {
    build();
    const ro = new ResizeObserver(() => plot?.setSize({ width: el.clientWidth, height: 92 }));
    ro.observe(el);
    const mq = matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", build);
    return () => {
      ro.disconnect();
      mq.removeEventListener("change", build);
      plot?.destroy();
    };
  });

  $effect(() => {
    plot?.setData([x, ...ys]);
  });
</script>

<section class="chart">
  <div class="head">
    <span class="title">{title}</span>
    <span class="vals">
      {#each shown as s}
        <span class="val">
          {#if series.length > 1}<i style="background: var({s.color})"></i><span class="lab">{s.label}</span>{/if}
          <b>{s.value === undefined || s.value === null ? "—" : s.value.toFixed(decimals)}</b>
        </span>
      {/each}
      <span class="unit">{unit}</span>
    </span>
  </div>
  <div class="plot" bind:this={el}></div>
  <div class="time">{at ?? ""}</div>
</section>

<style>
  .chart { padding: 8px 8px 2px 10px; position: relative; background: var(--card); border: 1px solid var(--line); border-radius: 12px; }
  .head { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
  .title { font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); white-space: nowrap; }
  .vals { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: baseline; gap: 2px 8px; font-variant-numeric: tabular-nums; }
  .val { display: inline-flex; align-items: baseline; gap: 3px; }
  .val i { width: 10px; height: 2px; border-radius: 1px; align-self: center; }
  .lab { font-size: 10px; color: var(--muted); }
  .val b { font-size: 12px; font-weight: 600; color: var(--text); }
  .unit { font-size: 10px; color: var(--muted); }
  .plot { width: 100%; }
  .time { position: absolute; top: 24px; left: 48px; font-size: 10px; color: var(--muted); pointer-events: none; }
</style>
