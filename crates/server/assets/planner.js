// Practice planner behaviour: the practice builder's drill library (drag and
// drop) and the drill diagram editor. Both are enhancements: without
// JavaScript, stations are filled with a <select> + "Place" button and the
// diagram is edited as JSON.
"use strict";

// ---- Practice builder: drill library -----------------------------------------
//
// Drag a drill from the library onto a station (a `.drop-slot` in a rink
// diagram) it fits in (`data-accepts` lists the drill sizes), or click a drill and then a station. Placing
// posts the same form the no-JavaScript "Place" button does.

function filterLibrary(library) {
  const query = library.querySelector("[data-library-search]").value.trim().toLowerCase();
  const size = library.querySelector("[data-library-size]").value;
  for (const chip of library.querySelectorAll(".drill-chip")) {
    chip.hidden = !(chip.dataset.name.includes(query) && (!size || chip.dataset.size === size));
  }
}

document.addEventListener("input", (event) => {
  const library = event.target.closest("[data-library]");
  if (library && event.target.matches("[data-library-search]")) {
    filterLibrary(library);
  }
});

document.addEventListener("change", (event) => {
  const library = event.target.closest("[data-library]");
  if (library && event.target.matches("[data-library-size]")) {
    filterLibrary(library);
  }
});

// The drill being dragged or picked by a click: { id, size, chip }.
let picked = null;

function showTargets(size) {
  for (const slot of document.querySelectorAll(".drop-slot")) {
    const accepts = (slot.dataset.accepts || slot.dataset.size || "").split(" ");
    slot.classList.toggle("can-drop", Boolean(size) && accepts.includes(size));
    slot.classList.remove("over");
  }
}

function pick(chip) {
  const same = picked && picked.chip === chip;
  for (const other of document.querySelectorAll(".drill-chip.armed")) {
    other.classList.remove("armed");
  }
  if (!chip || same) {
    picked = null;
    showTargets(null);
    return;
  }
  picked = { id: chip.dataset.drillId, size: chip.dataset.size, chip };
  chip.classList.add("armed");
  showTargets(picked.size);
}

// Post `fields` to `action` as the no-JavaScript form would. The form goes
// inside the board (so inside the builder column, which htmx updates in place).
function postFromBoard(wrap, action, fields) {
  const form = document.createElement("form");
  form.method = "post";
  form.action = action;
  form.hidden = true;
  for (const [name, value] of Object.entries(fields)) {
    const input = document.createElement("input");
    input.type = "hidden";
    input.name = name;
    input.value = value;
    form.append(input);
  }
  wrap.append(form);
  if (window.htmx) {
    htmx.process(form);
  }
  form.requestSubmit();
}

function placeDrill(slot, drillId) {
  const wrap = slot.closest("[data-assign-url]");
  if (!wrap || !drillId) {
    return;
  }
  pick(null);
  postFromBoard(wrap, wrap.dataset.assignUrl + slot.dataset.slot, { drill_id: drillId });
}

// ---- Practice builder: moving the goalie warm-up ------------------------------
//
// Drag the warm-up (two nets, goalies, coach) anywhere on the part's rink, and
// turn it: drag its round handle (15° steps, Shift to turn freely), or press
// R / Shift+R while moving it. It follows the pointer with a temporary
// transform; on release its new spot and angle are posted and the server
// draws it there.

let goalieDrag = null;

function rinkPoint(svg, event) {
  const pt = svg.createSVGPoint();
  pt.x = event.clientX;
  pt.y = event.clientY;
  return pt.matrixTransform(svg.getScreenCTM().inverse());
}

function previewGoalies(drag) {
  const x = Number(drag.spot.dataset.x);
  const y = Number(drag.spot.dataset.y);
  drag.spot.setAttribute(
    "transform",
    `translate(${drag.dx} ${drag.dy}) rotate(${drag.turn} ${x} ${y})`,
  );
}

document.addEventListener("pointerdown", (event) => {
  const spot = event.target.closest && event.target.closest(".board .goalie-spot");
  if (!spot || event.button !== 0 || !spot.closest("[data-goalie-url]")) {
    return;
  }
  event.preventDefault();
  const svg = spot.ownerSVGElement;
  const turning = Boolean(event.target.closest(".goalie-rot"));
  goalieDrag = { spot, svg, start: rinkPoint(svg, event), dx: 0, dy: 0, turn: 0, turning };
  spot.classList.add("dragging");
});

document.addEventListener("pointermove", (event) => {
  const drag = goalieDrag;
  if (!drag) {
    return;
  }
  const p = rinkPoint(drag.svg, event);
  if (drag.turning) {
    // Point the warm-up at the pointer.
    const x = Number(drag.spot.dataset.x);
    const y = Number(drag.spot.dataset.y);
    const step = event.shiftKey ? 1 : 15;
    const angle = (Math.atan2(p.y - y, p.x - x) * 180) / Math.PI;
    drag.turn = Math.round(angle / step) * step - Number(drag.spot.dataset.rot);
  } else {
    drag.dx = p.x - drag.start.x;
    drag.dy = p.y - drag.start.y;
  }
  previewGoalies(drag);
});

