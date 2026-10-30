// WebGL2 renderer: seafloor background, chemical overlays, drifting
// particles, death bursts and translucent soft-bodied cells.

import { CELL_STRIDE } from './net.js';

const FLOATS = 40; // per cell instance

const NOISE = `
float hash12(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * .1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p); vec2 u = f * f * (3. - 2. * f);
  return mix(mix(hash12(i), hash12(i + vec2(1, 0)), u.x), mix(hash12(i + vec2(0, 1)), hash12(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) { float s = 0., a = .5; for (int i = 0; i < 4; i++) { s += a * vnoise(p); p = p * 2.03 + 17.1; a *= .5; } return s; }
`;

const BG_VS = `#version 300 es
in vec2 a_pos;
void main() { gl_Position = vec4(a_pos, 0., 1.); }`;

const BG_FS = `#version 300 es
precision highp float;
uniform vec2 u_res; uniform vec3 u_cam; uniform vec2 u_world;
uniform float u_time, u_day, u_wavePeriod, u_ovOn, u_ovRamp;
uniform vec3 u_light; uniform vec3 u_ovColor;
uniform sampler2D u_static, u_overlay;
uniform vec3 u_vents[16]; uniform int u_nvents;
out vec4 o;
${NOISE}
float caustic(vec2 p, float t) {
  float a = vnoise(p + vec2(t * .6, t * .4));
  float b = vnoise(p * 1.37 - vec2(t * .5, -t * .3) + 5.2);
  float c = 1. - abs(a - b);
  return pow(c, 9.);
}
vec3 ramp(float v) {
  if (u_ovRamp < .5) return mix(vec3(.02, .02, .05), u_ovColor, v) + vec3(pow(v, 4.) * .35);
  if (u_ovRamp < 1.5) { // temperature: cold blue -> warm red -> white hot
    vec3 a = vec3(.1, .25, .85), b = vec3(.2, .8, .7), c = vec3(1., .75, .2), d = vec3(1., .2, .1);
    return v < .33 ? mix(a, b, v / .33) : v < .66 ? mix(b, c, (v - .33) / .33) : mix(c, d, (v - .66) / .34);
  }
  if (u_ovRamp < 2.5) return mix(vec3(.02, .02, .04), vec3(1., .93, .6), v); // light
  return mix(vec3(.05, .1, .2), vec3(.6, .9, 1.), v); // currents
}
void main() {
  vec2 frag = gl_FragCoord.xy;
  vec2 wp = u_cam.xy + vec2(frag.x - u_res.x * .5, u_res.y * .5 - frag.y) / u_cam.z;
  vec2 uv = wp / u_world;
  vec2 cuv = clamp(uv, vec2(0.), vec2(1.));
  bool inside = uv.x >= 0. && uv.y >= 0. && uv.x <= 1. && uv.y <= 1.;
  vec4 st = texture(u_static, cuv);
  float tot = st.r + st.g + st.b + 1e-4;
  vec3 z = st.rgb / tot;
  vec3 deepC = vec3(.012, .036, .085), shalC = vec3(.025, .15, .19), beachC = vec3(.25, .225, .17);
  vec3 col = deepC * z.x + shalC * z.y + beachC * z.z;
  float n = fbm(wp / 22.);
  col *= .78 + .42 * n;
  float zoomDetail = smoothstep(3., 12., u_cam.z);
  col *= 1. + (fbm(wp / 2.2) - .5) * .35 * zoomDetail;
  col += (hash12(floor(wp * 3.)) - .5) * .025 * zoomDetail;
  float grain = hash12(floor(wp * 1.5));
  col += z.z * (grain - .5) * .06;
  float light = dot(z, u_light) * u_day;
  float cz = caustic(wp / 38., u_time * .25) + .6 * caustic(wp / 17. + 3.1, u_time * .33);
  col += vec3(.45, .8, .85) * cz * light * .10 * (z.y + z.z * .7);
  col *= .62 + .55 * light;
  float wph = u_time * 6.2831 / u_wavePeriod - wp.x / 90.;
  float foam = smoothstep(.86, 1., sin(wph + fbm(wp / 30.) * 2.5)) * z.z;
  col += vec3(.75, .88, .95) * foam * .16;
  col += vec3(.5, .7, .75) * z.z * (sin(wph) * .5 + .5) * .05;
  float h = st.a;
  col += vec3(1., .42, .12) * pow(h, 2.5) * .5 * (.85 + .15 * sin(u_time * 2.3 + wp.y * .2));
  for (int i = 0; i < 16; i++) {
    if (i >= u_nvents) break;
    vec3 v = u_vents[i];
    float d = length(wp - v.xy);
    if (d > v.z * 4.5) continue;
    float rough = fbm((wp - v.xy) / 4. + float(i) * 7.) * .5;
    float rock = smoothstep(v.z * 1.25, v.z * .7, d * (1. + rough * .4));
    col = mix(col, vec3(.05, .04, .045) + vec3(.08, .05, .04) * rough, rock * .85);
    float core = smoothstep(v.z * .55, 0., d);
    col += vec3(1., .5, .18) * core * (.7 + .3 * sin(u_time * 4. + float(i)));
    float plume = fbm((wp - v.xy) / 6. - vec2(0., u_time * .6)) * smoothstep(v.z * 4., v.z * .5, d);
    col += vec3(.9, .55, .3) * plume * .08;
  }
  if (u_ovOn > .5) {
    float val = texture(u_overlay, cuv).r;
    float a = (u_ovRamp > .5 && u_ovRamp < 2.5) ? .72 : smoothstep(0., .06, val) * .8;
    col = mix(col, ramp(val), a);
  }
  if (!inside) {
    vec2 out_d = max(-uv, uv - 1.) * u_world;
    float dist = max(out_d.x, out_d.y);
    vec3 land = vec3(.27, .24, .18) * (.8 + .3 * fbm(wp / 15.));
    if (uv.x > 1.) col = mix(col, land, smoothstep(0., 30., out_d.x));
    col *= mix(1., .35, smoothstep(0., 60., dist));
  }
  vec2 q = frag / u_res - .5;
  col *= 1. - dot(q, q) * .55;
  o = vec4(col, 1.);
}`;

