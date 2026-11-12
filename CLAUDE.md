# CLAUDE.md

Guidance for AI-assisted work on this repository. See README.md for the
overview and layout.

## Architecture rules

- Two crates. `crates/core` holds domain logic and storage and must not depend
  on axum, maud, tokio or anything about users or HTTP. `crates/server` holds
  the web app and calls into core.
- Pages are server-rendered with maud. Every page must work without
  JavaScript: full pages, plain `<form method="post">`, and 303 redirects.
  htmx is an enhancement (boosting, live forms, fragments), never a requirement.
- No CDN or external requests. New JS/CSS/images go in `crates/server/assets/`
  and are registered in `assets.rs`.
- No inline `<script>`. The CSP forbids it, and htmx runs with `allowEval: false`.
  Put behaviour in `assets/app.js`, driven by `data-*` attributes.

## Practice planner specifics

- Diagrams are drawn twice: in Rust (`crates/core/src/diagram.rs`, used for
  web previews and the PDF) and in the browser editor
  (`crates/server/assets/planner.js`). Change both together.
- The PDF is built as one SVG per page (`pdf.rs`) and converted with
  krilla-svg using the embedded Noto Sans; text width for wrapping comes from
  the same font, so keep `svg::FONT` and the fonts in `crates/core/fonts` in step.
- Drawn SVG elements carry print colours as attributes and a class; web
  theming overrides only via those classes in `app.css`.
- Rotated items (net, barrier, dangler) are drawn point by point from their
  `rot`, not with SVG transforms, so slot mirrors and turns work the same in
  Rust and JS. New asymmetric items need a `rot` and an entry in `ROTATABLE`.
- `IceSize` (area + lanes) and `Layout` are structs, not `str_enum!`: their
  keys are compound (`"half-3"`, `"ends-h1-z3"`). `from_key` also accepts the
  pre-lanes names (`"quarter"`, `"halves"`, …).
- Every symbol a diagram can contain needs a legend entry in
  `diagram::legend`; PDF pages and the builder show only the symbols in use.

## Conventions

- **Permissions** are enforced by the extractor a handler takes: `Ctx` for any
  signed-in user, `AdminCtx` for admins. Never rely on hiding a button.
- **Database work** goes through `cx.db(|db| ...)` (runs on a blocking thread).
  Schema changes are new entries appended to `MIGRATIONS`. Never edit a
  shipped migration. One `db/<table>.rs` file per table.
- **Enums** stored in the database or shown in selects use `str_enum!`
  (stable key + label); parse form values with `form::key`.
- **Changes** are POST → `flash::info` / `flash::error` / `flash::report`
  (redirect + toast). Bad input → `AppError::BadRequest` via `form.rs` helpers.
- **URLs** always come from `cx.url("/path")` / `st.url(...)` so the base path
  is respected. Tests assert no link escapes the base path.
- **UI** comes from `views::components` and `views::forms` (cards, headings,
  fields, notices, tables). Colours are the CSS tokens in `app.css`
  (light and dark), never hard-coded.
- **Config** comes only from `LOCALAPP_*` env vars in `config.rs`. Document new
  ones in the README table.
- **Rust:** edition 2024, 100-column lines, `unsafe` forbidden.

## Checks before finishing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

New pages get an integration test in `crates/server/tests/` using
`common::TestApp` (it signs in, keeps cookies, and posts as a same-origin
browser).

## Running locally

`./scripts/dev.ps1` (Windows) or `./scripts/dev.sh`: serves
http://127.0.0.1:8080/localapp with data in `./data` and prints a first-run
setup link.
