<script lang="ts">
  import { analyze, type Eta } from "./recovery";
  import type { Basic, Sample } from "./bms";

  let { samples, basic, now }: { samples: Sample[]; basic: Basic | null; now: number } = $props();

  // recompute on new samples and once a minute (ETAs are relative to now)
  const minute = $derived(Math.floor(now / 60_000));
  const r = $derived.by(() => {
    void minute;
    return basic ? analyze(samples, basic.nominal_ah, basic.soc) : null;
  });

  const clock = (t: number) => {
    const d = new Date(t);
    const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    const days = Math.round((new Date(d).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86_400_000);
    return days === 0 ? time : days === 1 ? `tomorrow ${time}` : d.toLocaleDateString([], { weekday: "short" }) + " " + time;
  };
  const dur = (ms: number) => {
    const m = Math.max(0, Math.round(ms / 60_000));
    return m < 60 ? `${m} min` : m < 48 * 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${Math.round(m / 60)} h`;
  };
  const etaText = (e: Eta) => (e.done ? (e.reached ? `✓ ${clock(e.reached)}` : "✓ reached") : e.at ? clock(e.at) : "—");
  const etaSub = (e: Eta) => (e.done ? "" : e.at ? `in ${dur(e.at - now)}` : r?.charging ? "not predictable yet" : "not charging");
  const sign = (v: number) => (v > 0 ? "+" : "") + v.toFixed(0);
</script>

{#if !r}
  <section class="empty">Waiting for data…</section>
{:else}
  <section class="soc">
    <div class="socrow">
      <span class="big">{r.soc === null ? "—" : `${r.socSource === "bms" ? "" : "~"}${r.soc.toFixed(0)}`}<small>%</small></span>
      <span class="src">
        {#if r.socSource === "bms"}BMS state of charge{:else}estimated from {r.socSource}<br /><em>BMS shows {basic?.soc}% until its first full charge</em>{/if}
      </span>
    </div>
    <div class="bar"><span style="width:{r.soc ?? 0}%"></span></div>
    {#if r.session}
      <div class="session">
        <span><b>+{r.session.ah.toFixed(2)}</b> Ah</span>
        <span><b>{r.session.wh.toFixed(0)}</b> Wh</span>
        <span><b>{dur(r.session.end - r.session.start)}</b> {r.charging ? "charging" : "last charge"}</span>
        <span><b>{r.session.avgA.toFixed(2)}</b> A avg</span>
      </div>
      {#if r.session.untrackedMin > 0}<p class="note">{dur(r.session.untrackedMin * 60_000)} without data not counted</p>{/if}
    {/if}
  </section>

  <section>
    <h3>Forecast {#if r.charging}<span>at {r.rateA.toFixed(2)} A</span>{/if}</h3>
    {#each r.etas as e}
      <div class="eta" class:done={e.done}>
        <div class="l">{e.label}{#if e.note && !e.done}<small>{e.note}</small>{/if}</div>
        <div class="r"><b>{etaText(e)}</b><small>{etaSub(e)}</small></div>
      </div>
    {/each}
    {#if basic}
      <div class="eta">
        <div class="l">Discharge output</div>
        <div class="r"><b class={basic.discharge_fet ? "good" : "warn"}>{basic.discharge_fet ? "unlocked" : "blocked"}</b></div>
      </div>
    {/if}
  </section>

  <section>
    <h3>Cell health <span>last 30 min</span></h3>
    <div class="hrow">
      <span>Imbalance Δ</span>
      <b>{r.health.deltaMv.toFixed(0)} mV</b>
      {#if r.health.deltaTrendMvH !== null}
        <em class={r.health.deltaTrendMvH > 5 ? "warn" : r.health.deltaTrendMvH < -2 ? "good" : ""}>
          {r.health.deltaTrendMvH > 5 ? "↗ diverging" : r.health.deltaTrendMvH < -2 ? "↘ converging" : "→ stable"} {sign(r.health.deltaTrendMvH)} mV/h
        </em>
      {/if}
    </div>
    <div class="rates">
      {#each r.health.cellRatesMvH as v, i}
        <div class:warn={r.health.fastCell === i}><span>Cell {i + 1}</span><b>{v === null ? "—" : sign(v)}</b><small>mV/h</small></div>
      {/each}
    </div>
    {#if r.health.fastCell !== null}
      <p class="alert">Cell {r.health.fastCell + 1} is rising much faster than the others. That usually means lower capacity, and it will hit overvoltage first. Watch it near the top of charge.</p>
    {/if}
    {#if r.health.maxTemp !== null && r.health.maxTemp > 45}
      <p class="alert">Temperature {r.health.maxTemp.toFixed(1)} °C. Reduce the charge current.</p>
    {/if}
  </section>

  <section>
    <h3>Self-discharge test</h3>
    {#if r.rest}
      <div class="hrow">
        <span>Resting {dur(r.rest.hours * 3_600_000)}</span>
        <b>{r.rest.startV.toFixed(2)} → {r.rest.nowV.toFixed(2)} V</b>
      </div>
      {#if r.rest.dropMvPerCellDay === null}
        <p class="note">Measurement starts 1 h after the current stops, once the surface charge has settled.</p>
      {:else}
        <p class:alert={r.rest.dropMvPerCellDay > 20} class:note={r.rest.dropMvPerCellDay <= 20}>
          {r.rest.dropMvPerCellDay.toFixed(1)} mV per cell per day. {r.rest.dropMvPerCellDay > 20 ? "High — a damaged cell may be leaking." : "Normal for LiFePO4."}
        </p>
      {/if}
    {:else}
      <p class="note">Starts automatically when charging stops. Let the pack rest 12–24 h after the first full charge, without a load.</p>
    {/if}
  </section>

  <section>
    <h3>Events</h3>
    {#each r.events as e}
      <div class="ev {e.kind}"><time>{clock(e.t)}</time><span>{e.text}</span></div>
    {:else}
      <p class="note">No state changes recorded yet.</p>
    {/each}
  </section>
{/if}

<style>
  section { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 10px 12px; }
  h3 { margin: 0 0 6px; font-size: 10px; font-weight: 400; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); display: flex; justify-content: space-between; }
  h3 span { text-transform: none; letter-spacing: 0; }
  .empty { text-align: center; color: var(--muted); padding: 30px 12px; }
  .note { margin: 6px 0 0; font-size: 11px; color: var(--muted); }
  .alert { margin: 6px 0 0; font-size: 11px; color: var(--red); }
  .good { color: var(--green); }
  .warn { color: var(--amber); }

  .socrow { display: flex; align-items: center; gap: 12px; }
  .big { font-size: 34px; font-weight: 600; line-height: 1; }
  .big small { font-size: 15px; color: var(--muted); font-weight: 400; margin-left: 2px; }
  .src { font-size: 11px; color: var(--muted); }
  .src em { font-style: normal; font-size: 10px; }
  .bar { height: 6px; border-radius: 3px; background: var(--track); margin: 8px 0; overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--green); border-radius: 3px; transition: width 0.6s; }
  .session { display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 11px; color: var(--muted); }
  .session b { color: var(--text); font-size: 13px; }

  .eta { display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 5px 0; border-top: 1px solid var(--line); }
  .eta:first-of-type { border-top: 0; }
  .eta .l { display: flex; flex-direction: column; font-size: 12px; }
  .eta .r { display: flex; flex-direction: column; align-items: flex-end; text-align: right; white-space: nowrap; }
  .eta small { font-size: 10px; color: var(--muted); }
  .eta b { font-size: 12px; }
  .eta.done .l { color: var(--muted); }
  .eta.done b { color: var(--green); }

  .hrow { display: flex; align-items: baseline; gap: 8px; font-size: 12px; flex-wrap: wrap; }
  .hrow span { color: var(--muted); }
  .hrow em { font-style: normal; font-size: 11px; margin-left: auto; }
  .rates { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; margin-top: 8px; }
  .rates div { display: flex; flex-direction: column; align-items: center; background: var(--track); border-radius: 6px; padding: 4px 0; }
  .rates div.warn { outline: 1px solid var(--amber); }
  .rates span, .rates small { font-size: 10px; color: var(--muted); }
  .rates b { font-size: 13px; }

  .ev { display: grid; grid-template-columns: 92px 1fr; gap: 6px; font-size: 11px; padding: 3px 0; }
  .ev time { color: var(--muted); }
  .ev.good span { color: var(--green); }
  .ev.warn span { color: var(--amber); }
</style>