const CELL_VS = `#version 300 es
in vec2 a_corner;
in vec4 i_pos; in vec4 i_s0; in vec4 i_s1; in vec4 i_s2; in vec4 i_s3;
in vec4 i_a; in vec4 i_b; in vec4 i_c; in vec4 i_d; in vec4 i_e;
uniform vec2 u_res; uniform vec3 u_cam; uniform float u_detail;
out vec4 v_local;
flat out vec4 f_s0, f_s1, f_s2, f_s3, f_a, f_b, f_c, f_d, f_e;
void main() {
  float r = max(i_pos.z, .3);
  float px = r * u_cam.z;
  bool flag = i_b.x > .01 && u_detail > .5 && px > 4.;
  float ext = flag ? 4.6 : (i_c.w > .5 ? 1.9 : 1.6);
  vec2 local = a_corner * ext;
  vec2 world = i_pos.xy + local * r;
  vec2 sp = (world - u_cam.xy) * u_cam.z;
  gl_Position = vec4(sp.x / (u_res.x * .5), -sp.y / (u_res.y * .5), 0., 1.);
  v_local = vec4(local, px, ext);
  f_s0 = i_s0; f_s1 = i_s1; f_s2 = i_s2; f_s3 = i_s3;
  f_a = i_a; f_b = i_b; f_c = i_c; f_d = i_d; f_e = vec4(i_pos.w, i_e.yzw);
}`;

