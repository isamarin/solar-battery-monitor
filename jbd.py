# /// script
# dependencies = ["bleak"]
# ///
import asyncio, struct
from bleak import BleakScanner, BleakClient
NOTIFY, WRITE = "0000ff01-0000-1000-8000-00805f9b34fb", "0000ff02-0000-1000-8000-00805f9b34fb"

def req(cmd):
    cs = (0x10000 - (cmd + 0)) & 0xFFFF
    return bytes([0xDD, 0xA5, cmd, 0x00, cs >> 8, cs & 0xFF, 0x77])

async def main():
    found = await BleakScanner.discover(timeout=12, return_adv=True)
    cands = []
    for addr, (d, adv) in found.items():
        name = (d.name or adv.local_name or "")
        u = [x[4:8] for x in adv.service_uuids]
        print(f"{adv.rssi:4d} {name or '-':25} svc={u}")
        if "ff00" in u or name.lower().startswith(("xiaoxiang", "jbd", "sp0", "dp0")):
            cands.append((d, name))
    if not cands:
        print("\nJBD NOT FOUND"); return
    d, name = cands[0]
    print(f"\n==> connecting to {name} ({d.address})")
    buf = bytearray(); done = asyncio.Event()
    def on(_, data):
        buf.extend(data)
        if len(buf) >= 4 and len(buf) >= buf[3] + 7: done.set()
    async with BleakClient(d, timeout=20) as c:
        await c.start_notify(NOTIFY, on)
        out = {}
        for cmd in (4, 3, 3):
            buf.clear(); done.clear()
            await c.write_gatt_char(WRITE, req(cmd), response=False)
            try: await asyncio.wait_for(done.wait(), 5)
            except asyncio.TimeoutError: print(f"cmd {cmd:02x}: timeout, raw={buf.hex()}"); continue
            if buf[2] != 0: print(f"cmd {cmd:02x}: status {buf[2]:02x}"); continue
            out[cmd] = bytes(buf[4:4 + buf[3]])
    if 3 in out:
        p = out[3]
        v, i, rem, nom, cyc = struct.unpack(">HhHHH", p[0:10])
        prot = struct.unpack(">H", p[16:18])[0]
        ver, soc, fet, cells, ntc = p[18], p[19], p[20], p[21], p[22]
        temps = [(struct.unpack(">H", p[23+2*k:25+2*k])[0] - 2731) / 10 for k in range(ntc)]
        bal = struct.unpack(">I", p[12:16])[0]
        print(f"Voltage {v/100:.2f} V | Current {i/100:.2f} A | SOC {soc}% | {rem/100:.2f}/{nom/100:.2f} Ah | cycles {cyc}")
        print(f"Cells {cells} | Temps {temps} °C | FET charge={'ON' if fet&1 else 'OFF'} discharge={'ON' if fet&2 else 'OFF'} | fw 0x{ver:02x}")
        print(f"Protection 0x{prot:04x} | balancing mask 0x{bal:x}")
    if 4 in out:
        cv = [x/1000 for x in struct.unpack(f">{len(out[4])//2}H", out[4])]
        print("Cell V:", cv, f"| Δ {1000*(max(cv)-min(cv)):.0f} mV")
asyncio.run(main())
