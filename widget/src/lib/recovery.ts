// Recovery analytics for a deeply discharged LiFePO4 pack, derived purely from the sample history.
// All estimates assume LiFePO4 chemistry and the current charge rate staying constant.
import { PROTECTION_FLAGS, type Sample } from "./bms";

const H = 3600_000;
const MIN = 60_000;

/** Cell voltage → SOC % for LiFePO4 at low charge rate (<0.1C). Only trustworthy below the plateau and at the top. */
const OCV: [number, number][] = [
  [2.5, 0], [2.9, 1], [3.0, 3], [3.1, 6], [3.15, 8], [3.2, 12], [3.25, 25], [3.27, 40],
  [3.28, 50], [3.3, 65], [3.32, 80], [3.35, 90], [3.4, 97], [3.45, 100],
];
/** Below this the voltage curve is steep enough to read SOC from it. */
export const PLATEAU_V = 3.2;
/** JBD balancing turn-on voltage (spec 3.2: 3.30 V, Δ ≥ 15 mV). */
export const BALANCE_V = 3.3;
const BALANCE_SOC = 88;
const CHARGING_A = 0.1;
const IDLE_A = 0.05;
/** Gaps longer than this (app closed, BLE lost) are not integrated. */
const MAX_GAP = 30 * MIN;

export function ocvSoc(cellV: number): number {
  if (cellV <= OCV[0][0]) return 0;
  for (let k = 1; k < OCV.length; k++) {
    const [v1, s1] = OCV[k];
    if (cellV <= v1) {
      const [v0, s0] = OCV[k - 1];
      return s0 + ((cellV - v0) / (v1 - v0)) * (s1 - s0);
    }
  }
  return 100;
}

const avg = (a: number[]) => a.reduce((x, y) => x + y, 0) / a.length;
const maxCell = (s: Sample) => Math.max(...s.cells);
const avgCell = (s: Sample) => avg(s.cells);

/** Least-squares slope of y over time, per hour. */
function slopePerHour(pts: Sample[], y: (s: Sample) => number): number | null {
  if (pts.length < 5 || pts[pts.length - 1].t - pts[0].t < 5 * MIN) return null;
  const t0 = pts[0].t;
  const xs = pts.map((p) => (p.t - t0) / H);
  const ys = pts.map(y);
  const mx = avg(xs), my = avg(ys);
  let num = 0, den = 0;
  xs.forEach((x, k) => {
    num += (x - mx) * (ys[k] - my);
    den += (x - mx) ** 2;
  });
  return den ? num / den : null;
}

/** Trapezoid integration of current; skips gaps longer than MAX_GAP. */
function integrate(pts: Sample[]) {
  let ah = 0, wh = 0, untracked = 0;
  for (let k = 1; k < pts.length; k++) {
    const a = pts[k - 1], b = pts[k];
    const dt = b.t - a.t;
    if (dt > MAX_GAP) {
      untracked += dt;
      continue;
    }
    const h = dt / H;
    ah += ((a.i + b.i) / 2) * h;
    wh += ((a.i * a.v + b.i * b.v) / 2) * h;
  }
  return { ah, wh, untrackedMin: untracked / MIN };
}

export interface Eta {
  label: string;
  /** unix ms; null when not predictable */
  at: number | null;
  /** already happened */
  done?: boolean;
  /** when it happened, if seen in the history */
  reached?: number;
  note?: string;
}

export interface RecoveryEvent {
  t: number;
  text: string;
  kind: "good" | "warn" | "info";
}

export interface Recovery {
  soc: number | null;
  socSource: "bms" | "voltage" | "voltage + Ah count";
  charging: boolean;
  rateA: number;
  session: { start: number; end: number; ah: number; wh: number; hours: number; avgA: number; untrackedMin: number } | null;
  etas: Eta[];
  health: {
    deltaMv: number;
    deltaTrendMvH: number | null;
    cellRatesMvH: (number | null)[];
    fastCell: number | null;
    maxTemp: number | null;
  };
  rest: { since: number; hours: number; startV: number; nowV: number; dropMvPerCellDay: number | null } | null;
  events: RecoveryEvent[];
}

/** Last contiguous charging run (brief dips < 2 min tolerated). */
function chargingSession(s: Sample[]): [number, number] | null {
  let end = s.length - 1;
  while (end >= 0 && s[end].i <= CHARGING_A) end--;
  if (end < 0) return null;
  let start = end;
  let dipStart: number | null = null;
  for (let k = end - 1; k >= 0; k--) {
    if (s[k + 1].t - s[k].t > MAX_GAP) break;
    if (s[k].i > CHARGING_A) {
      dipStart = null;
      start = k;
    } else {
      dipStart ??= s[k].t;
      if (dipStart - s[k].t > 2 * MIN) break;
    }
  }
  return [start, end];
}