// R / Shift+R turn the warm-up while it's being moved.
document.addEventListener("keydown", (event) => {
  if (!goalieDrag || event.key.toLowerCase() !== "r") {
    return;
  }
  event.preventDefault();
  goalieDrag.turn += event.shiftKey ? -15 : 15;
  previewGoalies(goalieDrag);
});

function endGoalieDrag(commit) {
  const drag = goalieDrag;
  goalieDrag = null;
  if (!drag) {
    return;
  }
  drag.spot.classList.remove("dragging");
  const moved = Math.hypot(drag.dx, drag.dy) >= 1;
  const turned = Math.round(drag.turn) % 360 !== 0;
  if (!commit || (!moved && !turned)) {
    drag.spot.removeAttribute("transform");
    return;
  }
  // Keep the spot on the visible ice.
  const vb = drag.svg.viewBox.baseVal;
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const x = clamp(Number(drag.spot.dataset.x) + drag.dx, vb.x + 2, vb.x + vb.width - 2);
  const y = clamp(Number(drag.spot.dataset.y) + drag.dy, vb.y + 2, vb.y + vb.height - 2);
  const rot = Math.round(Number(drag.spot.dataset.rot) + drag.turn);
  const wrap = drag.spot.closest("[data-goalie-url]");
  postFromBoard(wrap, wrap.dataset.goalieUrl, { x: x.toFixed(1), y: y.toFixed(1), rot });
}

document.addEventListener("pointerup", () => endGoalieDrag(true));
document.addEventListener("pointercancel", () => endGoalieDrag(false));

document.addEventListener("dragstart", (event) => {
  const chip = event.target.closest && event.target.closest(".drill-chip");
  if (!chip) {
    return;
  }
  event.dataTransfer.setData("text/plain", chip.dataset.drillId);
  event.dataTransfer.effectAllowed = "copy";
  picked = { id: chip.dataset.drillId, size: chip.dataset.size, chip };
  showTargets(chip.dataset.size);
});

document.addEventListener("dragend", () => {
  pick(null);
});

document.addEventListener("dragover", (event) => {
  const slot = event.target.closest && event.target.closest(".drop-slot.can-drop");
  if (slot) {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    slot.classList.add("over");
  }
});

document.addEventListener("dragleave", (event) => {
  const slot = event.target.closest && event.target.closest(".drop-slot");
  if (slot) {
    slot.classList.remove("over");
  }
});

document.addEventListener("drop", (event) => {
  const slot = event.target.closest && event.target.closest(".drop-slot.can-drop");
  if (slot) {
    event.preventDefault();
    placeDrill(slot, event.dataTransfer.getData("text/plain") || (picked && picked.id));
  }
});

document.addEventListener("click", (event) => {
  const chip = event.target.closest(".drill-chip");
  if (chip) {
    pick(chip);
    return;
  }
  const slot = event.target.closest(".drop-slot");
  if (slot && picked) {
    if (slot.classList.contains("can-drop")) {
      placeDrill(slot, picked.id);
    }
    return;
  }
  if (picked && !event.target.closest("[data-library]")) {
    pick(null);
  }
});

document.addEventListener("keydown", (event) => {
  const chip = event.target.closest && event.target.closest(".drill-chip");
  if (chip && (event.key === "Enter" || event.key === " ")) {
    event.preventDefault();
    pick(chip);
  } else if (event.key === "Escape" && picked) {
    pick(null);
  }
});

document.addEventListener("htmx:beforeSwap", () => {
  picked = null;
});

// ---- Drill diagram editor ------------------------------------------------------
//
// Edits the diagram JSON in the form's [data-diagram-json] textarea. Drawing
// mirrors `crates/core/src/diagram.rs`; keep the two in step.

const SVG_NS = "http://www.w3.org/2000/svg";
const INK = "#151515";
const RED = "#d0312d";
const ICE = "#ffffff";
const NET = "#f4f6f8";
const NET_MESH = "#9aa4ae";
const CONE = "#f08a24";
const BARRIER = "#f2c230";
const DANGLER = "#7b5cd6";
const STEP = 0.5;

// Items that can be turned (their `rot` is the direction they face).
const ROTATABLE = new Set(["net", "barrier", "dangler"]);

function num(v) {
  return String(Math.round(v * 100) / 100);
}

