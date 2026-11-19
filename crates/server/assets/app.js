// LocalApp page behaviour. Every page also works without JavaScript; this adds
// conveniences on top of htmx (which turns links and forms into in-page
// updates) and plain HTML forms.
"use strict";

document.documentElement.classList.add("js");

// Ask before submitting a button marked data-confirm="…".
document.addEventListener("click", (event) => {
  const button = event.target.closest("[data-confirm]");
  if (button && !window.confirm(button.dataset.confirm)) {
    event.preventDefault();
    event.stopPropagation();
  }
});

// Submit the enclosing form as soon as a control marked data-autosubmit changes
// (for example a filter drop-down).
document.addEventListener("change", (event) => {
  const control = event.target.closest("[data-autosubmit]");
  if (control && control.form) {
    control.form.requestSubmit();
  }
});

// Editing a field marked data-implies="name=value" also sets the field `name`
// in the same form (e.g. typing a date switches the range preset to "custom").
document.addEventListener("input", (event) => {
  const source = event.target.closest("[data-implies]");
  if (!source || !source.form) {
    return;
  }
  const [name, value] = source.dataset.implies.split("=");
  const target = source.form.elements.namedItem(name);
  if (target) {
    target.value = value;
  }
});

// A checkbox marked data-select-all="name" ticks every checkbox called `name`.
document.addEventListener("change", (event) => {
  const master = event.target.closest("[data-select-all]");
  if (!master) {
    return;
  }
  const selector = `input[type=checkbox][name="${master.dataset.selectAll}"]`;
  for (const box of document.querySelectorAll(selector)) {
    box.checked = master.checked;
  }
});

// Close open drop-down menus (user menu) on an outside
// click or Escape.
function closeMenus(except) {
  for (const menu of document.querySelectorAll("details.menu[open]")) {
    if (menu !== except) {
      menu.open = false;
    }
  }
}

document.addEventListener("click", (event) => {
  closeMenus(event.target.closest("details.menu"));
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closeMenus(null);
  }
});

// "/" jumps to the page's search or filter box, as in many web apps.
document.addEventListener("keydown", (event) => {
  const typing = event.target.closest("input, select, textarea, [contenteditable]");
  if (event.key !== "/" || typing || event.ctrlKey || event.metaKey || event.altKey) {
    return;
  }
  const search = document.querySelector("main input[type=search]");
  if (search) {
    event.preventDefault();
    search.focus();
    search.select();
  }
});

// ---- Tooltips --------------------------------------------------------------

// Floating tooltip for anything with data-tip (the "?" badges).
function tooltip() {
  let tip = document.getElementById("tooltip");
  if (!tip) {
    tip = document.createElement("div");
    tip.id = "tooltip";
    tip.className = "tooltip";
    tip.hidden = true;
    document.body.append(tip);
  }
  return tip;
}

function showTip(target, x, y) {
  const tip = tooltip();
  tip.textContent = target.dataset.tip;
  tip.hidden = false;
  const pad = 12;
  const box = tip.getBoundingClientRect();
  const left = Math.min(x + pad, window.innerWidth - box.width - pad);
  const top = y + pad + box.height > window.innerHeight ? y - box.height - pad : y + pad;
  tip.style.left = `${Math.max(pad, left)}px`;
  tip.style.top = `${Math.max(pad, top)}px`;
}

document.addEventListener("pointermove", (event) => {
  const target = event.target.closest("[data-tip]");
  if (target) {
    showTip(target, event.clientX, event.clientY);
  } else {
    tooltip().hidden = true;
  }
});

document.addEventListener("focusin", (event) => {
  const target = event.target.closest("[data-tip]");
  if (target) {
    const box = target.getBoundingClientRect();
    showTip(target, box.left, box.bottom);
  }
});

document.addEventListener("focusout", () => {
  tooltip().hidden = true;
});

// ---- Flash messages ----------------------------------------------------------