function thresholdTime(s: Sample[], from: number, v: number): number | undefined {
  for (let k = Math.max(from, 1); k < s.length; k++) if (maxCell(s[k]) >= v && maxCell(s[k - 1]) < v) return s[k].t;
  return from < s.length && maxCell(s[from]) >= v ? s[from].t : undefined;
}

export function analyze(s: Sample[], nominalAh: number, bmsSoc: number, now = Date.now()): Recovery | null {
  if (s.length < 2 || nominalAh <= 0) return null;
  const last = s[s.length - 1];
  const charging = last.i > CHARGING_A;
  const recent = s.filter((p) => p.t >= last.t - 10 * MIN);
  const rateA = charging ? avg(recent.filter((p) => p.i > CHARGING_A).map((p) => p.i)) : 0;

  // SOC: the BMS keeps 0 until its first full-charge calibration; until then estimate it ourselves
  let soc: number;
  let socSource: Recovery["socSource"];
  const vNow = avgCell(last);
  if (bmsSoc > 0) {
    soc = bmsSoc;
    socSource = "bms";
  } else if (vNow < PLATEAU_V) {
    soc = ocvSoc(vNow);
    socSource = "voltage";
  } else {
    // anchor at the last point where the voltage was still readable, then count Ah
    let k = s.length - 1;
    while (k > 0 && avgCell(s[k]) >= PLATEAU_V) k--;
    const anchor = ocvSoc(Math.min(avgCell(s[k]), PLATEAU_V));
    soc = anchor + (integrate(s.slice(k)).ah / nominalAh) * 100;
    if (charging) soc = Math.max(soc, ocvSoc(vNow));
    socSource = "voltage + Ah count";
  }
  soc = Math.max(0, Math.min(100, soc));

  const run = chargingSession(s);
  let session: Recovery["session"] = null;
  if (run) {
    const pts = s.slice(run[0], run[1] + 1);
    const { ah, wh, untrackedMin } = integrate(pts);
    const hours = (pts[pts.length - 1].t - pts[0].t) / H;
    session = { start: pts[0].t, end: pts[pts.length - 1].t, ah, wh, hours, avgA: hours > 0 ? ah / hours : 0, untrackedMin };
  }

  // forecasts at the current charge rate
  const from = run ? run[0] : 0;
  const etas: Eta[] = [];
  const hoursToSoc = (target: number) => (charging && rateA > IDLE_A && soc < target ? (((target - soc) / 100) * nominalAh) / rateA : null);
  const at = (h: number | null) => (h === null ? null : now + h * H);

  const mc = maxCell(last);
  const plateauReached = thresholdTime(s, from, PLATEAU_V);
  let plateauEta: number | null = null;
  if (mc < PLATEAU_V && charging) {
    const slope = slopePerHour(s.filter((p) => p.t >= last.t - 20 * MIN), maxCell);
    if (slope && slope > 0.001) plateauEta = now + ((PLATEAU_V - mc) / slope) * H;
  }
  etas.push({ label: `Plateau · cells ≥ ${PLATEAU_V.toFixed(2)} V (~12%)`, at: plateauEta, done: mc >= PLATEAU_V, reached: plateauReached, note: "from voltage trend" });
  etas.push({ label: "50%", at: at(hoursToSoc(50)), done: soc >= 50 });
  const balReached = thresholdTime(s, from, BALANCE_V);
  etas.push({
    label: `Balancing starts · cell ≥ ${BALANCE_V.toFixed(2)} V (~${BALANCE_SOC}%)`,
    at: mc >= BALANCE_V ? null : at(hoursToSoc(BALANCE_SOC)),
    done: mc >= BALANCE_V,
    reached: balReached,
    note: "only near the top of charge",
  });
  const full = hoursToSoc(100);
  etas.push({ label: "100%", at: at(full), done: soc >= 100, note: full === null ? undefined : "+1–2 h absorption at the end" });

  // cell health over the last 30 min
  const win = s.filter((p) => p.t >= last.t - 30 * MIN);
  const n = last.cells.length;
  const cellRatesMvH = Array.from({ length: n }, (_, i) => {
    const r = slopePerHour(win, (p) => p.cells[i] ?? NaN);
    return r === null || Number.isNaN(r) ? null : r * 1000;
  });
  const valid = cellRatesMvH.filter((r): r is number => r !== null).sort((a, b) => a - b);
  const median = valid.length ? valid[Math.floor(valid.length / 2)] : 0;
  const fastIdx = cellRatesMvH.findIndex((r) => r !== null && valid.length >= 2 && r > median * 1.5 && r - median > 10);
  const deltaOf = (p: Sample) => (Math.max(...p.cells) - Math.min(...p.cells)) * 1000;
  const dTrend = slopePerHour(s.filter((p) => p.t >= last.t - 60 * MIN), deltaOf);

  // rest / self-discharge: measure from 1 h after current stopped (surface charge settles first)
  let rest: Recovery["rest"] = null;
  if (Math.abs(last.i) < IDLE_A) {
    let k = s.length - 1;
    while (k > 0 && Math.abs(s[k - 1].i) < IDLE_A && s[k].t - s[k - 1].t < MAX_GAP) k--;
    const restStart = s[k].t;
    const base = s.find((p) => p.t >= restStart + H);
    const hours = (last.t - restStart) / H;
    const sinceBase = base ? (last.t - base.t) / H : 0;
    rest = {
      since: restStart,
      hours,
      startV: base?.v ?? s[k].v,
      nowV: last.v,
      dropMvPerCellDay: base && sinceBase >= 0.5 ? (((base.v - last.v) * 1000) / n / sinceBase) * 24 : null,
    };
  }

  const temps = last.temps;
  return {
    soc,
    socSource,
    charging,
    rateA,
    session,
    etas,
    health: {
      deltaMv: deltaOf(last),
      deltaTrendMvH: dTrend,
      cellRatesMvH,
      fastCell: fastIdx >= 0 ? fastIdx : null,
      maxTemp: temps.length ? Math.max(...temps) : null,
    },
    rest,
    events: events(s),
  };
}

