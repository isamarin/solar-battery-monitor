// Mirrors ble::Snapshot / protocol::Basic on the Rust side.
export interface Basic {
  voltage: number;
  current: number; // + charging, - discharging
  remaining_ah: number;
  nominal_ah: number;
  cycles: number;
  production_date: string;
  balancing: number; // bit n = cell n+1
  protection: number;
  protections: string[];
  sw_version: string;
  soc: number;
  charge_fet: boolean;
  discharge_fet: boolean;
  cell_count: number;
  temps: number[];
}

// Same order as protocol::PROTECTION_FLAGS on the Rust side.
export const PROTECTION_FLAGS = [
  "Cell overvoltage",
  "Cell undervoltage",
  "Pack overvoltage",
  "Pack undervoltage",
  "Charge overtemp",
  "Charge undertemp",
  "Discharge overtemp",
  "Discharge undertemp",
  "Charge overcurrent",
  "Discharge overcurrent",
  "Short circuit",
  "AFE error",
  "MOS software lock",
];

export interface Snapshot {
  status: "scanning" | "connecting" | "connected" | "error" | "";
  error: string | null;
  device: string | null;
  model: string | null;
  basic: Basic | null;
  cells: number[];
  updated_at: number;
}

// LiFePO4 display range for cell bars
export const CELL_MIN = 2.5;
export const CELL_MAX = 3.65;

// Mirrors history::Sample (one point every 5 s, last 72 h).
export interface Sample {
  t: number; // unix ms
  soc: number;
  v: number;
  i: number;
  cells: number[];
  temps: number[];
  // absent in samples recorded before these fields existed
  fet?: number; // 1 = charge, 2 = discharge
  prot?: number;
  bal?: number;
}