// Messages such as "Saved" appear as a toast and fade after a few
// seconds; errors stay until closed.
function setUpToasts(root) {
  for (const toast of root.querySelectorAll(".toasts .notice")) {
    const close = document.createElement("button");
    close.type = "button";
    close.className = "toast-close";
    close.setAttribute("aria-label", "Dismiss");
    close.textContent = "×";
    close.addEventListener("click", () => toast.remove());
    toast.append(close);
    if (!toast.classList.contains("error")) {
      setTimeout(() => toast.classList.add("fading"), 4500);
      setTimeout(() => toast.remove(), 5000);
    }
  }
}

// ---- htmx ----------------------------------------------------------------------

// A form that changes something and comes back to the same page (save a
// setting, toggle an option…) keeps the scroll position instead of jumping to the top.
let restoreScroll = null;

document.addEventListener("htmx:beforeRequest", (event) => {
  const el = event.detail.elt;
  const isPost = el.tagName === "FORM" && el.method === "post";
  restoreScroll = isPost ? { y: window.scrollY, path: location.pathname } : null;
});

document.addEventListener("htmx:afterSettle", () => {
  if (restoreScroll && location.pathname === restoreScroll.path) {
    window.scrollTo(0, restoreScroll.y);
  }
  restoreScroll = null;
});

if (window.htmx) {
  htmx.onLoad(setUpToasts);
} else {
  document.addEventListener("DOMContentLoaded", () => setUpToasts(document.body));
}

// A development split ([data-focus]: a drill's split or a team's targets):
// show the running total ([data-focus-total]), flagged until it makes 100%.
// An empty box counts as 0 (the server reads it the same way).
function focusValue(input) {
  return Math.max(0, parseInt(input.value, 10) || 0);
}

function updateFocusTotal(group) {
  const total = Array.from(group.querySelectorAll("[data-focus-part]")).reduce(
    (sum, input) => sum + focusValue(input),
    0,
  );
  const out = group.querySelector("[data-focus-total]");
  if (out) {
    out.textContent = String(total);
    out.classList.toggle("warn-text", total !== 100);
  }
}
document.addEventListener("input", (event) => {
  const group = event.target.closest("[data-focus]");
  if (group) {
    updateFocusTotal(group);
  }
});

// Normalize ([data-focus-normalize]): scale what's entered so it adds up to
// 100% in the boxes' steps (5%), keeping the proportions as closely as the
// steps allow (largest remainders get the leftover steps).
function normalizeFocus(group) {
  const inputs = Array.from(group.querySelectorAll("[data-focus-part]"));
  const values = inputs.map(focusValue);
  const sum = values.reduce((a, b) => a + b, 0);
  if (sum === 0) {
    return;
  }
  const step = Number(inputs[0].step) || 5;
  const units = Math.round(100 / step);
  const exact = values.map((v) => (v * units) / sum);
  const steps = exact.map(Math.floor);
  let left = units - steps.reduce((a, b) => a + b, 0);
  const order = exact
    .map((e, i) => [e - Math.floor(e), i])
    .sort((a, b) => b[0] - a[0] || a[1] - b[1]);
  for (const [, i] of order) {
    if (left <= 0) {
      break;
    }
    steps[i] += 1;
    left -= 1;
  }
  inputs.forEach((input, i) => {
    input.value = String(steps[i] * step);
    // Lets data-implies and the total react as if typed.
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
document.addEventListener("click", (event) => {
  const button = event.target.closest("[data-focus-normalize]");
  const group = button && button.closest("[data-focus]");
  if (group) {
    normalizeFocus(group);
  }
});
function setUpFocus(root) {
  root.querySelectorAll("[data-focus]").forEach(updateFocusTotal);
}
document.addEventListener("DOMContentLoaded", () => setUpFocus(document));
document.addEventListener("htmx:load", (event) => setUpFocus(event.target));
