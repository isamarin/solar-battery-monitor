<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { CELL_MAX, CELL_MIN, type Sample, type Snapshot } from "$lib/bms";
  import Charts from "$lib/Charts.svelte";

  let snap = $state<Snapshot>({ status: "", error: null, device: null, model: null, basic: null, cells: [], updated_at: 0 });
  let now = $state(Date.now());
  let pinned = $state(true);
  let tab = $state<"now" | "charts">("now");
  let samples = $state<Sample[]>([]);

  onMount(() => {
    invoke<Snapshot>("snapshot").then((s) => (snap = s));
    invoke<Sample[]>("history").then((h) => (samples = h));
    const unlisten = listen<Snapshot>("bms", (e) => (snap = e.payload));
    const unlistenSample = listen<Sample>("sample", (e) => {
      const cutoff = Date.now() - 24 * 3600 * 1000;
      samples = [...samples.filter((s) => s.t >= cutoff), e.payload];
    });
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => {
      unlisten.then((f) => f());
      unlistenSample.then((f) => f());
      clearInterval(tick);
    };
  });

  async function togglePin() {
    pinned = !pinned;
    await getCurrentWindow().setAlwaysOnTop(pinned);
  }

  const b = $derived(snap.basic);
  const age = $derived(snap.updated_at ? Math.round((now - snap.updated_at) / 1000) : null);
  const stale = $derived(age !== null && age > 10);
  const power = $derived(b ? b.voltage * b.current : 0);
  const flow = $derived(!b || Math.abs(b.current) < 0.05 ? "idle" : b.current > 0 ? "charging" : "discharging");
  const cellMin = $derived(snap.cells.length ? Math.min(...snap.cells) : 0);
  const cellMax = $derived(snap.cells.length ? Math.max(...snap.cells) : 0);
  const cellAvg = $derived(snap.cells.length ? snap.cells.reduce((a, c) => a + c, 0) / snap.cells.length : 0);

  const statusText = $derived(
    stale ? `no data for ${age}s` : ({ scanning: "searching…", connecting: "connecting…", connected: "live", error: "retrying…", "": "starting…" })[snap.status]
  );

  // SOC ring geometry
  const R = 44;
  const C = 2 * Math.PI * R;

  const fmt = (v: number, d = 2) => v.toFixed(d);
  const fill = (v: number) => Math.max(0, Math.min(1, (v - CELL_MIN) / (CELL_MAX - CELL_MIN))) * 100;
</script>