function escapeXml(text) {
  return String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function svgText(x, y, size, weight, text, cls) {
  return (
    `<text class="${cls}" x="${num(x)}" y="${num(y)}" font-size="${num(size)}" ` +
    `font-weight="${weight}" fill="${INK}" text-anchor="middle">${escapeXml(text)}</text>`
  );
}

function pathD(pts) {
  return pts.map(([x, y], i) => `${i ? "L" : "M"} ${num(x)} ${num(y)}`).join(" ");
}

function segmentsD(segs) {
  return segs
    .map(([a, b]) => `M ${num(a[0])} ${num(a[1])} L ${num(b[0])} ${num(b[1])}`)
    .join(" ");
}

function resample(pts, step) {
  const out = [pts[0]];
  let carry = 0;
  for (let i = 0; i + 1 < pts.length; i++) {
    const [ax, ay] = pts[i];
    const [bx, by] = pts[i + 1];
    const len = Math.hypot(bx - ax, by - ay);
    if (len === 0) {
      continue;
    }
    let d = step - carry;
    while (d <= len) {
      const t = d / len;
      out.push([ax + (bx - ax) * t, ay + (by - ay) * t]);
      d += step;
    }
    carry = len - (d - step);
  }
  const last = pts[pts.length - 1];
  const end = out[out.length - 1];
  if (end[0] !== last[0] || end[1] !== last[1]) {
    out.push(last);
  }
  return out;
}

// Points along a Catmull-Rom curve through `pts`, every STEP ft.
function smooth(pts) {
  if (pts.length < 2) {
    return pts.slice();
  }
  const dense = [];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(i - 1, 0)];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[Math.min(i + 2, pts.length - 1)];
    for (let k = 0; k < 16; k++) {
      const t = k / 16;
      const t2 = t * t;
      const t3 = t2 * t;
      const f = (a, b, c, d) =>
        0.5 *
        (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
      dense.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
    }
  }
  dense.push(pts[pts.length - 1]);
  return resample(dense, STEP);
}