const CELL_FS = `#version 300 es
precision highp float;
in vec4 v_local;
flat in vec4 f_s0, f_s1, f_s2, f_s3, f_a, f_b, f_c, f_d, f_e;
uniform float u_time, u_detail;
out vec4 o;
${NOISE}
float S(int k) {
  k = k & 15;
  vec4 v = k < 4 ? f_s0 : k < 8 ? f_s1 : k < 12 ? f_s2 : f_s3;
  int c = k & 3;
  return c == 0 ? v.x : c == 1 ? v.y : c == 2 ? v.z : v.w;
}
float shapeR(float ang, float seed, float dmg) {
  float f = ang / 6.2831853 * 16.;
  float fl = floor(f);
  float t = f - fl;
  int i = int(fl);
  float p0 = S(i - 1), p1 = S(i), p2 = S(i + 1), p3 = S(i + 2);
  float r = .5 * ((2. * p1) + (-p0 + p2) * t + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t + (-p0 + 3. * p1 - 3. * p2 + p3) * t * t * t);
  float tt = u_time;
  float wob = sin(2. * ang + tt * .9 + seed) * .5 + sin(3. * ang - tt * 1.3 + seed * 2.1) * .3 + sin(5. * ang + tt * 2.1 + seed * 3.7) * .2;
  return r * (1. + wob * (.022 + dmg * .05));
}
float smin(float a, float b, float k) { float h = clamp(.5 + .5 * (b - a) / k, 0., 1.); return mix(b, a, h) - k * h * (1. - h); }
vec2 rot(vec2 p, float a) { float c = cos(a), s = sin(a); return vec2(c * p.x + s * p.y, -s * p.x + c * p.y); }

void main() {
  vec2 p = v_local.xy;
  float px = v_local.z;
  vec3 col = f_c.rgb;
  float hl = f_c.w;
  float divp = f_a.x, axis = f_a.y, energy = f_a.z, dmg = f_a.w;
  float motor = f_b.x, photo = f_b.y, genes = f_b.z, memp = f_b.w;
  float rep = f_d.x, store = f_d.y, ros = f_d.z, seed = f_d.w;
  float heading = f_e.x;

  // Far away: a soft luminous speck.
  if (px < 2.6) {
    float d = length(p);
    float a = smoothstep(1.3, .2, d) * .9;
    o = vec4(col * a * 1.15, a);
    return;
  }

  float ang = atan(p.y, p.x); if (ang < 0.) ang += 6.2831853;
  float d = length(p) - shapeR(ang, seed, dmg);
  vec2 ax = vec2(cos(axis), sin(axis));
  if (divp > 0.) {
    float rl = mix(1., .7071, divp);
    float off = divp * .7071 * .95;
    vec2 p1 = p - ax * off, p2 = p + ax * off;
    float a1 = atan(p1.y, p1.x); if (a1 < 0.) a1 += 6.2831853;
    float a2 = atan(p2.y, p2.x); if (a2 < 0.) a2 += 6.2831853;
    float d1 = length(p1) - rl * shapeR(a1, seed + 1.3, dmg);
    float d2 = length(p2) - rl * shapeR(a2, seed + 2.7, dmg);
    float du = smin(d1, d2, mix(.6, .05, divp));
    d = mix(d, du, smoothstep(0., .1, divp));
  }
  float aa = 1.2 / px;                      // one pixel in radius units
  float inside = smoothstep(aa, -aa, d);
  vec3 c = vec3(0.); float a = 0.;

  if (inside > 0.) {
    // Cytoplasm: translucent, granular, slowly streaming.
    float gran = vnoise(p * 7. + vec2(seed * 3., u_time * .12)) * .6 + vnoise(p * 15. - vec2(u_time * .08, seed)) * .4;
    float cyt = (.13 + .1 * energy) * (.75 + .5 * gran);
    vec3 cc = mix(col, col * vec3(1.15, .7, .7), ros * .8) * (1. - dmg * .45);
    c += cc * cyt; a += cyt;
    // Inner glow just under the membrane.
    float glow = exp(d / .22) * (.18 + .25 * energy);
    c += mix(cc, vec3(1.), .25) * glow * .6; a += glow * .35;

    if (px > 7.) {
      // Nucleoid(s): the DNA, splitting in two as replication completes.
      float sep = smoothstep(.55, 1., rep) * .3 + max(divp, 0.) * .38;
      float nr = .22 + genes * .16;
      float nn = 1e3;
      for (int k = 0; k < 2; k++) {
        vec2 cpos = (k == 0 ? 1. : -1.) * ax * sep;
        vec2 q = p - cpos;
        float wob = fbm(q * 4. + seed + float(k) * 3.1 + u_time * .05) * .14;
        nn = min(nn, length(q) - nr * (sep > .02 ? .8 : 1.) - wob);
      }
      float nuc = smoothstep(.06, -.04, nn);
      float fib = .5 + .5 * sin((p.x * 31. + p.y * 23.) + fbm(p * 9. + seed) * 9.);
      vec3 ncol = mix(col, vec3(.85, .8, 1.), .35) * (.55 + .45 * fib);
      c = mix(c, ncol * .45, nuc * .65); a = mix(a, .42, nuc * .55);

      // Storage granules.
      float ng = floor(store * 9.);
      for (int k = 0; k < 9; k++) {
        if (float(k) >= ng) break;
        float h1 = hash12(vec2(seed * 13.1, float(k) * 7.7)), h2 = hash12(vec2(float(k) * 3.3, seed * 5.9));
        vec2 gp = vec2(cos(h1 * 6.283), sin(h1 * 6.283)) * (.35 + h2 * .4);
        if (divp > 0.) {
          // Granules ride along into whichever daughter lobe they're nearer.
          float side = dot(gp, ax) >= 0. ? 1. : -1.;
          gp = gp * mix(1., .62, divp) + ax * side * divp * .67;
        }
        float gd = length(p - gp) - (.055 + h2 * .03);
        float g = smoothstep(aa * 1.5, -aa, gd) * inside;
        c = mix(c, vec3(1., .93, .7) * .85, g * .8); a = max(a, g * .75);
      }
      // Photosynthetic membranes: concentric lamellae near the edge.
      if (photo > .02) {
        float lam = smoothstep(.55, .95, .5 + .5 * sin(d * 70.)) * smoothstep(-.42, -.18, d) * smoothstep(-.04, -.12, d);
        vec3 pcol = col.g > col.r ? vec3(.35, 1., .45) : vec3(.95, .5, .55);
        c += pcol * lam * photo * .5; a += lam * photo * .3;
      }
    }
  }

  // Membrane: a bright lipid rim, resolving into a bilayer up close.
  float rimW = max(.07, 1.4 / px);
  float rim = smoothstep(rimW, 0., abs(d + rimW * .5));
  if (px > 55.) {
    float gap = .028;
    rim = max(smoothstep(.012 + .6 / px, 0., abs(d + gap * .4)), smoothstep(.012 + .6 / px, 0., abs(d + gap * 1.6)));
    rim = max(rim, smoothstep(rimW * 1.2, 0., abs(d + gap)) * .35);
  }
  vec3 rcol = mix(col, vec3(1.), .5) * (1. - dmg * .5);
  // Membrane proteins as tiny round beads along the rim.
  if (px > 20. && memp > .12) {
    float nprot = floor(10. + memp * 46.);
    float k = floor(ang / 6.2831853 * nprot + seed) + .5;
    float bang = (k - seed) / nprot * 6.2831853;
    float rr = length(p) - d - rimW * .45;
    vec2 bc = vec2(cos(bang), sin(bang)) * rr;
    float br = .032 + .9 / px;
    float bead = smoothstep(br, br * .45, length(p - bc));
    rcol = mix(rcol, vec3(1., .85, .45), bead * .85);
    rim = max(rim, bead * .9);
  }
  c += rcol * rim * .95; a += rim * .9;

  // Flagella: beating filaments trailing behind swimmers.
  if (motor > .01 && u_detail > .5 && px > 4.) {
    vec2 q = rot(p, heading);
    float nf = 1. + floor(min(motor, .99) * 3.);
    float flen = .8 + 2.6 * smoothstep(.0, .35, motor);
    float fvis = smoothstep(.0, .12, motor);
    float beat = u_time * (9. + motor * 26.);
    for (int k = 0; k < 3; k++) {
      if (float(k) >= nf) break;
      float yo = (float(k) - (nf - 1.) * .5) * .32;
      float x = -q.x - .9;                  // distance behind the cell edge
      if (x > 0. && x < flen) {
        float env = smoothstep(0., .5, x) * (.18 + .1 * motor);
        float y = yo + sin(x * 3.4 - beat + float(k) * 2.) * env;
        float slope = cos(x * 3.4 - beat) * 3.4 * env;
        float fd = abs(q.y - y) / sqrt(1. + slope * slope);
        float wdt = max(.028, .9 / px);
        float fl = smoothstep(wdt, 0., fd) * (1. - x / flen) * .8 * fvis;
        c += mix(col, vec3(1.), .4) * fl * .8; a += fl * .7;
      }
    }
  }

  // Hover / selection ring.
  if (hl > .5) {
    float ringR = .28 + .04 * sin(u_time * 4.);
    float ring = smoothstep(max(.04, 1.2 / px), 0., abs(d - ringR));
    vec3 hc = hl > 1.5 ? vec3(.37, .9, .82) : vec3(.8, .9, 1.);
    c += hc * ring * (hl > 1.5 ? .95 : .5); a += ring * (hl > 1.5 ? .8 : .4);
  }
  a = clamp(a, 0., 1.);
  o = vec4(min(c, vec3(1.5)), a);
}`;