/** State transitions found by diffing consecutive samples, newest first. */
export function events(s: Sample[]): RecoveryEvent[] {
  const out: RecoveryEvent[] = [];
  const lastAt = new Map<string, number>();
  const add = (t: number, text: string, kind: RecoveryEvent["kind"], key = text, quiet = 10 * MIN) => {
    if (t - (lastAt.get(key) ?? -Infinity) < quiet) return;
    lastAt.set(key, t);
    out.push({ t, text, kind });
  };
  for (let k = 1; k < s.length; k++) {
    const a = s[k - 1], b = s[k];
    if (b.t - a.t > MAX_GAP) add(b.t, `No data for ${Math.round((b.t - a.t) / MIN)} min`, "info");
    if (a.i <= CHARGING_A && b.i > CHARGING_A) add(b.t, `Charging started · ${b.i.toFixed(2)} A`, "info", "chg", 2 * MIN);
    if (a.i > IDLE_A && b.i <= IDLE_A && b.i >= -IDLE_A) add(b.t, "Current stopped", "info", "chg", 2 * MIN);
    if (a.i >= -CHARGING_A && b.i < -CHARGING_A) add(b.t, `Discharging · ${(-b.i).toFixed(2)} A`, "info", "chg", 2 * MIN);
    if (a.fet !== undefined && b.fet !== undefined) {
      if ((a.fet ^ b.fet) & 2) add(b.t, b.fet & 2 ? "Discharge unlocked (DSG ON)" : "Discharge blocked (DSG OFF)", b.fet & 2 ? "good" : "warn");
      if ((a.fet ^ b.fet) & 1) add(b.t, b.fet & 1 ? "Charge enabled (CHG ON)" : "Charge blocked (CHG OFF)", b.fet & 1 ? "good" : "warn");
    }
    if (a.prot !== undefined && b.prot !== undefined && a.prot !== b.prot) {
      PROTECTION_FLAGS.forEach((name, bit) => {
        const was = (a.prot! >> bit) & 1, is = (b.prot! >> bit) & 1;
        if (!was && is) add(b.t, `${name} protection`, "warn");
        if (was && !is) add(b.t, `${name} cleared`, "good");
      });
    }
    if (a.bal !== undefined && b.bal !== undefined && !a.bal !== !b.bal) add(b.t, b.bal ? "Balancing started" : "Balancing stopped", "info");
    for (const v of [PLATEAU_V, BALANCE_V, 3.4, 3.55, 3.65]) {
      if (maxCell(a) < v && maxCell(b) >= v) add(b.t, `Highest cell reached ${v.toFixed(2)} V`, v >= 3.55 ? "warn" : "info", `v${v}`, 30 * MIN);
    }
  }
  return out.reverse().slice(0, 40);
}