function normal(pts, i) {
  const a = pts[Math.max(i - 1, 0)];
  const b = pts[Math.min(i + 1, pts.length - 1)];
  const len = Math.max(Math.hypot(b[0] - a[0], b[1] - a[1]), 1e-6);
  return [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
}

// The point and normal at fractional index `s` of a sampled curve.
function at(pts, s) {
  const last = pts.length - 1;
  s = Math.min(Math.max(s, 0), last);
  const i = Math.min(Math.floor(s), Math.max(last - 1, 0));
  const t = s - i;
  const a = pts[i];
  const b = pts[Math.min(i + 1, last)];
  return [[a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], normal(pts, t < 0.5 ? i : i + 1)];
}

function offset(pts, by) {
  return pts.map((p, i) => {
    const [nx, ny] = normal(pts, i);
    const o = by(i);
    return [p[0] + nx * o, p[1] + ny * o];
  });
}

function stroke(d, extra = "") {
  return (
    `<path class="d-line" d="${d}" fill="none" stroke="${INK}" stroke-width="0.4" ` +
    `stroke-linecap="round" stroke-linejoin="round"${extra}/>`
  );
}

// Backward skating: 2 ft cups every 3 ft (with a puck dot between them).
function cups(path, until, dots) {
  let out = "";
  for (let start = 0; start + 4 <= until; start += 6) {
    const unit = [];
    for (let k = 0; k <= 8; k++) {
      const u = k / 8;
      const [p, n] = at(path, start + 4 * u);
      const o = 0.8 * Math.sin(Math.PI * u);
      unit.push([p[0] + n[0] * o, p[1] + n[1] * o]);
    }
    out += stroke(pathD(unit));
    if (dots && start + 5 < until) {
      const [p] = at(path, start + 5);
      out += `<circle class="d-dot" cx="${num(p[0])}" cy="${num(p[1])}" r="0.35" fill="${INK}"/>`;
    }
  }
  return out;
}

// The small loop at the start of a goalie slide.
function curl(path) {
  const [p, n] = at(path, 0);
  const r = 0.7;
  return (
    `<circle class="d-line" cx="${num(p[0] - n[0] * r)}" cy="${num(p[1] - n[1] * r)}" ` +
    `r="${num(r)}" fill="none" stroke="${INK}" stroke-width="0.4"/>`
  );
}

function drawLine(style, end, raw) {
  if (raw.length < 2) {
    return "";
  }
  const path = smooth(raw);
  const count = path.length;
  const plainFrom = Math.max(count - 5, 0);
  const tail = () => (count - plainFrom >= 2 ? stroke(pathD(path.slice(plainFrom))) : "");
  let out = "";
  if (style === "pass") {
    out += stroke(pathD(path), ' stroke-dasharray="1.6 1.2"');
  } else if (style === "shot") {
    for (const side of [-0.55, 0.55]) {
      out += stroke(pathD(offset(path, () => side).slice(0, Math.max(plainFrom, 2))));
    }
  } else if (style === "carry") {
    // A 3 ft wave, sampled finely, easing into the plain end.
    const wave = [];
    for (let k = 0; k <= plainFrom * 4; k++) {
      const s = k / 4;
      const [[x, y], [nx, ny]] = at(path, s);
      const fade = Math.min((plainFrom - s) / 2, 1);
      const o = 0.8 * fade * Math.sin((s * STEP * 2 * Math.PI) / 3);
      wave.push([x + nx * o, y + ny * o]);
    }
    out += stroke(pathD(wave.concat(path.slice(plainFrom + 1))));
  } else if (style === "backward" || style === "backcarry") {
    out += cups(path, plainFrom, style === "backcarry") + tail();
  } else if (style === "lateral") {
    const ticks = [];
    for (let i = 1; i < plainFrom; i += 2) {
      const [x, y] = path[i];
      const [nx, ny] = normal(path, i);
      ticks.push([
        [x - nx, y - ny],
        [x + nx, y + ny],
      ]);
    }
    if (ticks.length) {
      out += stroke(segmentsD(ticks));
    }
    out += tail();
  } else if (style === "crossover") {
    const arms = [];
    for (let s = 2; s + 1 < plainFrom; s += 4) {
      const [x, y] = path[s];
      const [nx, ny] = normal(path, s);
      const tx = ny;
      const ty = -nx;
      const a = 0.6;
      arms.push([
        [x - (tx + nx) * a, y - (ty + ny) * a],
        [x + (tx + nx) * a, y + (ty + ny) * a],
      ]);
      arms.push([
        [x - (tx - nx) * a, y - (ty - ny) * a],
        [x + (tx - nx) * a, y + (ty - ny) * a],
      ]);
    }
    if (arms.length) {
      out += stroke(segmentsD(arms));
    }
    out += tail();
  } else if (style === "padslide" || style === "butterfly") {
    const bump = style === "padslide";
    out += curl(path);
    const shape = (i) => (bump && i >= 2 && i <= 8 ? 0.9 * Math.sin((Math.PI * (i - 2)) / 6) : 0);
    out += stroke(pathD(offset(path, shape)));
  } else {
    out += stroke(pathD(path));
  }
  const tip = path[count - 1];
  const back = path[Math.max(count - 4, 0)];
  const len = Math.hypot(tip[0] - back[0], tip[1] - back[1]);
  if (len < 1e-3) {
    return out;
  }
  const ux = (tip[0] - back[0]) / len;
  const uy = (tip[1] - back[1]) / len;
  if (end === "stop") {
    const bars = [0, 0.9].map((b) => {
      const x = tip[0] - ux * b;
      const y = tip[1] - uy * b;
      return [
        [x - uy * 1.4, y + ux * 1.4],
        [x + uy * 1.4, y - ux * 1.4],
      ];
    });
    out += `<path class="d-line" d="${segmentsD(bars)}" fill="none" stroke="${INK}" stroke-width="0.5"/>`;
  } else if (end !== "none") {
    const bx = tip[0] - ux * 2.2;
    const by = tip[1] - uy * 2.2;
    const d =
      `M ${num(tip[0])} ${num(tip[1])} L ${num(bx - uy)} ${num(by + ux)} ` +
      `L ${num(bx + uy)} ${num(by - ux)} Z`;
    out += `<path class="d-head" d="${d}" fill="${INK}"/>`;
  }
  return out;
}

// Local coordinates for a turned item: u along `deg`, v across.
function local(x, y, deg) {
  const r = (deg * Math.PI) / 180;
  const s = Math.sin(r);
  const c = Math.cos(r);
  return (u, v) => [x + u * c - v * s, y + u * s + v * c];
}

const NET_DEPTH = 3.4;
const NET_HALF = 3.3;
const NET_FLARE = 0.5;
const NET_P = 2.6;

function netBack(v) {
  const t = Math.min(Math.abs(v) / NET_HALF, 1);
  return -NET_FLARE - (NET_DEPTH - NET_FLARE) * Math.max(1 - t ** NET_P, 0) ** (1 / NET_P);
}

function netSide(u) {
  const g = Math.min(Math.max((-u - NET_FLARE) / (NET_DEPTH - NET_FLARE), 0), 1);
  return NET_HALF * Math.max(1 - g ** NET_P, 0) ** (1 / NET_P);
}

function drawNet(x, y, deg) {
  const p = local(x, y, deg);
  const outline = [p(0, -3)];
  for (let k = 0; k <= 20; k++) {
    const v = -NET_HALF + (2 * NET_HALF * k) / 20;
    outline.push(p(netBack(v), v));
  }
  outline.push(p(0, 3));
  const mesh = [];
  for (const v of [-2.2, -1.1, 0, 1.1, 2.2]) {
    mesh.push([p(0, v), p(netBack(v), v)]);
  }
  for (const u of [-1.2, -2.3]) {
    const s = netSide(u);
    mesh.push([p(u, -s), p(u, s)]);
  }
  const a = p(0, -3);
  const b = p(0, 3);
  return (
    `<path class="d-net" d="${pathD(outline)} Z" fill="${NET}" stroke="${RED}" stroke-width="0.3" ` +
    `stroke-linejoin="round"/>` +
    `<path class="d-mesh" d="${segmentsD(mesh)}" fill="none" stroke="${NET_MESH}" stroke-width="0.12"/>` +
    `<path class="d-red" d="M ${num(a[0])} ${num(a[1])} L ${num(b[0])} ${num(b[1])}" ` +
    `stroke="${RED}" stroke-width="0.6" stroke-linecap="round"/>` +
    [a, b]
      .map(
        (q) =>
          `<circle class="d-red-fill" cx="${num(q[0])}" cy="${num(q[1])}" r="0.4" fill="${RED}"/>`,
      )
      .join("")
  );
}

function drawBarrier(x, y, deg) {
  const p = local(x, y, deg);
  const body = [p(-4, -0.55), p(4, -0.55), p(4, 0.55), p(-4, 0.55)];
  const stripes = [];
  for (let k = -3; k <= 3; k++) {
    const u = k * 1.1;
    stripes.push([p(u - 0.45, 0.55), p(u + 0.45, -0.55)]);
  }
  return (
    `<path class="d-barrier" d="${pathD(body)} Z" fill="${BARRIER}" stroke="${INK}" ` +
    `stroke-width="0.2" stroke-linejoin="round"/>` +
    `<path class="d-stripe" d="${segmentsD(stripes)}" fill="none" stroke="${INK}" stroke-width="0.2"/>`
  );
}

function drawDangler(x, y, deg) {
  const p = local(x, y, deg);
  const bar = [p(-2, -0.3), p(2, -0.3), p(2, 0.3), p(-2, 0.3)];
  const feet = [-2, 2].map((u) => [p(u, -1), p(u, 1)]);
  return (
    `<path class="d-dangler" d="${pathD(bar)} Z" fill="${DANGLER}" stroke="${INK}" stroke-width="0.15"/>` +
    `<path class="d-foot" d="${segmentsD(feet)}" fill="none" stroke="${INK}" stroke-width="0.5" ` +
    `stroke-linecap="round"/>`
  );
}

// Coaches and goalies (C, G, C2, …) are circled.
function circled(label) {
  return /^[CG][0-9]*$/.test(label);
}

function drawPlayer(x, y, raw) {
  const label = (raw || "").trim() || "X";
  if (circled(label)) {
    const r = 2.6 + 0.9 * (label.length - 1);
    return (
      `<circle class="d-ring" cx="${num(x)}" cy="${num(y)}" r="${num(r)}" fill="${ICE}" ` +
      `stroke="${INK}" stroke-width="0.35"/>` +
      svgText(x, y + 1.45, 4, 700, label, "d-player")
    );
  }
  return svgText(x, y + 1.8, 5, 700, label, "d-player");
}

function puck(x, y) {
  return `<circle class="d-puck" cx="${num(x)}" cy="${num(y)}" r="0.75" fill="${INK}"/>`;
}

function drawItem(item) {
  const { x, y } = item;
  switch (item.t) {
    case "player":
      return drawPlayer(x, y, item.label);
    case "cone": {
      const d =
        `M ${num(x)} ${num(y - 1.6)} L ${num(x + 1.4)} ${num(y + 1.1)} ` +
        `L ${num(x - 1.4)} ${num(y + 1.1)} Z`;
      return `<path class="d-cone" d="${d}" fill="${CONE}" stroke="${INK}" stroke-width="0.2"/>`;
    }
    case "puck":
      return puck(x, y);
    case "pucks":
      return [
        [-1.1, 0.6],
        [1.1, 0.6],
        [0, -0.9],
        [0, 1.5],
      ]
        .map(([dx, dy]) => puck(x + dx, y + dy))
        .join("");
    case "tire":
      return (
        `<circle class="d-tire" cx="${num(x)}" cy="${num(y)}" r="1.8" fill="none" ` +
        `stroke="${INK}" stroke-width="0.7"/>`
      );
    case "net":
      return drawNet(x, y, item.rot || 0);
    case "barrier":
      return drawBarrier(x, y, item.rot || 0);
    case "dangler":
      return drawDangler(x, y, item.rot || 0);
    case "text":
      return svgText(x, y + 1, 3.2, 600, item.text || "", "d-text");
    case "line":
      return drawLine(item.style, item.end || "arrow", item.pts);
    default:
      return "";
  }
}

// Drawing order: nets, barriers and danglers; then lines; then the rest.
function layerOf(item) {
  if (ROTATABLE.has(item.t)) {
    return 0;
  }
  return item.t === "line" ? 1 : 2;
}

// Something easy to grab under each item.
function hitArea(item) {
  if (item.t === "line") {
    return `<path class="hit" d="${pathD(smooth(item.pts))}"/>`;
  }
  const r = item.t === "barrier" ? 4 : 3;
  return `<circle class="hit-area" cx="${num(item.x)}" cy="${num(item.y)}" r="${r}"/>`;
}

// Where a turned item's rotation handle sits.
function handleAt(item) {
  const reach = item.t === "barrier" ? 6 : 5.2;
  const r = ((item.rot || 0) * Math.PI) / 180;
  return [item.x + Math.cos(r) * reach, item.y + Math.sin(r) * reach];
}

// Goal-line spots a new net snaps to: [x, y, facing].
const GOALS = [
  [11, 42.5, 0],
  [189, 42.5, 180],
];

// Fewer points for a freehand stroke (Ramer–Douglas–Peucker).
function simplify(pts, epsilon) {
  if (pts.length < 3) {
    return pts;
  }
  const a = pts[0];
  const b = pts[pts.length - 1];
  const len = Math.max(Math.hypot(b[0] - a[0], b[1] - a[1]), 1e-6);
  let worst = 0;
  let index = 0;
  for (let i = 1; i < pts.length - 1; i++) {
    const p = pts[i];
    const d = Math.abs((b[0] - a[0]) * (a[1] - p[1]) - (a[0] - p[0]) * (b[1] - a[1])) / len;
    if (d > worst) {
      worst = d;
      index = i;
    }
  }
  if (worst <= epsilon) {
    return [a, b];
  }
  const left = simplify(pts.slice(0, index + 1), epsilon);
  return left.slice(0, -1).concat(simplify(pts.slice(index), epsilon));
}

function lengthOf(pts) {
  let total = 0;
  for (let i = 1; i < pts.length; i++) {
    total += Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]);
  }
  return total;
}

