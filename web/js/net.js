// WebSocket connection to the simulation and binary frame parsing.

export const CELL_STRIDE = 52;

export class Net {
  constructor(handlers) {
    this.h = handlers;
    this.ws = null;
    this.retry = 0;
  }

  connect() {
    const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`;
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';
    ws.onopen = () => { this.retry = 0; this.h.onOpen?.(); };
    ws.onclose = () => {
      this.h.onClose?.();
      const wait = Math.min(4000, 250 * 2 ** this.retry++);
      setTimeout(() => this.connect(), wait);
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === 'string') {
        let m;
        try { m = JSON.parse(ev.data); } catch { return; }
        this.h.onJson?.(m);
        return;
      }
      const buf = ev.data;
      if (buf.byteLength < 4) return;
      const t = new Uint8Array(buf, 0, 1)[0];
      if (t === 1) this.h.onCells?.(parseCells(buf));
      else if (t === 2) this.h.onGrid?.(parseGrid(buf));
      else if (t === 3) this.h.onStatic?.(parseStatic(buf));
    };
    this.ws = ws;
  }

  send(obj) {
    if (this.ws && this.ws.readyState === 1) this.ws.send(JSON.stringify(obj));
  }
}

function parseCells(buf) {
  const dv = new DataView(buf);
  const n = dv.getUint32(4, true);
  const time = dv.getFloat64(8, true);
  const step = dv.getUint32(16, true) + dv.getUint32(20, true) * 4294967296;
  const nd = dv.getUint32(24, true);
  const daylight = dv.getFloat32(28, true);
  const ids = new Uint32Array(n);
  const pos = new Float32Array(n * 4);
  for (let i = 0, o = 32; i < n; i++, o += CELL_STRIDE) {
    ids[i] = dv.getUint32(o, true);
    pos[i * 4] = dv.getFloat32(o + 4, true);
    pos[i * 4 + 1] = dv.getFloat32(o + 8, true);
    pos[i * 4 + 2] = dv.getFloat32(o + 12, true);
    pos[i * 4 + 3] = dv.getFloat32(o + 16, true);
  }
  const deaths = [];
  for (let k = 0, o = 32 + n * CELL_STRIDE; k < nd; k++, o += 16) {
    deaths.push({
      x: dv.getFloat32(o, true), y: dv.getFloat32(o + 4, true), r: dv.getFloat32(o + 8, true),
      c: [dv.getUint8(o + 12) / 255, dv.getUint8(o + 13) / 255, dv.getUint8(o + 14) / 255],
    });
  }
  const index = new Map();
  for (let i = 0; i < n; i++) index.set(ids[i], i);
  return { n, time, step, daylight, ids, pos, deaths, u8: new Uint8Array(buf), index, arrived: performance.now() };
}

function parseGrid(buf) {
  const dv = new DataView(buf);
  const nchan = dv.getUint8(1);
  const gw = dv.getUint16(2, true);
  const gh = dv.getUint16(4, true);
  const daylight = dv.getFloat32(8, true);
  const scales = [];
  let o = 12;
  for (let s = 0; s < nchan; s++, o += 4) scales.push(dv.getFloat32(o, true));
  const vmax = dv.getFloat32(o, true);
  o += 4;
  const n = gw * gh;
  const planes = [];
  for (let s = 0; s < nchan; s++, o += n) planes.push(new Uint8Array(buf, o, n));
  const fu = new Int8Array(buf, o, n);
  const fv = new Int8Array(buf, o + n, n);
  return { gw, gh, daylight, scales, planes, flow: { u: fu, v: fv, vmax } };
}

function parseStatic(buf) {
  const dv = new DataView(buf);
  const gw = dv.getUint16(2, true);
  const gh = dv.getUint16(4, true);
  return { gw, gh, rgba: new Uint8Array(buf, 8, gw * gh * 4) };
}