const PT_VS = `#version 300 es
in vec3 a_p;
uniform vec2 u_res; uniform vec3 u_cam; uniform float u_dpr;
out float v_a;
void main() {
  vec2 sp = (a_p.xy - u_cam.xy) * u_cam.z;
  gl_Position = vec4(sp.x / (u_res.x * .5), -sp.y / (u_res.y * .5), 0., 1.);
  gl_PointSize = (1.2 + a_p.z * 1.8) * u_dpr * clamp(u_cam.z / u_dpr * .6, .7, 2.2);
  v_a = .1 + a_p.z * .22;
}`;
const PT_FS = `#version 300 es
precision mediump float;
in float v_a; out vec4 o;
void main() { float d = length(gl_PointCoord - .5); float a = smoothstep(.5, .1, d) * v_a; o = vec4(vec3(.75, .88, .92) * a, a); }`;

const BURST_VS = `#version 300 es
in vec2 a_corner; in vec4 i_b; in vec3 i_c;
uniform vec2 u_res; uniform vec3 u_cam;
out vec2 v_p; flat out vec4 f_b; flat out vec3 f_c;
void main() {
  float ext = 3.2;
  vec2 world = i_b.xy + a_corner * i_b.z * ext;
  vec2 sp = (world - u_cam.xy) * u_cam.z;
  gl_Position = vec4(sp.x / (u_res.x * .5), -sp.y / (u_res.y * .5), 0., 1.);
  v_p = a_corner * ext; f_b = i_b; f_c = i_c;
}`;
const BURST_FS = `#version 300 es
precision highp float;
in vec2 v_p; flat in vec4 f_b; flat in vec3 f_c;
out vec4 o;
float hash(float n) { return fract(sin(n) * 43758.5453); }
void main() {
  float t = f_b.w;                 // 0..1 age
  float rr = 1. + t * 1.2;
  float d = length(v_p);
  float fade = (1. - t) * (1. - t);
  float ring = smoothstep(.08, 0., abs(d - rr)) * fade * .35;
  float a = ring;
  vec3 c = mix(f_c, vec3(1.), .4) * ring;
  for (int k = 0; k < 7; k++) {
    float an = hash(float(k) * 12.9 + f_b.x) * 6.283;
    vec2 dp = vec2(cos(an), sin(an)) * (.4 + t * (1.4 + hash(float(k) + f_b.y) * 1.2));
    float fd = length(v_p - dp) - .08 * (1. - t);
    float f = smoothstep(.05, -.02, fd) * fade;
    c += f_c * f * .5; a += f * .35;
  }
  o = vec4(c, clamp(a, 0., 1.));
}`;