let activeEditor = null;

function setUpEditor(root) {
  if (root.dataset.ready) {
    return;
  }
  root.dataset.ready = "1";
  const form = root.closest("form");
  const json = form.querySelector("[data-diagram-json]");
  const areaSelect = form.querySelector("[data-editor-area]");
  const lanesSelect = form.querySelector("[data-editor-lanes]");
  const endSelect = root.querySelector("[data-line-end]");
  const canvas = root.querySelector(".editor-canvas");
  const svg = root.querySelector("[data-editor-svg]");
  const frames = JSON.parse(root.dataset.frames);

  let state;
  try {
    state = JSON.parse(json.value || '{"items":[]}');
  } catch {
    state = { items: [] };
  }
  if (!state || !Array.isArray(state.items)) {
    state = { items: [] };
  }
  let tool = { kind: "select" };
  let selected = -1;
  let drag = null;
  // Turning an item with its handle: { item }.
  let spin = null;
  let draft = null;
  const history = [];

  const layer = document.createElementNS(SVG_NS, "g");
  layer.setAttribute("class", "drill");
  const overlay = document.createElementNS(SVG_NS, "g");
  svg.append(layer, overlay);
  canvas.dataset.mode = "select";

  // Keep receiving moves while the pointer is down, even outside the rink.
  const capture = (event) => {
    try {
      svg.setPointerCapture(event.pointerId);
    } catch {
      // Not an active pointer (e.g. a synthetic event): nothing to capture.
    }
  };

  // The drawable area: the frame without the padding around the boards.
  // The drill's size key ("half", "half-3"); areas without lanes use the whole width.
  const sizeKey = () => {
    const lanes = Number(lanesSelect.value) || 1;
    const key = lanes > 1 ? `${areaSelect.value}-${lanes}` : areaSelect.value;
    return frames[key] ? key : areaSelect.value;
  };
  const syncLanes = () => {
    const single = !frames[`${areaSelect.value}-2`];
    lanesSelect.disabled = single;
    if (single) {
      lanesSelect.value = "1";
    }
  };
  const frame = () => {
    const [x, y, w, h] = frames[sizeKey()];
    return { x: x + 1.5, y: y + 1.5, w: w - 3, h: h - 3 };
  };
  const setView = () => svg.setAttribute("viewBox", frames[sizeKey()].join(" "));
  const keep = (v, lo, hi) => Math.round(Math.min(Math.max(v, lo), hi) * 10) / 10;

  function rawPoint(event) {
    const pt = svg.createSVGPoint();
    pt.x = event.clientX;
    pt.y = event.clientY;
    return pt.matrixTransform(svg.getScreenCTM().inverse());
  }

  function point(event) {
    const p = rawPoint(event);
    const f = frame();
    return [keep(p.x, f.x, f.x + f.w), keep(p.y, f.y, f.y + f.h)];
  }

  function save() {
    json.value = JSON.stringify(state);
  }

  function remember() {
    history.push(JSON.stringify(state));
    if (history.length > 100) {
      history.shift();
    }
  }

  function render() {
    const order = state.items
      .map((item, i) => [item, i])
      .sort((a, b) => layerOf(a[0]) - layerOf(b[0]));
    layer.innerHTML = order
      .map(([item, i]) => `<g data-i="${i}">${drawItem(item)}${hitArea(item)}</g>`)
      .join("");
    overlay.innerHTML = "";
    if (draft && draft.pts.length) {
      const pts = draft.hover ? draft.pts.concat([draft.hover]) : draft.pts;
      if (pts.length > 1) {
        overlay.innerHTML = `<g class="draft">${drawLine(draft.style, draft.end, pts)}</g>`;
      }
    }
    if (selected >= 0) {
      const g = layer.querySelector(`[data-i="${selected}"]`);
      if (g) {
        const b = g.getBBox();
        const box = document.createElementNS(SVG_NS, "rect");
        box.setAttribute("class", "sel-box");
        box.setAttribute("x", b.x - 1);
        box.setAttribute("y", b.y - 1);
        box.setAttribute("width", b.width + 2);
        box.setAttribute("height", b.height + 2);
        overlay.append(box);
        const item = state.items[selected];
        if (item && ROTATABLE.has(item.t)) {
          const [hx, hy] = handleAt(item);
          const g2 = document.createElementNS(SVG_NS, "g");
          g2.setAttribute("class", "rot-handle");
          g2.innerHTML =
            `<path d="M ${num(item.x)} ${num(item.y)} L ${num(hx)} ${num(hy)}"/>` +
            `<circle cx="${num(hx)}" cy="${num(hy)}" r="1.3"><title>Drag to turn</title></circle>`;
          overlay.append(g2);
        }
      }
    }
  }

  // Turn the selected item by `by` degrees, onto a multiple of `by`.
  function turn(by) {
    const item = state.items[selected];
    if (item && ROTATABLE.has(item.t)) {
      change(() => {
        const next = Math.round(((item.rot || 0) + by) / by) * by;
        item.rot = ((next % 360) + 360) % 360;
      });
    }
  }

  function change(mutate) {
    remember();
    mutate();
    save();
    render();
  }

  function setTool(button) {
    for (const b of root.querySelectorAll("[data-tool]")) {
      b.setAttribute("aria-pressed", String(b === button));
    }
    tool = { kind: button.dataset.tool, label: button.dataset.label, style: button.dataset.style };
    canvas.dataset.mode = tool.kind;
    draft = null;
    if (tool.kind !== "select") {
      selected = -1;
    }
    render();
  }

  function finishDraft() {
    if (!draft) {
      return;
    }
    const d = draft;
    draft = null;
    const pts = d.pts.filter(
      (p, i) => i === 0 || Math.hypot(p[0] - d.pts[i - 1][0], p[1] - d.pts[i - 1][1]) > 0.5,
    );
    if (pts.length >= 2) {
      change(() => state.items.push({ t: "line", style: d.style, end: d.end, pts }));
    } else {
      render();
    }
  }

  function editLabel(i) {
    const item = state.items[i];
    if (!item || (item.t !== "player" && item.t !== "text")) {
      return;
    }
    const key = item.t === "player" ? "label" : "text";
    const value = window.prompt("Label", item[key]);
    if (value !== null && value.trim()) {
      change(() => {
        item[key] = value.trim().slice(0, 60);
      });
    }
  }

  const actions = {
    delete() {
      if (selected >= 0) {
        const i = selected;
        selected = -1;
        change(() => state.items.splice(i, 1));
      }
    },
    rotateRight() {
      turn(15);
    },
    rotateLeft() {
      turn(-15);
    },
    undo() {
      if (history.length) {
        state = JSON.parse(history.pop());
        selected = -1;
        save();
        render();
      }
    },
    clear() {
      if (state.items.length && window.confirm("Remove everything from the diagram?")) {
        selected = -1;
        change(() => {
          state.items = [];
        });
      }
    },
    finish: finishDraft,
    cancel() {
      draft = null;
      selected = -1;
      render();
    },
    selectTool() {
      setTool(root.querySelector('[data-tool="select"]'));
    },
  };

  root.addEventListener("click", (event) => {
    const button = event.target.closest(".editor-tools [data-tool], .editor-tools [data-action]");
    if (!button) {
      return;
    }
    if (button.dataset.tool) {
      setTool(button);
    } else {
      actions[button.dataset.action]();
    }
  });

  svg.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) {
      return;
    }
    activeEditor = editor;
    const p = point(event);
    const [x, y] = p;
    if (tool.kind === "select" && selected >= 0 && event.target.closest(".rot-handle")) {
      remember();
      spin = { item: state.items[selected] };
      capture(event);
      return;
    }
    if (tool.kind === "select") {
      const g = event.target.closest("[data-i]");
      selected = g ? Number(g.dataset.i) : -1;
      if (g) {
        drag = { start: p, original: JSON.stringify(state.items[selected]), moved: false };
        capture(event);
      }
      render();
      return;
    }
    if (tool.kind === "line") {
      if (draft) {
        draft.pts.push(p);
      } else {
        draft = { style: tool.style, end: endSelect.value, pts: [p], freehand: true };
      }
      capture(event);
      render();
      return;
    }
    let item;
    if (tool.kind === "player") {
      let label = tool.label;
      if (!label) {
        label = window.prompt("Label (for example F1, D2, LW)", "");
        if (!label || !label.trim()) {
          return;
        }
      }
      item = { t: "player", x, y, label: label.trim().slice(0, 60) };
    } else if (tool.kind === "text") {
      const text = window.prompt("Text", "");
      if (!text || !text.trim()) {
        return;
      }
      item = { t: "text", x, y, text: text.trim().slice(0, 60) };
    } else if (tool.kind === "net") {
      const f = frame();
      item = { t: "net", x, y, rot: x < f.x + f.w / 2 ? 0 : 180 };
      // Snap onto a goal line when dropped near one.
      for (const [gx, gy, rot] of GOALS) {
        const inside = gx >= f.x && gx <= f.x + f.w && gy >= f.y && gy <= f.y + f.h;
        if (inside && Math.hypot(x - gx, y - gy) < 5) {
          item = { t: "net", x: gx, y: gy, rot };
        }
      }
    } else if (ROTATABLE.has(tool.kind)) {
      item = { t: tool.kind, x, y, rot: 0 };
    } else {
      item = { t: tool.kind, x, y };
    }
    change(() => state.items.push(item));
  });

  svg.addEventListener("pointermove", (event) => {
    if (spin) {
      const p = rawPoint(event);
      const deg = (Math.atan2(p.y - spin.item.y, p.x - spin.item.x) * 180) / Math.PI;
      const step = event.shiftKey ? 1 : 15;
      spin.item.rot = (((Math.round(deg / step) * step) % 360) + 360) % 360;
      render();
      return;
    }
    if (drag && selected >= 0) {
      const p = point(event);
      const dx = p[0] - drag.start[0];
      const dy = p[1] - drag.start[1];
      if (!drag.moved && Math.hypot(dx, dy) < 0.3) {
        return;
      }
      if (!drag.moved) {
        remember();
        drag.moved = true;
      }
      const moved = JSON.parse(drag.original);
      const f = frame();
      if (moved.t === "line") {
        moved.pts = moved.pts.map(([px, py]) => [
          keep(px + dx, f.x, f.x + f.w),
          keep(py + dy, f.y, f.y + f.h),
        ]);
      } else {
        moved.x = keep(moved.x + dx, f.x, f.x + f.w);
        moved.y = keep(moved.y + dy, f.y, f.y + f.h);
      }
      state.items[selected] = moved;
      render();
    } else if (draft) {
      const p = point(event);
      if (draft.freehand && event.buttons & 1) {
        const last = draft.pts[draft.pts.length - 1];
        if (Math.hypot(p[0] - last[0], p[1] - last[1]) > 0.8) {
          draft.pts.push(p);
        }
      } else {
        draft.hover = p;
      }
      render();
    }
  });

  svg.addEventListener("pointerup", () => {
    if (spin) {
      spin = null;
      save();
      return;
    }
    if (drag) {
      if (drag.moved) {
        save();
      }
      drag = null;
      return;
    }
    if (draft && draft.freehand) {
      draft.freehand = false;
      if (lengthOf(draft.pts) > 3) {
        draft.pts = simplify(draft.pts, 1.2);
        finishDraft();
      } else {
        // A click: keep adding points until a double-click or Enter.
        draft.pts = [draft.pts[0]];
        render();
      }
    }
  });

  svg.addEventListener("dblclick", (event) => {
    if (draft) {
      finishDraft();
    } else if (tool.kind === "select") {
      const g = event.target.closest("[data-i]");
      if (g) {
        editLabel(Number(g.dataset.i));
      }
    }
  });

  for (const select of [areaSelect, lanesSelect]) {
    select.addEventListener("change", () => {
      syncLanes();
      setView();
      render();
    });
  }
  syncLanes();

  const editor = { root, actions, isDrawing: () => Boolean(draft) };
  activeEditor = editor;
  setView();
  save();
  render();
}

