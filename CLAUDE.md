# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository layout

- `widget/`: a macOS desktop widget built with Tauri v2 (Rust) and Svelte 5 (SvelteKit in SPA mode). It reads the BMS over BLE and is read-only; it never writes to the board.
- `jbd.py`: a standalone Python/bleak script that reads the BMS once, for quick checks.
- Vendor datasheet: `JBD-DP04S007 V1.6-BMS Specification.pdf` (not committed). Extract it with `pdftotext -layout "<file>" -`.

## Commands (run in `widget/`)

- `pnpm install` once, then `pnpm tauri dev` to run with hot reload. The dev binary gets Bluetooth permission through the terminal it is launched from.
- `pnpm tauri build` produces the release `.app`/`.dmg`. `src-tauri/Info.plist` supplies `NSBluetoothAlwaysUsageDescription`, which a bundled app needs for Bluetooth access.
- `pnpm check` type-checks Svelte/TS.
- Rust tests: `cd src-tauri && cargo test`. To run a single test: `cargo test assembles_split_cell_frame`.

## Widget architecture

- `src-tauri/src/protocol.rs` is pure and unit-tested. It builds JBD frames (`read_request`), reassembles notification chunks into frames (`FrameAssembler`, which checks tail and checksum and ignores late replies to other commands), and parses `03` (`parse_basic`) and `04` (`parse_cells`).
- `src-tauri/src/ble.rs` is a long-lived tokio task started in `lib.rs` `setup`. It runs scan (service `FF00`) → connect → read `05` model → poll `03`+`04` every 2 s. On any error it reconnects after 3 s. Every CoreBluetooth call is wrapped in `guard()` with a timeout, because btleplug on macOS can hang forever after a silent disconnect. `request()` retries 3× because of the wake-up quirk below. Errors are logged to stderr with the prefix `[bms]`.
- State reaches the frontend in two ways. The full `Snapshot` is emitted as event `bms` on every poll and is also available through command `snapshot`. `history.rs` keeps one `Sample` per 5 s for 24 h, appended as JSON lines to `~/Library/Application Support/com.igor.jbdwidget/history.jsonl` and compacted on start. New points are emitted as event `sample`, and the full list is available through command `history`.
- `src/lib/bms.ts` mirrors the Rust structs by hand, so keep the two in sync. `src/routes/+page.svelte` has the Now/Charts tabs. `src/lib/Charts.svelte` and `src/lib/Chart.svelte` use uPlot with one y-axis per chart and a synced cursor. Chart colors come from the CSS variables `--s1..--s4`, with separate values for light and dark themes.

## Target hardware (from the spec)

JBD-DP04S007 V1.6: a smart ("software") BMS for 3–4S LiFePO4 packs, 60–200A continuous, with a common charge/discharge port.
- Architecture: an analog front-end (AFE) chip, **Devechip DVC1006**, plus an MCU, **Telink TLSR8250F512ET32** (a BLE SoC, which is why Bluetooth is a standard option).
- Interfaces: **non-isolated UART plus a Bluetooth module only.** RS485, CAN, RS232 and isolated UART are *not* supported.
- Features: 1 external NTC (10K, B3950). Passive balancing: pulsed, resistor bypass, and two adjacent cells never balance at the same time. Coulomb-counting SOC with cycle counting. Optional heating film (H-/PTC) and optional soft switch (J4, K+/K-).
- Sleep mode starts after about 1 min with no current, no communication, no balancing and no protection active. Current draw is ≤800 µA asleep and ≤10 mA running. Communication, the switch or current flow wakes the board. **A client talking to the board must expect the first request after idle to wake the board.**

## Serial link

- **9600 baud, 8N1, no parity.**
- Connector J3 (HY2.0-4P): pin 1 GND, pin 2 RXD, pin 3 TXD, pin 4 VDD 5V. (The spec's remark calls the UART "J4", but the pin table puts it on J3. J4 is the switch.)
- UART ground is B-. The port is non-isolated and cannot talk to chargers or loads.
- The vendor PC tool is **JBDTOOLS ≥ V4.2 with the "AFE_DC10XX" profile** selected. The vendor workflow requires reading the parameters before writing any.
- **The PDF does not define the frame format, commands or register map.** Take the protocol from another source, such as the JBD/Xiaoxiang protocol documentation or a capture of JBDTOOLS traffic, and record here which source was used.

## Bluetooth link (verified 2026-09-29)

- The board advertises as **"BatteryOne"** with service `FF00`. It uses notify on `FF01` and write on `FF02` (write without response).
- It speaks the standard JBD frame protocol: `DD A5 <cmd> 00 <checksum hi> <checksum lo> 77`, where checksum = 0x10000 − sum(cmd, len). The reply is `DD <cmd> <status> <len> <payload> <checksum> 77` and can arrive split across several notifications. Cmd `03` returns basic info and cmd `04` returns cell voltages in mV.
- **The first request after connecting is always lost, because it wakes the board.** Send a dummy request first or retry.
- The advertisement disappears while a phone app (Xiaoxiang/JBD) is connected. Only one client can be connected at a time.
- `jbd.py` is a read-only status client for macOS. Run it with `uv run jbd.py`: it scans, connects to the first `FF00` device, and prints voltage, current, SOC, temperature, FET state, protection flags and cell voltages.

## Default protection parameters (typical values)

| Parameter | Trip | Delay | Release |
|---|---|---|---|
| Cell over-voltage | 3.75 V | 2 s | 3.60 V |
| Cell under-voltage | 2.20 V | 2 s | 2.60 V (or on charging) |
| Charge over-current | see table | 10 s | auto after 32 s, or on discharging |
| Discharge OC level 1 / level 2 | see table | 10 s / 300 ms | auto after 32 s, or on charging |
| Short circuit | see table | ~560 µs | about 5 s after the load is removed |
| Charge temp, high | 65 °C | – | 55 °C |
| Charge temp, low (without / with heating) | -10 / 0 °C | – | -5 / 5 °C |
| Discharge temp, high / low | 75 / -20 °C | – | 65 / -10 °C |
| FET temp | 105 °C | – | 75 °C |
| Balancing | starts at 3.30 V and Δ ≥ 15 mV, 200 mA | | |

Over-current thresholds by rating (charge OC = discharge OC1 / discharge OC2 / short circuit):
60A → 70 / 210 / 750 A · 80A → 90 / 280 / 1000 A · 100A → 110 / 330 / 1350 A · 120A → 130 / 440 / 1550 A · 150A → 160 / 550 / 1800 A · 200A → 210 / 760 / 2400 A.

Cell voltage range is 2.20–3.75 V. Charge voltage is 3.6 V × cell count. Operating temperature is -20 to 75 °C.

## Wiring (J1, HY2.0-5P voltage sense)

Pins 1–5 = BC0 (cell 1 negative) to BC4 (cell 4 positive). For 3S, pins 1–4 carry BC0–BC3 and pin 5 is left unused. J2 is the NTC.
