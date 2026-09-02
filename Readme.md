# x11-web

A cloud-managed X11 desktop aggregator. Run X11 applications on remote machines and interact with them from your browser.

```
┌──────────┐     WebSocket      ┌──────────┐     WebSocket      ┌──────────────┐
│ Frontend  │◄──────────────────►│ Backend  │◄──────────────────►│   Sidecar    │
│ (React)   │  display stream   │ (Rust)   │  control + stream  │   (Rust)     │
│           │  + control msgs   │  Cloud   │                    │ X11 Server + │
│ Canvas    │                   │  Routes  │                    │ Process Mgr  │
└──────────┘                    └──────────┘                    └──────────────┘
                                                                  ▲
                                                                  │ X11 protocol
                                                                  │ (Unix socket)
                                                                ┌─┴────────────┐
                                                                │  X11 Client  │
                                                                │  (any app)   │
                                                                └──────────────┘
```

Multiple desktop machines or servers can act as sidecars, each running X11 applications. All of them connect to a central backend in the cloud. The frontend lets you manage and interact with every application from a single browser tab.

## Components

### Backend (`crates/backend`)

Rust service (axum) that acts as the central hub. Provides WebSocket endpoints for sidecars (`/ws/sidecar`) and frontends (`/ws/frontend`), routing display updates and control messages between them.

### Sidecar (`crates/sidecar`)

Rust service that runs alongside X11 applications. Contains:

- **Process manager** — spawns and monitors child processes with `DISPLAY` set to the built-in X server
- **X11 server** — a minimal X11 server implemented in Rust using `x11rb-protocol`, handling window management, drawing primitives, and event dispatch
- **Backend connection** — maintains a persistent WebSocket to the backend, forwarding display updates and receiving commands

### Wayland sidecar (`crates/sidecar-wayland` + `crates/wayland-server`)

The same idea for Wayland clients. `crates/wayland-server` is an embeddable headless Wayland compositor (smithay-based, derived from [waylandcraft](https://github.com/EVV1E/waylandcraft) — see `crates/wayland-server/NOTICE`) that turns every mapped `xdg_toplevel` into the same `DisplayUpdate` stream the X11 server emits, and injects browser input back through `wl_seat`. `crates/sidecar-wayland` is the binary around it: process manager (children get `WAYLAND_DISPLAY` instead of `DISPLAY`), QUIC connection to the backend, WebP encoding.

Scope is deliberately narrow: `wl_shm` software buffers only (no dmabuf/EGL/GPU, no XWayland), `xdg_shell` toplevels and popups, `wl_seat` pointer + keyboard, `wl_output`, `viewporter`, `xdg_decoration`. Both crates only build on Linux; on other hosts they compile to a stub that exits with a message, so `cargo check --workspace` stays green on macOS.

### Protocol (`crates/protocol`)

Shared Rust crate defining the wire protocol between all components: control messages (spawn/kill), display updates (arcs, rectangles, images), and input events (keyboard, mouse).

### Frontend (`frontend/`)

React + TypeScript SPA that connects to the backend via WebSocket. Shows connected sidecars, lets you launch applications, and renders X11 output on an HTML5 Canvas.

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) (v20+)
- [pnpm](https://pnpm.io/)
- [Docker](https://docs.docker.com/get-docker/) (for containerized deployment and e2e tests)

## Getting started

Install dependencies:

```sh
pnpm install
```

Activate the repo's git hooks (one-shot, per clone):

```sh
mise run install-hooks
```

Points `core.hooksPath` at `.hooks/`. The pre-commit hook runs
`turbo run typecheck` across every TS workspace; turbo caches the
result, so commits that don't touch typed code are effectively
free.

### Local development

Run all three components in separate terminals:

```sh
# Terminal 1: backend
cargo run --bin x11-web-backend

# Terminal 2: sidecar
cargo run --bin x11-web-sidecar

# Terminal 3: frontend
pnpm dev
```

Then open http://localhost:5173. The sidecar will appear in the dashboard. Click "Spawn xeyes" to launch an X11 app.

Optional dev infra (mock OIDC, local OpenObserve for traces +
metrics + logs):

```sh
docker compose -f compose.dev.yml up -d
```

OpenObserve UI lives at http://localhost:5080 — log in with
`admin@admin.com` / `admin` (dev creds in `compose.dev.yml`).
`mprocs` already wires the right `OTEL_EXPORTER_OTLP_*` env
into the backend; leave those unset to opt out entirely.

### Docker Compose

```sh
# One-time: backend writes the QUIC fingerprint here on startup;
# Docker would otherwise create a directory at this path.
touch ~/.x11web-fingerprint

docker compose --profile full up --build
```

Opens the frontend at http://localhost:8080 with backend on port 3001.

`docker compose up sidecar` (without `--profile full`) starts only
the sidecar — used by mprocs when the backend runs natively on the
host via `cargo run`.

`docker compose --profile wayland up sidecar-wayland` starts the
Wayland sidecar instead. It's behind its own profile because the Dock
renders one spawn button per connected sidecar, so the dev loop wants
one at a time. Handy commands inside that container:
`wl-input-probe` (deterministic click/keystroke probe),
`weston-simple-shm`, `foot`, `wayland-info`.

### Build

```sh
# Frontend (via turbo)
pnpm build

# Rust
cargo build --release
```

## Testing

### E2E tests

The e2e tests use [Testcontainers](https://testcontainers.com/) to spin up the backend and sidecar in Docker, then [Playwright](https://playwright.dev/) to verify the frontend renders X11 output correctly — including a screenshot comparison of xeyes.

```sh
cd e2e
pnpm exec playwright install chromium
pnpm test
```

The Wayland suite (`e2e/tests/wayland/`) brings up its own backend +
Wayland sidecar pair, so it doesn't disturb the X11 specs:

```sh
cd e2e && pnpm exec playwright test tests/wayland
```

For a browser-free check of the Wayland sidecar image — socket, global
inventory, a real client mapping a toplevel, and pixels coming out the
other end — run:

```sh
bash e2e/scripts/wayland-smoke.sh
```

## Project structure

```
x11-web/
├── Cargo.toml                 # Rust workspace
├── crates/
│   ├── backend/               # Cloud backend (axum + WebSocket)
│   ├── protocol/              # Shared message types
│   ├── sidecar/               # X11 server + process manager
│   ├── sidecar-wayland/       # Wayland sidecar binary (Linux only)
│   └── wayland-server/        # Embedded headless Wayland compositor
├── frontend/                  # React SPA (Vite + TypeScript)
├── e2e/                       # Playwright + Testcontainers e2e tests
├── Dockerfile.backend
├── Dockerfile.sidecar
├── Dockerfile.sidecar-wayland
├── Dockerfile.frontend
├── compose.yml
├── turbo.json
└── pnpm-workspace.yaml
```