document.addEventListener("keydown", (event) => {
  if (!activeEditor || !activeEditor.root.isConnected) {
    return;
  }
  if (event.target.closest && event.target.closest("input, select, textarea, [contenteditable]")) {
    return;
  }
  const { actions } = activeEditor;
  const key = event.key.toLowerCase();
  if ((event.ctrlKey || event.metaKey) && key === "z") {
    event.preventDefault();
    actions.undo();
  } else if (event.ctrlKey || event.metaKey || event.altKey) {
    return;
  } else if (key === "delete" || key === "backspace") {
    event.preventDefault();
    actions.delete();
  } else if (key === "enter" && activeEditor.isDrawing()) {
    event.preventDefault();
    actions.finish();
  } else if (key === "escape") {
    actions.cancel();
  } else if (key === "r") {
    if (event.shiftKey) {
      actions.rotateLeft();
    } else {
      actions.rotateRight();
    }
  } else if (key === "v") {
    actions.selectTool();
  }
});

// "Count from the diagram": cones, pucks (a pile counts as 10), nets, tires.
document.addEventListener("click", (event) => {
  const button = event.target.closest("[data-count-equipment]");
  if (!button) {
    return;
  }
  const form = button.closest("form");
  let items = [];
  try {
    items = JSON.parse(form.querySelector("[data-diagram-json]").value).items || [];
  } catch {
    return;
  }
  const count = (t) => items.filter((i) => i.t === t).length;
  const totals = {
    cones: count("cone"),
    pucks: count("puck") + 10 * count("pucks"),
    nets: count("net"),
    tires: count("tire"),
    barriers: count("barrier"),
    danglers: count("dangler"),
  };
  for (const [key, value] of Object.entries(totals)) {
    const input = form.querySelector(`[data-equipment="${key}"]`);
    if (input) {
      input.value = value;
    }
  }
});

