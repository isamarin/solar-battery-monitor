//! BLE session with the BMS: scan for service FF00, connect, poll every POLL, reconnect on failure.

use crate::history::{History, Sample};
use crate::protocol::{self, Basic, FrameAssembler, CMD_BASIC, CMD_CELLS, CMD_HW};
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{FutureExt, Stream, StreamExt};
use serde::Serialize;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

const SERVICE: Uuid = Uuid::from_u128(0x0000ff00_0000_1000_8000_00805f9b34fb);
const RX: Uuid = Uuid::from_u128(0x0000ff01_0000_1000_8000_00805f9b34fb);
const TX: Uuid = Uuid::from_u128(0x0000ff02_0000_1000_8000_00805f9b34fb);
const POLL: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);
/// the first request after connecting (or after the board fell asleep) only wakes it up
const ATTEMPTS: usize = 3;
/// a session that neither polls successfully nor ends for this long is abandoned together with its adapter;
/// longer than the worst-case scan + connect + first poll, and it also fires right after the Mac wakes from sleep
const STALL: Duration = Duration::from_secs(120);
/// consecutive sessions without a single successful poll before the adapter is recreated
const FRESH_ADAPTER_AFTER: u32 = 5;

type Notifications = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

#[derive(Serialize, Clone, Default)]
pub struct Snapshot {
    /// "scanning" | "connecting" | "connected" | "error"
    pub status: String,
    pub error: Option<String>,
    pub device: Option<String>,
    pub model: Option<String>,
    pub basic: Option<Basic>,
    pub cells: Vec<f64>,
    /// unix ms of the last successful poll
    pub updated_at: u64,
}

pub type Shared = Arc<Mutex<Snapshot>>;
pub type SharedHistory = Arc<Mutex<History>>;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn publish(app: &AppHandle, shared: &Shared, f: impl FnOnce(&mut Snapshot)) {
    let snap = {
        let mut s = shared.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("bms", snap);
}

pub async fn run(app: AppHandle, shared: Shared, history: SharedHistory) {
    let manager = match Manager::new().await {
        Ok(m) => m,
        Err(e) => return publish(&app, &shared, |s| fail(s, format!("Bluetooth unavailable: {e}"))),
    };
    // every `adapters()` call spawns a CoreBluetooth thread that never exits, so keep one adapter
    // and replace it only when it looks wedged (e.g. after system sleep)
    let mut adapter: Option<Adapter> = None;
    let mut failures = 0;
    loop {
        let a = match &adapter {
            Some(a) => a,
            None => match open_adapter(&manager).await {
                Ok(a) => adapter.insert(a),
                Err(e) => {
                    eprintln!("[bms] {e}");
                    publish(&app, &shared, |s| fail(s, e));
                    sleep(Duration::from_secs(3)).await;
                    continue;
                }
            },
        };
        let started = now_ms();
        let progress = AtomicU64::new(started);
        let result = tokio::select! {
            r = session(&app, &shared, &history, a, &progress) => r,
            _ = stalled(&progress) => Err(format!("no progress for {} s", STALL.as_secs())),
        };
        let stall = matches!(&result, Err(e) if e.starts_with("no progress"));
        if let Err(e) = result {
            eprintln!("[bms] session ended: {e}");
            publish(&app, &shared, |s| fail(s, e));
        }
        failures = if progress.load(Ordering::Relaxed) > started { 0 } else { failures + 1 };
        if stall || failures >= FRESH_ADAPTER_AFTER {
            eprintln!("[bms] recreating Bluetooth adapter");
            adapter = None;
            failures = 0;
        }
        sleep(Duration::from_secs(3)).await;
    }
}

async fn open_adapter(manager: &Manager) -> Result<Adapter, String> {
    // Adapter::new waits for CoreBluetooth's first state update, which may never come right after wake
    guard("open adapter", Duration::from_secs(10), manager.adapters())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "no Bluetooth adapter".to_string())
}

/// Resolves once `progress` (unix ms of the last successful poll) is older than STALL.
/// Wall-clock time, so a Mac that slept through a session trips it right after waking.
async fn stalled(progress: &AtomicU64) {
    loop {
        sleep(Duration::from_secs(5)).await;
        if now_ms().saturating_sub(progress.load(Ordering::Relaxed)) > STALL.as_millis() as u64 {
            return;
        }
    }
}

fn fail(s: &mut Snapshot, e: String) {
    s.status = "error".into();
    s.error = Some(e);
}

/// CoreBluetooth calls can hang forever after a silent disconnect; bound every one of them.
async fn guard<T, E: std::fmt::Display>(what: &str, limit: Duration, f: impl std::future::Future<Output = Result<T, E>>) -> Result<T, String> {
    match timeout(limit, f).await {
        Ok(r) => r.map_err(|e| format!("{what}: {e}")),
        Err(_) => Err(format!("{what}: timed out")),
    }
}

const OP: Duration = Duration::from_secs(5);