<main>
  <header data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>
      <span class="name" data-tauri-drag-region>{snap.device ?? "JBD BMS"}</span>
      {#if snap.model}<span class="model" data-tauri-drag-region>{snap.model}</span>{/if}
    </div>
    <span class="status {stale ? 'error' : snap.status}" title={snap.error ?? ""}><i></i>{statusText}</span>
    <button class="pin" class:on={pinned} onclick={togglePin} title={pinned ? "Always on top: on" : "Always on top: off"} aria-label="Toggle always on top">
      <svg viewBox="0 0 16 16" width="13" height="13"><path d="M9.5 1.5l5 5-2 .5-2.5 2.5.5 3-1.5 1.5-3-3-3.5 3.5h-1v-1L5 10 2 7l1.5-1.5 3 .5L9 3.5z" fill="currentColor" /></svg>
    </button>
  </header>

  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={tab === "now"} class:on={tab === "now"} onclick={() => (tab = "now")}>Now</button>
    <button role="tab" aria-selected={tab === "charts"} class:on={tab === "charts"} onclick={() => (tab = "charts")}>Charts</button>
  </div>

  {#if tab === "charts"}
    <Charts {samples} />
  {:else if !b}
    <section class="empty">
      <div class="spinner" class:off={snap.status === "error"}></div>
      <p>{snap.status === "error" ? snap.error : statusText}</p>
      {#if snap.status === "error"}<p class="hint">Close the Xiaoxiang / JBD app on your phone — the BMS accepts one connection at a time.</p>{/if}
    </section>
  {:else}
    <section class="hero" class:stale>
      <svg class="ring" viewBox="0 0 100 100" role="img" aria-label="State of charge {b.soc}%">
        <circle cx="50" cy="50" r={R} class="track" />
        <circle cx="50" cy="50" r={R} class="arc" class:low={b.soc <= 15} stroke-dasharray="{(C * b.soc) / 100} {C}" transform="rotate(-90 50 50)" />
        <text x="50" y="50" class="soc">{b.soc}<tspan class="pct">%</tspan></text>
        <text x="50" y="66" class="socl">{fmt(b.remaining_ah, 1)} / {fmt(b.nominal_ah, 0)} Ah</text>
      </svg>
      <div class="big">
        <div><span class="v">{fmt(b.voltage)}</span><span class="u">V</span></div>
        <div class="flow {flow}"><span class="v">{b.current > 0 ? "+" : ""}{fmt(b.current)}</span><span class="u">A</span></div>
        <div class="w"><span class="v">{fmt(power, 1)}</span><span class="u">W</span></div>
        <div class="flowtag {flow}">{flow}</div>
      </div>
    </section>

    <section class="chips">
      <span class="chip" class:on={b.charge_fet}>CHG {b.charge_fet ? "ON" : "OFF"}</span>
      <span class="chip" class:on={b.discharge_fet}>DSG {b.discharge_fet ? "ON" : "OFF"}</span>
      {#if b.balancing}<span class="chip bal">balancing</span>{/if}
    </section>

    <section class="alerts">
      {#if b.protections.length}
        {#each b.protections as p}<span class="alert">{p}</span>{/each}
      {:else}
        <span class="ok">No active protections</span>
      {/if}
    </section>

    <section class="tiles">
      <div><span class="lbl">Temp</span><b>{b.temps.map((t) => fmt(t, 1)).join(" / ") || "—"}<small>°C</small></b></div>
      <div><span class="lbl">Cycles</span><b>{b.cycles}</b></div>
      <div><span class="lbl">Cell Δ</span><b class:warn={cellMax - cellMin > 0.05}>{Math.round((cellMax - cellMin) * 1000)}<small>mV</small></b></div>
    </section>

    <section class="cells">
      <div class="cellhead"><span>Cells · {b.cell_count}S</span><span>avg {fmt(cellAvg, 3)} V</span></div>
      {#each snap.cells as v, i}
        <div class="cell" class:max={v === cellMax && cellMax !== cellMin} class:min={v === cellMin && cellMax !== cellMin}>
          <span class="idx">{i + 1}</span>
          <span class="bar"><span style="width:{fill(v)}%"></span></span>
          <span class="cv">{fmt(v, 3)}</span>
          <span class="dot" class:on={(b.balancing >> i) & 1} title="balancing"></span>
        </div>
      {/each}
    </section>

    <footer>
      <span>fw {b.sw_version}</span>
      <span>{b.production_date}</span>
      <span>{age === null ? "" : age < 2 ? "just now" : `${age}s ago`}</span>
    </footer>
  {/if}
</main>

<style>
  :global(:root) {
    --bg: #f5f5f7;
    --card: #ffffff;
    --line: rgba(0, 0, 0, 0.08);
    --text: #1d1d1f;
    --muted: #6e6e73;
    --green: #1f9d55;
    --amber: #c77700;
    --red: #d70015;
    --blue: #0a66d6;
    --track: rgba(0, 0, 0, 0.07);
    /* categorical chart slots (dataviz reference palette, fixed order) */
    --s1: #2a78d6;
    --s2: #eb6834;
    --s3: #1baf7a;
    --s4: #eda100;
    color-scheme: light dark;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1c1c1e;
      --card: #2c2c2e;
      --line: rgba(255, 255, 255, 0.08);
      --text: #f5f5f7;
      --muted: #98989d;
      --green: #32d74b;
      --amber: #ffb340;
      --red: #ff453a;
      --blue: #4aa3ff;
      --track: rgba(255, 255, 255, 0.1);
      --s1: #3987e5;
      --s2: #d95926;
      --s3: #199e70;
      --s4: #c98500;
    }
  }
  :global(html, body) {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font: 13px/1.35 -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    -webkit-font-smoothing: antialiased;
    user-select: none;
    cursor: default;
  }
  main {
    padding: 0 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 100vh;
    box-sizing: border-box;
    font-variant-numeric: tabular-nums;
  }

  header {
    height: 34px;
    padding-left: 70px; /* traffic lights */
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .title { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 6px; }
  .name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .model { color: var(--muted); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status { display: flex; align-items: center; gap: 5px; font-size: 11px; color: var(--muted); white-space: nowrap; }
  .status i { width: 7px; height: 7px; border-radius: 50%; background: var(--muted); }
  .status.connected i { background: var(--green); box-shadow: 0 0 0 3px color-mix(in srgb, var(--green) 25%, transparent); }
  .status.error i { background: var(--red); }
  .status.scanning i, .status.connecting i { background: var(--amber); animation: blink 1s infinite; }
  @keyframes blink { 50% { opacity: 0.3; } }
  .pin { border: 0; background: none; color: var(--muted); padding: 3px; border-radius: 5px; display: grid; }
  .pin.on { color: var(--blue); }
  .pin:hover { background: var(--track); }

  .tabs { display: grid; grid-template-columns: 1fr 1fr; gap: 2px; padding: 2px; border-radius: 8px; background: var(--track); }
  .tabs button { border: 0; background: none; color: var(--muted); font: 600 12px -apple-system, sans-serif; padding: 4px 0; border-radius: 6px; }
  .tabs button.on { background: var(--card); color: var(--text); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12); }

  section { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 10px 12px; }

  .hero { display: flex; align-items: center; gap: 12px; transition: opacity 0.3s; }
  .hero.stale { opacity: 0.5; }
  .ring { width: 118px; height: 118px; flex: none; }
  .track { fill: none; stroke: var(--track); stroke-width: 8; }
  .arc { fill: none; stroke: var(--green); stroke-width: 8; stroke-linecap: round; transition: stroke-dasharray 0.6s ease; }
  .arc.low { stroke: var(--red); }
  .soc { font-size: 26px; font-weight: 600; text-anchor: middle; fill: var(--text); }
  .pct { font-size: 13px; fill: var(--muted); }
  .socl { font-size: 8.5px; text-anchor: middle; fill: var(--muted); }
  .big { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .big .v { font-size: 20px; font-weight: 600; }
  .big .u { margin-left: 3px; color: var(--muted); font-size: 12px; }
  .big .w .v { font-size: 15px; font-weight: 500; }
  .flow.charging .v { color: var(--green); }
  .flow.discharging .v { color: var(--amber); }
  .flowtag { margin-top: 2px; font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .flowtag.charging { color: var(--green); }
  .flowtag.discharging { color: var(--amber); }

  .chips, .alerts { display: flex; flex-wrap: wrap; gap: 6px; padding: 8px 10px; }
  .chip { font-size: 11px; font-weight: 600; padding: 3px 8px; border-radius: 6px; background: var(--track); color: var(--muted); }
  .chip.on { background: color-mix(in srgb, var(--green) 18%, transparent); color: var(--green); }
  .chip.bal { background: color-mix(in srgb, var(--blue) 18%, transparent); color: var(--blue); }
  .alert { font-size: 11px; font-weight: 600; padding: 3px 8px; border-radius: 6px; background: color-mix(in srgb, var(--red) 16%, transparent); color: var(--red); }
  .ok { font-size: 11px; color: var(--muted); }

  .tiles { display: grid; grid-template-columns: repeat(3, 1fr); padding: 8px 4px; }
  .tiles div { display: flex; flex-direction: column; align-items: center; gap: 1px; min-width: 0; }
  .tiles div + div { border-left: 1px solid var(--line); }
  .tiles .lbl { font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .tiles b { font-size: 15px; font-weight: 600; white-space: nowrap; }
  .tiles small { font-size: 10px; color: var(--muted); font-weight: 400; margin-left: 2px; }
  .tiles .warn { color: var(--amber); }

  .cells { display: flex; flex-direction: column; gap: 5px; }
  .cellhead { display: flex; justify-content: space-between; font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); margin-bottom: 2px; }
  .cell { display: grid; grid-template-columns: 14px 1fr 44px 8px; align-items: center; gap: 8px; }
  .idx { color: var(--muted); font-size: 11px; text-align: right; }
  .bar { height: 6px; border-radius: 3px; background: var(--track); overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 3px; background: var(--muted); transition: width 0.6s ease; }
  .cell.max .bar span { background: var(--green); }
  .cell.min .bar span { background: var(--amber); }
  .cv { text-align: right; font-size: 12px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; }
  .dot.on { background: var(--blue); }

  footer { display: flex; justify-content: space-between; font-size: 10px; color: var(--muted); padding: 0 4px; margin-top: auto; }

  .empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; text-align: center; color: var(--muted); }
  .empty p { margin: 0; }
  .hint { font-size: 11px; max-width: 240px; }
  .spinner { width: 22px; height: 22px; border: 2px solid var(--track); border-top-color: var(--blue); border-radius: 50%; animation: spin 0.9s linear infinite; }
  .spinner.off { border-top-color: var(--red); animation: none; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