// The builder column is swapped in place after each change; <details> marked
// data-keep (part settings, drill text) stay as the coach left them.
let keptDetails = null;

document.addEventListener("htmx:beforeSwap", (event) => {
  const target = event.detail.target;
  if (target && target.id === "builder-main") {
    keptDetails = new Map();
    for (const d of target.querySelectorAll("details[data-keep]")) {
      keptDetails.set(d.dataset.keep, d.open);
    }
  }
});

document.addEventListener("htmx:afterSwap", () => {
  if (!keptDetails) {
    return;
  }
  for (const d of document.querySelectorAll("#builder-main details[data-keep]")) {
    if (keptDetails.has(d.dataset.keep)) {
      d.open = keptDetails.get(d.dataset.keep);
    }
  }
  keptDetails = null;
});

// Part forms on full ice: the left/right end fields only matter when the ice
// is split at the red line, so hide them otherwise.
function syncLayout(select) {
  const form = select.closest("form");
  for (const field of form.querySelectorAll("[data-layout-end]")) {
    field.hidden = select.value !== "ends";
  }
}

document.addEventListener("change", (event) => {
  if (event.target.matches("[data-layout-whole]")) {
    syncLayout(event.target);
  }
});

function setUpEditors(root) {
  for (const editor of root.querySelectorAll("[data-diagram-editor]")) {
    setUpEditor(editor);
  }
  for (const select of root.querySelectorAll("[data-layout-whole]")) {
    syncLayout(select);
  }
}

if (window.htmx) {
  htmx.onLoad(setUpEditors);
} else {
  document.addEventListener("DOMContentLoaded", () => setUpEditors(document.body));
}