async fn find_bms(adapter: &Adapter) -> Result<Peripheral, String> {
    guard("start scan", OP, adapter.start_scan(ScanFilter { services: vec![SERVICE] })).await?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let found = 'scan: loop {
        for p in guard("peripherals", OP, adapter.peripherals()).await? {
            if matches!(guard("properties", OP, p.properties()).await, Ok(Some(ref props)) if props.services.contains(&SERVICE)) {
                break 'scan Ok(p);
            }
        }
        if Instant::now() > deadline {
            break Err("BMS not found — is a phone app connected to it?".to_string());
        }
        sleep(Duration::from_millis(500)).await;
    };
    let _ = guard("stop scan", OP, adapter.stop_scan()).await;
    found
}

async fn session(app: &AppHandle, shared: &Shared, history: &SharedHistory, adapter: &Adapter, progress: &AtomicU64) -> Result<(), String> {
    publish(app, shared, |s| s.status = "scanning".into());
    let p = find_bms(adapter).await?;
    let props = guard("properties", OP, p.properties()).await.ok().flatten();
    let name = props.as_ref().and_then(|pr| pr.local_name.clone());
    eprintln!("[bms] found {} rssi {:?}", name.as_deref().unwrap_or("?"), props.and_then(|pr| pr.rssi));

    publish(app, shared, |s| {
        s.status = "connecting".into();
        s.device = name.clone();
    });
    let started = Instant::now();
    let result = poll(app, shared, history, &p, progress).await;
    eprintln!("[bms] session lasted {} s", started.elapsed().as_secs());
    let _ = guard("disconnect", OP, p.disconnect()).await;
    result
}

async fn poll(app: &AppHandle, shared: &Shared, history: &SharedHistory, p: &Peripheral, progress: &AtomicU64) -> Result<(), String> {
    guard("connect", Duration::from_secs(15), p.connect()).await?;
    guard("discover services", Duration::from_secs(10), p.discover_services()).await?;
    let chars = p.characteristics();
    let find = |u: Uuid| chars.iter().find(|c| c.uuid == u).cloned().ok_or(format!("characteristic {u} missing"));
    let (rx, tx) = (find(RX)?, find(TX)?);
    eprintln!("[bms] tx properties {:?}", tx.properties);
    guard("subscribe", OP, p.subscribe(&rx)).await?;
    let mut link = Link { p, tx, notes: guard("notifications", OP, p.notifications()).await?, asm: FrameAssembler::default() };

    let model = link
        .request(CMD_HW)
        .await
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim_matches(char::from(0)).trim().to_string());

    loop {
        let basic = protocol::parse_basic(&link.request(CMD_BASIC).await?)?;
        let cells = protocol::parse_cells(&link.request(CMD_CELLS).await?);
        let sample = Sample {
            t: now_ms(),
            soc: basic.soc,
            v: basic.voltage,
            i: basic.current,
            cells: cells.clone(),
            temps: basic.temps.clone(),
            fet: Some(basic.charge_fet as u8 | (basic.discharge_fet as u8) << 1),
            prot: Some(basic.protection),
            bal: Some(basic.balancing),
        };
        if history.lock().unwrap().push(sample.clone()) {
            let _ = app.emit("sample", sample);
        }
        publish(app, shared, |s| {
            s.status = "connected".into();
            s.error = None;
            s.model = model.clone();
            s.basic = Some(basic);
            s.cells = cells;
            s.updated_at = now_ms();
        });
        progress.store(now_ms(), Ordering::Relaxed);
        sleep(POLL).await;
        if !guard("is_connected", OP, p.is_connected()).await.unwrap_or(false) {
            return Err("connection lost".into());
        }
    }
}

struct Link<'a> {
    p: &'a Peripheral,
    tx: Characteristic,
    notes: Notifications,
    asm: FrameAssembler,
}

impl Link<'_> {
    async fn request(&mut self, cmd: u8) -> Result<Vec<u8>, String> {
        let frame = protocol::read_request(cmd);
        let mut last = format!("no reply to command 0x{cmd:02x}");
        for _ in 0..ATTEMPTS {
            // drop late replies from earlier requests
            while let Some(Some(_)) = self.notes.next().now_or_never() {}
            self.asm.clear();
            guard("write", OP, self.p.write(&self.tx, &frame, WriteType::WithoutResponse)).await?;
            let deadline = Instant::now() + REPLY_TIMEOUT;
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                match timeout(left, self.notes.next()).await {
                    Ok(Some(n)) if n.uuid == RX => match self.asm.push(&n.value, cmd) {
                        Some(Ok(payload)) => return Ok(payload),
                        Some(Err(e)) => {
                            last = e;
                            break;
                        }
                        None => {}
                    },
                    Ok(Some(_)) => {}
                    Ok(None) => return Err("notification stream closed".into()),
                    Err(_) => break,
                }
            }
        }
        Err(last)
    }
}
