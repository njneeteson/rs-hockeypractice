// Practice planner behaviour: the practice builder's drill library (drag and
// drop) and the drill diagram editor. Both are enhancements: without
// JavaScript, stations are filled with a <select> + "Place" button and the
// diagram is edited as JSON.
"use strict";

// ---- Practice builder: drill library -----------------------------------------
//
// Drag a drill from the library onto a station (a `.drop-slot` in a rink
// diagram) of the same ice size, or click a drill and then a station. Placing
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
    slot.classList.toggle("can-drop", Boolean(size) && slot.dataset.size === size);
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

function placeDrill(slot, drillId) {
  const wrap = slot.closest("[data-assign-url]");
  if (!wrap || !drillId) {
    return;
  }
  const form = document.createElement("form");
  form.method = "post";
  form.action = wrap.dataset.assignUrl + slot.dataset.slot;
  form.hidden = true;
  const input = document.createElement("input");
  input.type = "hidden";
  input.name = "drill_id";
  input.value = drillId;
  form.append(input);
  document.body.append(form);
  if (window.htmx) {
    htmx.process(form);
  }
  pick(null);
  form.requestSubmit();
}

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
const NET = "#e6e6e6";
const CONE = "#f08a24";

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

// Points along a Catmull-Rom curve through `pts`, about every 0.5 ft.
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
  return resample(dense, 0.5);
}

function normal(pts, i) {
  const a = pts[Math.max(i - 1, 0)];
  const b = pts[Math.min(i + 1, pts.length - 1)];
  const len = Math.max(Math.hypot(b[0] - a[0], b[1] - a[1]), 1e-6);
  return [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
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

function drawLine(style, end, raw) {
  if (raw.length < 2) {
    return "";
  }
  const path = smooth(raw);
  const count = path.length;
  const plainFrom = Math.max(count - 5, 0);
  let out = "";
  if (style === "pass") {
    out += stroke(pathD(path), ' stroke-dasharray="1.6 1.2"');
  } else if (style === "shot") {
    for (const side of [-0.55, 0.55]) {
      out += stroke(pathD(offset(path, () => side).slice(0, Math.max(plainFrom, 2))));
    }
  } else if (style === "carry") {
    const wave = (i) => (i >= plainFrom ? 0 : 0.8 * Math.sin((i * 0.5 * 2 * Math.PI) / 3));
    out += stroke(pathD(offset(path, wave)));
  } else if (style === "lateral") {
    const zigzag = (i) => (i >= plainFrom ? 0 : 0.9 * (1 - 4 * Math.abs((i % 6) / 6 - 0.5)));
    out += stroke(pathD(offset(path, zigzag)));
  } else if (style === "backward") {
    out += stroke(pathD(path));
    let ticks = "";
    for (let i = 4; i < plainFrom; i += 6) {
      const [nx, ny] = normal(path, i);
      const [x, y] = path[i];
      ticks += `M ${num(x - nx)} ${num(y - ny)} L ${num(x + nx)} ${num(y + ny)} `;
    }
    if (ticks) {
      out += stroke(ticks.trim());
    }
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
    const d =
      `M ${num(tip[0] - uy * 1.4)} ${num(tip[1] + ux * 1.4)} ` +
      `L ${num(tip[0] + uy * 1.4)} ${num(tip[1] - ux * 1.4)}`;
    out += `<path class="d-line" d="${d}" fill="none" stroke="${INK}" stroke-width="0.7"/>`;
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

function drawNet(x, y, deg) {
  const r = (deg * Math.PI) / 180;
  const s = Math.sin(r);
  const c = Math.cos(r);
  const p = (u, v) => [x + u * c - v * s, y + u * s + v * c];
  const pts = [p(0, -3), p(-2.6, -2.6), p(-3.3, 0), p(-2.6, 2.6), p(0, 3)];
  const [a, b] = [p(0, -3), p(0, 3)];
  return (
    `<path class="d-net" d="${pathD(pts)} Z" fill="${NET}" stroke="${INK}" stroke-width="0.35" ` +
    `stroke-linejoin="round"/>` +
    `<path class="d-red" d="M ${num(a[0])} ${num(a[1])} L ${num(b[0])} ${num(b[1])}" ` +
    `stroke="${RED}" stroke-width="0.35"/>`
  );
}

function puck(x, y) {
  return `<circle class="d-puck" cx="${num(x)}" cy="${num(y)}" r="0.75" fill="${INK}"/>`;
}

function drawItem(item) {
  const { x, y } = item;
  switch (item.t) {
    case "player":
      return svgText(x, y + 1.8, 5, 700, (item.label || "").trim() || "X", "d-player");
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
    case "text":
      return svgText(x, y + 1, 3.2, 600, item.text || "", "d-text");
    case "line":
      return drawLine(item.style, item.end || "arrow", item.pts);
    default:
      return "";
  }
}

// Something easy to grab under each item.
function hitArea(item) {
  if (item.t === "line") {
    return `<path class="hit" d="${pathD(smooth(item.pts))}"/>`;
  }
  return `<circle class="hit-area" cx="${num(item.x)}" cy="${num(item.y)}" r="3"/>`;
}

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
  const sizeSelect = form.querySelector("[data-editor-size-select]");
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
  const frame = () => {
    const [x, y, w, h] = frames[sizeSelect.value];
    return { x: x + 1.5, y: y + 1.5, w: w - 3, h: h - 3 };
  };
  const setView = () => svg.setAttribute("viewBox", frames[sizeSelect.value].join(" "));
  const keep = (v, lo, hi) => Math.round(Math.min(Math.max(v, lo), hi) * 10) / 10;

  function point(event) {
    const pt = svg.createSVGPoint();
    pt.x = event.clientX;
    pt.y = event.clientY;
    const p = pt.matrixTransform(svg.getScreenCTM().inverse());
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
      .sort((a, b) => (a[0].t === "line" ? 0 : 1) - (b[0].t === "line" ? 0 : 1));
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
      }
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
    rotate() {
      const item = state.items[selected];
      if (item && item.t === "net") {
        change(() => {
          item.rot = ((item.rot || 0) + 45) % 360;
        });
      }
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
    } else {
      item = { t: tool.kind, x, y };
    }
    change(() => state.items.push(item));
  });

  svg.addEventListener("pointermove", (event) => {
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

  sizeSelect.addEventListener("change", () => {
    setView();
    render();
  });

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
    actions.rotate();
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
  };
  for (const [key, value] of Object.entries(totals)) {
    const input = form.querySelector(`[data-equipment="${key}"]`);
    if (input) {
      input.value = value;
    }
  }
});

function setUpEditors(root) {
  for (const editor of root.querySelectorAll("[data-diagram-editor]")) {
    setUpEditor(editor);
  }
}

if (window.htmx) {
  htmx.onLoad(setUpEditors);
} else {
  document.addEventListener("DOMContentLoaded", () => setUpEditors(document.body));
}