function compile(gl, type, src) {
  const s = gl.createShader(type);
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(s);
    console.error(log, src.split('\n').map((l, i) => `${i + 1}: ${l}`).join('\n'));
    throw new Error('shader compile failed: ' + log);
  }
  return s;
}
function program(gl, vs, fs) {
  const p = gl.createProgram();
  gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, vs));
  gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  const u = {};
  const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < n; i++) {
    const info = gl.getActiveUniform(p, i);
    const name = info.name.replace(/\[0\]$/, '');
    u[name] = gl.getUniformLocation(p, info.name);
  }
  return { p, u };
}

function hsl(h, s, l) {
  const k = (n) => (n + h * 12) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0), f(8), f(4)];
}
const VIRIDIS = [[0.27, 0.0, 0.33], [0.23, 0.32, 0.55], [0.13, 0.57, 0.55], [0.37, 0.79, 0.38], [0.99, 0.91, 0.14]];
export function rampColor(v, stops = VIRIDIS) {
  v = Math.min(1, Math.max(0, v)) * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(v));
  const t = v - i;
  return stops[i].map((x, k) => x + (stops[i + 1][k] - x) * t);
}

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    const gl = canvas.getContext('webgl2', { antialias: false, alpha: false, premultipliedAlpha: true, powerPreference: 'high-performance' });
    if (!gl) throw new Error('WebGL2 is not available in this browser.');
    this.gl = gl;
    this.cam = { x: 480, y: 300, z: 1 };
    this.camTarget = { x: 480, y: 300, z: 1 };
    this.time = 0;
    this.world = { w: 960, h: 600, gw: 1, gh: 1 };
    this.vents = [];
    this.light = [0.06, 0.75, 1.0];
    this.wavePeriod = 7;
    this.detail = true;
    this.particlesOn = true;
    this.burstsOn = true;
    this.overlay = null; // {ramp, color}
    this.bursts = [];
    this.instances = new Float32Array(FLOATS * 1024);
    this.instanceCount = 0;
    this.visible = [];

    this.bg = program(gl, BG_VS, BG_FS);
    this.cell = program(gl, CELL_VS, CELL_FS);
    this.pt = program(gl, PT_VS, PT_FS);
    this.burst = program(gl, BURST_VS, BURST_FS);

    this.triBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.triBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    this.quadBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    this.bgVao = gl.createVertexArray();
    gl.bindVertexArray(this.bgVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.triBuf);
    const lp = gl.getAttribLocation(this.bg.p, 'a_pos');
    gl.enableVertexAttribArray(lp);
    gl.vertexAttribPointer(lp, 2, gl.FLOAT, false, 0, 0);

    this.instBuf = gl.createBuffer();
    this.cellVao = gl.createVertexArray();
    gl.bindVertexArray(this.cellVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    const lc = gl.getAttribLocation(this.cell.p, 'a_corner');
    gl.enableVertexAttribArray(lc);
    gl.vertexAttribPointer(lc, 2, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instBuf);
    const names = ['i_pos', 'i_s0', 'i_s1', 'i_s2', 'i_s3', 'i_a', 'i_b', 'i_c', 'i_d', 'i_e'];
    names.forEach((nm, k) => {
      const l = gl.getAttribLocation(this.cell.p, nm);
      if (l < 0) return;
      gl.enableVertexAttribArray(l);
      gl.vertexAttribPointer(l, 4, gl.FLOAT, false, FLOATS * 4, k * 16);
      gl.vertexAttribDivisor(l, 1);
    });

    this.ptBuf = gl.createBuffer();
    this.ptVao = gl.createVertexArray();
    gl.bindVertexArray(this.ptVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.ptBuf);
    const lpp = gl.getAttribLocation(this.pt.p, 'a_p');
    gl.enableVertexAttribArray(lpp);
    gl.vertexAttribPointer(lpp, 3, gl.FLOAT, false, 0, 0);

    this.burstBuf = gl.createBuffer();
    this.burstVao = gl.createVertexArray();
    gl.bindVertexArray(this.burstVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    const lb = gl.getAttribLocation(this.burst.p, 'a_corner');
    gl.enableVertexAttribArray(lb);
    gl.vertexAttribPointer(lb, 2, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.burstBuf);
    const lbi = gl.getAttribLocation(this.burst.p, 'i_b');
    const lbc = gl.getAttribLocation(this.burst.p, 'i_c');
    gl.enableVertexAttribArray(lbi);
    gl.vertexAttribPointer(lbi, 4, gl.FLOAT, false, 28, 0);
    gl.vertexAttribDivisor(lbi, 1);
    gl.enableVertexAttribArray(lbc);
    gl.vertexAttribPointer(lbc, 3, gl.FLOAT, false, 28, 16);
    gl.vertexAttribDivisor(lbc, 1);
    gl.bindVertexArray(null);

    this.staticTex = this.makeTex(gl.RGBA8, 1, 1, gl.RGBA, new Uint8Array([255, 0, 0, 0]));
    this.overlayTex = this.makeTex(gl.R8, 1, 1, gl.RED, new Uint8Array([0]));
    this.particles = null;
    this.resize();
  }

  makeTex(internal, w, h, format, data) {
    const gl = this.gl;
    const t = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, internal, w, h, 0, format, gl.UNSIGNED_BYTE, data);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return t;
  }

  setWorld(info) {
    this.world = { ...info.world };
    this.vents = info.vents;
    this.initParticles();
  }

  setStatic(st) {
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.staticTex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, st.gw, st.gh, 0, gl.RGBA, gl.UNSIGNED_BYTE, st.rgba);
  }

  setOverlay(data, gw, gh, rampKind, color) {
    const gl = this.gl;
    if (!data) { this.overlay = null; return; }
    gl.bindTexture(gl.TEXTURE_2D, this.overlayTex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, gw, gh, 0, gl.RED, gl.UNSIGNED_BYTE, data);
    this.overlay = { ramp: rampKind, color };
  }

  initParticles() {
    const n = Math.round(Math.min(6000, this.world.w * this.world.h / 180));
    this.particles = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) {
      this.particles[i * 3] = Math.random() * this.world.w;
      this.particles[i * 3 + 1] = Math.random() * this.world.h;
      this.particles[i * 3 + 2] = Math.random();
    }
  }

  stepParticles(dt, flow) {
    if (!this.particles || !flow) return;
    const { w, h } = this.world;
    const gw = flow.gw, gh = flow.gh, dx = w / gw;
    const k = flow.vmax / 127;
    const P = this.particles;
    for (let i = 0; i < P.length; i += 3) {
      let x = P[i], y = P[i + 1];
      const gx = Math.min(gw - 1, Math.max(0, (x / dx) | 0));
      const gy = Math.min(gh - 1, Math.max(0, (y / dx) | 0));
      const gi = gy * gw + gx;
      const jitter = 0.6 + P[i + 2];
      x += (flow.u[gi] * k * jitter + (Math.random() - 0.5) * 3) * dt;
      y += (flow.v[gi] * k * jitter + (Math.random() - 0.5) * 3) * dt;
      if (x < 0) x += w; else if (x > w) x -= w;
      if (y < 0) y += h; else if (y > h) y -= h;
      P[i] = x; P[i + 1] = y;
    }
  }

  addBursts(deaths) {
    if (!this.burstsOn) return;
    const now = this.time;
    for (const d of deaths) {
      if (this.bursts.length > 600) break;
      this.bursts.push({ x: d.x, y: d.y, r: d.r, c: d.c, t0: now });
    }
  }

  resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.dpr = dpr;
    const w = Math.floor(this.canvas.clientWidth * dpr);
    const h = Math.floor(this.canvas.clientHeight * dpr);
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  fitWorld(marginLeft = 260, marginRight = this.fitMarginRight ?? 410, marginTop = 80) {
    const cw = this.canvas.clientWidth - marginLeft - marginRight;
    const ch = this.canvas.clientHeight - marginTop - 20;
    const z = Math.max(0.2, Math.min(cw / this.world.w, ch / this.world.h) * 0.97);
    const cx = this.world.w / 2 - (marginLeft - marginRight) / 2 / z;
    const cy = this.world.h / 2 - (marginTop - 20) / 2 / z;
    this.camTarget = { x: cx, y: cy, z };
  }

  screenToWorld(sx, sy) {
    const c = this.cam;
    return [c.x + (sx - this.canvas.clientWidth / 2) / c.z, c.y + (sy - this.canvas.clientHeight / 2) / c.z];
  }
  worldToScreen(wx, wy) {
    const c = this.cam;
    return [(wx - c.x) * c.z + this.canvas.clientWidth / 2, (wy - c.y) * c.z + this.canvas.clientHeight / 2];
  }

  /// Build per-instance data, interpolating between two frames.
  buildInstances(prev, cur, alpha, colorFn, highlight) {
    if (!cur) { this.instanceCount = 0; return; }
    const n = cur.n;
    if (this.instances.length < n * FLOATS) this.instances = new Float32Array(Math.ceil(n * 1.5) * FLOATS);
    const I = this.instances;
    const u8 = cur.u8;
    const pu8 = prev ? prev.u8 : null;
    const halfW = this.canvas.clientWidth / 2 / this.cam.z, halfH = this.canvas.clientHeight / 2 / this.cam.z;
    const minX = this.cam.x - halfW - 30, maxX = this.cam.x + halfW + 30;
    const minY = this.cam.y - halfH - 30, maxY = this.cam.y + halfH + 30;
    let m = 0;
    const vis = this.visible;
    vis.length = 0;
    const TAU = Math.PI * 2;
    for (let i = 0; i < n; i++) {
      const o = 32 + i * CELL_STRIDE;
      let x = cur.pos[i * 4], y = cur.pos[i * 4 + 1], r = cur.pos[i * 4 + 2], hd = cur.pos[i * 4 + 3];
      let pj = -1;
      if (prev) {
        const j = prev.index.get(cur.ids[i]);
        if (j !== undefined) {
          pj = j;
          const px = prev.pos[j * 4], py = prev.pos[j * 4 + 1];
          // Skip interpolation across big jumps (e.g. after a world reset).
          if (Math.abs(px - x) < 40 && Math.abs(py - y) < 40) {
            x = px + (x - px) * alpha;
            y = py + (y - py) * alpha;
            r = prev.pos[j * 4 + 2] + (r - prev.pos[j * 4 + 2]) * alpha;
            let ph = prev.pos[j * 4 + 3];
            let dh = ((hd - ph) % TAU + TAU * 1.5) % TAU - TAU * 0.5;
            hd = ph + dh * alpha;
          }
        }
      }
      if (x < minX || x > maxX || y < minY || y > maxY) continue;
      vis.push(i, x, y, r);
      const b = m * FLOATS;
      I[b] = x; I[b + 1] = y; I[b + 2] = r; I[b + 3] = hd;
      const po = pj >= 0 ? 32 + pj * CELL_STRIDE : -1;
      for (let k = 0; k < 16; k++) {
        let s = u8[o + 20 + k];
        if (po >= 0) s = pu8[po + 20 + k] + (s - pu8[po + 20 + k]) * alpha;
        I[b + 4 + k] = s / 100;
      }
      let dv = u8[o + 36];
      if (po >= 0 && dv > 0 && pu8[po + 36] > 0) dv = pu8[po + 36] + (dv - pu8[po + 36]) * alpha;
      I[b + 20] = dv > 0 ? (dv - 1) / 254 : -1;
      I[b + 21] = u8[o + 37] / 255 * TAU;
      I[b + 22] = u8[o + 38] / 255;
      I[b + 23] = u8[o + 39] / 255;
      I[b + 24] = u8[o + 40] / 255;
      I[b + 25] = u8[o + 41] / 255;
      I[b + 26] = Math.min(1, u8[o + 42] / 24);
      I[b + 27] = u8[o + 43] / 255;
      const c = colorFn(u8, o);
      I[b + 28] = c[0]; I[b + 29] = c[1]; I[b + 30] = c[2];
      I[b + 31] = cur.ids[i] === highlight.selected ? 2 : cur.ids[i] === highlight.hover ? 1 : 0;
      I[b + 32] = u8[o + 48] / 255;
      I[b + 33] = u8[o + 49] / 255;
      I[b + 34] = u8[o + 50] / 255;
      I[b + 35] = (cur.ids[i] % 997) * 0.731;
      m++;
    }
    this.instanceCount = m;
  }

  draw(dtReal, running, gridFlow) {
    const gl = this.gl;
    this.resize();
    // Smooth camera.
    const k = 1 - Math.pow(0.0001, dtReal);
    const c = this.cam, t = this.camTarget;
    const zl = Math.log(c.z) + (Math.log(t.z) - Math.log(c.z)) * k;
    c.z = Math.exp(zl);
    c.x += (t.x - c.x) * k;
    c.y += (t.y - c.y) * k;
    if (running) this.time += dtReal;
    if (running && this.particlesOn) this.stepParticles(Math.min(dtReal, 0.05), gridFlow);

    const W = this.canvas.width, H = this.canvas.height, dpr = this.dpr;
    gl.viewport(0, 0, W, H);
    const camU = [c.x, c.y, c.z * dpr];

    // Background.
    gl.disable(gl.BLEND);
    gl.useProgram(this.bg.p);
    const u = this.bg.u;
    gl.uniform2f(u.u_res, W, H);
    gl.uniform3f(u.u_cam, ...camU);
    gl.uniform2f(u.u_world, this.world.w, this.world.h);
    gl.uniform1f(u.u_time, this.time);
    gl.uniform1f(u.u_day, this.daylight ?? 1);
    gl.uniform1f(u.u_wavePeriod, this.wavePeriod);
    gl.uniform3f(u.u_light, ...this.light);
    gl.uniform1f(u.u_ovOn, this.overlay ? 1 : 0);
    gl.uniform1f(u.u_ovRamp, this.overlay ? this.overlay.ramp : 0);
    gl.uniform3f(u.u_ovColor, ...(this.overlay ? this.overlay.color : [1, 1, 1]));
    const vv = new Float32Array(48);
    const nv = Math.min(16, this.vents.length);
    for (let i = 0; i < nv; i++) { vv[i * 3] = this.vents[i].x; vv[i * 3 + 1] = this.vents[i].y; vv[i * 3 + 2] = this.ventRadius ?? 10; }
    gl.uniform3fv(u.u_vents, vv);
    gl.uniform1i(u.u_nvents, nv);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.staticTex);
    gl.uniform1i(u.u_static, 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.overlayTex);
    gl.uniform1i(u.u_overlay, 1);
    gl.bindVertexArray(this.bgVao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);

    // Drifting particles.
    if (this.particlesOn && this.particles) {
      gl.useProgram(this.pt.p);
      gl.uniform2f(this.pt.u.u_res, W, H);
      gl.uniform3f(this.pt.u.u_cam, ...camU);
      gl.uniform1f(this.pt.u.u_dpr, dpr);
      gl.bindVertexArray(this.ptVao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.ptBuf);
      gl.bufferData(gl.ARRAY_BUFFER, this.particles, gl.DYNAMIC_DRAW);
      gl.drawArrays(gl.POINTS, 0, this.particles.length / 3);
    }

    // Death bursts.
    const life = 0.9;
    this.bursts = this.bursts.filter((b) => this.time - b.t0 < life);
    if (this.bursts.length) {
      const data = new Float32Array(this.bursts.length * 7);
      this.bursts.forEach((b, i) => {
        data.set([b.x, b.y, b.r, (this.time - b.t0) / life, b.c[0], b.c[1], b.c[2]], i * 7);
      });
      gl.useProgram(this.burst.p);
      gl.uniform2f(this.burst.u.u_res, W, H);
      gl.uniform3f(this.burst.u.u_cam, ...camU);
      gl.bindVertexArray(this.burstVao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.burstBuf);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.bursts.length);
    }

    // Cells.
    if (this.instanceCount > 0) {
      gl.useProgram(this.cell.p);
      gl.uniform2f(this.cell.u.u_res, W, H);
      gl.uniform3f(this.cell.u.u_cam, ...camU);
      gl.uniform1f(this.cell.u.u_time, this.time);
      gl.uniform1f(this.cell.u.u_detail, this.detail ? 1 : 0);
      gl.bindVertexArray(this.cellVao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.instBuf);
      gl.bufferData(gl.ARRAY_BUFFER, this.instances.subarray(0, this.instanceCount * FLOATS), gl.DYNAMIC_DRAW);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.instanceCount);
    }
    gl.bindVertexArray(null);
  }

  /// Nearest visible cell to a screen point (within its radius + slack).
  pick(sx, sy) {
    const [wx, wy] = this.screenToWorld(sx, sy);
    const v = this.visible;
    let best = -1, bd = Infinity;
    const slack = 6 / this.cam.z;
    for (let k = 0; k < v.length; k += 4) {
      const dx = v[k + 1] - wx, dy = v[k + 2] - wy;
      const d = Math.sqrt(dx * dx + dy * dy) - v[k + 3];
      if (d < slack && d < bd) { bd = d; best = v[k]; }
    }
    return best;
  }

  static lineageColor(hue) { return hsl(hue, 0.75, 0.6); }
}
