# solar-battery-monitor

A small macOS widget for a JBD smart BMS (tested with JBD-DP04S001 / DP04S007, 4S LiFePO4) over Bluetooth LE.
It is read-only: it polls the board and never writes to it.

- **Now tab:** SOC, voltage, current, power, charge/discharge MOSFETs, active protections, temperature, cycles, cell voltages and imbalance, and balancing state.
- **Charts tab:** SOC, power, cell voltages, cell imbalance and temperature, for the last 15 min / 1 h / 6 h / 24 h. History is kept locally for 24 h.

Built with Tauri v2 (Rust, [btleplug](https://github.com/deviceplug/btleplug)) and Svelte 5, with charts drawn by [uPlot](https://github.com/leeoniya/uPlot).
The protocol is the standard JBD `DD A5 … 77` frame format over service `FF00` (notify `FF01`, write `FF02`).

## Run

```sh
cd widget
pnpm install
pnpm tauri dev      # or: pnpm tauri build
```

Close the Xiaoxiang / JBD phone app first, because the BMS accepts only one BLE connection at a time.

`jbd.py` is a one-shot status reader: run it with `uv run jbd.py`.
