//! Child-process supervision and the per-sidecar session bus.
//!
//! Derived from `crates/sidecar/src/main.rs` (the X11 sidecar) and kept
//! deliberately diff-able against it: same `ProcessManager` shape, same
//! stdout/stderr drain, same `spawned_pid_history`, same
//! `start_dbus_session`. The only real difference is the environment a
//! child is launched with — `WAYLAND_DISPLAY` + `XDG_RUNTIME_DIR` and
//! the toolkit backend hints instead of `DISPLAY` + `XAUTHORITY`.
//!
//! The `/proc` PPid walk lives here too. It works unchanged on the
//! Wayland side because the compositor reports the connecting client's
//! pid from `SO_PEERCRED` on the accepted socket, which is exactly the
//! same fact the X11 server reports from its own accepted socket — see
//! `x11-web-wayland-server`'s `server::peer_pid`.

use std::collections::{HashMap, HashSet};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::Duration;
use tracing::{info, warn};

use crate::telemetry;

/// Timeout for `dbus-daemon`'s `--print-address` line on startup.
const DBUS_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum depth we walk when traversing a /proc-style parent chain to
/// locate the original Wayland client owning a given process. Prevents
/// an unbounded loop if the chain contains a cycle.
const MAX_PROCESS_PARENT_DEPTH: usize = 50;

pub struct ProcessManager {
    processes: HashMap<u32, ManagedProcess>,
    /// `WAYLAND_DISPLAY` for the embedded compositor, e.g. `wayland-1`.
    wayland_display: String,
    /// `XDG_RUNTIME_DIR` the compositor bound its socket in. Children
    /// need it as well as the display name: libwayland resolves a
    /// relative `WAYLAND_DISPLAY` against this directory, and a client
    /// that inherits the wrong one simply fails to connect.
    xdg_runtime_dir: String,
    /// `DBUS_SESSION_BUS_ADDRESS` for the per-sidecar session bus.
    /// `None` if dbus-daemon failed to start. Kept from the X11 sidecar
    /// verbatim: GTK apps under Wayland still export their AppMenu and
    /// their notifications over the session bus, and a missing bus makes
    /// some of them log-spam on every startup.
    dbus_session_address: Option<String>,
    /// Every pid we've ever spawned, alive or reaped. Some apps
    /// fork+exit a wrapper whose child connects to the compositor
    /// *after* the wrapper has been reaped; walking /proc PPid up from
    /// the connecting peer would then never hit the live `processes`
    /// set. See `find_ancestor_pid`.
    spawned_pid_history: HashSet<u32>,
}

struct ManagedProcess {
    child: Child,
    command: String,
}

impl ProcessManager {
    pub fn new(
        wayland_display: String,
        xdg_runtime_dir: String,
        dbus_session_address: Option<String>,
    ) -> Self {
        Self {
            processes: HashMap::new(),
            wayland_display,
            xdg_runtime_dir,
            dbus_session_address,
            spawned_pid_history: HashSet::new(),
        }
    }

    pub async fn spawn(&mut self, command: &str, args: &[String]) -> Result<u32, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .env("WAYLAND_DISPLAY", &self.wayland_display)
            .env("XDG_RUNTIME_DIR", &self.xdg_runtime_dir)
            // Toolkit backend hints. Every one of these toolkits
            // auto-detects Wayland from `WAYLAND_DISPLAY` already —
            // but each also has a fallback path that silently prefers
            // X11 when both are plausible, and a client that picks X11
            // in this container finds no X server and dies with a
            // message the user never sees. Same set waylandcraft puts
            // on its children (native/src/bridge.rs `exec_app`), plus
            // the two it doesn't need (Firefox, SDL).
            .env("GDK_BACKEND", "wayland")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("MOZ_ENABLE_WAYLAND", "1")
            .env("SDL_VIDEODRIVER", "wayland")
            .env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
            .env("XDG_SESSION_TYPE", "wayland")
            // There is no XWayland in this image, so an inherited
            // `DISPLAY` can only ever point somewhere that does not
            // answer. Removing it turns "hangs for 30s then dies" into
            // "uses Wayland", and makes the hints above redundant
            // rather than load-bearing.
            .env_remove("DISPLAY")
            .env_remove("XAUTHORITY")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(addr) = &self.dbus_session_address {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", addr);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {e}"))?;

        let pid = child.id().ok_or("Failed to get process id")?;

        // Drain child stdout/stderr into our own log. The pipes are
        // never read otherwise, so any app chatty enough to fill the
        // 64KB pipe buffer (GTK warnings, Mesa's software-rendering
        // notices, a terminal echoing a build log…) would block on
        // write and appear to hang. Also makes child diagnostics
        // visible in `docker logs`.
        if let Some(stdout) = child.stdout.take() {
            let cmd_name = command.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!("[{cmd_name}:{pid}:stdout] {line}");
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let cmd_name = command.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!("[{cmd_name}:{pid}:stderr] {line}");
                }
            });
        }
        self.processes.insert(
            pid,
            ManagedProcess {
                child,
                command: command.to_string(),
            },
        );
        self.spawned_pid_history.insert(pid);
        info!(
            "Spawned process: {} (pid {}) with WAYLAND_DISPLAY={}",
            command, pid, self.wayland_display
        );
        if let Some(m) = telemetry::metrics() {
            m.processes_spawned.add(1, &[]);
        }
        Ok(pid)
    }

    pub async fn kill(&mut self, pid: u32) -> Result<(), String> {
        if let Some(mut proc) = self.processes.remove(&pid) {
            proc.child
                .kill()
                .await
                .map_err(|e| format!("Failed to kill process {pid}: {e}"))?;
            info!("Killed process: {}", pid);
            Ok(())
        } else {
            Err(format!("Process {pid} not found"))
        }
    }

    pub fn get_command(&self, pid: u32) -> Option<&str> {
        self.processes.get(&pid).map(|p| p.command.as_str())
    }

    pub fn list(&self) -> Vec<x11_web_wire::SpawnedProcessInfo> {
        self.processes
            .iter()
            .map(|(&pid, proc)| x11_web_wire::SpawnedProcessInfo {
                pid,
                command: proc.command.clone(),
            })
            .collect()
    }

    /// Every pid we've ever spawned, alive or reaped. Used by
    /// [`find_ancestor_pid`] so wrapper-then-exit launchers still
    /// resolve their connecting descendant.
    pub fn spawned_pid_history(&self) -> &HashSet<u32> {
        &self.spawned_pid_history
    }

    pub async fn check_exited(&mut self) -> Vec<(u32, Option<i32>)> {
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();

        for (&pid, proc) in self.processes.iter_mut() {
            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    exited.push((pid, status.code()));
                    to_remove.push(pid);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("Error checking process {pid}: {e}");
                    to_remove.push(pid);
                    exited.push((pid, None));
                }
            }
        }

        for pid in to_remove {
            self.processes.remove(&pid);
        }

        exited
    }
}

/// Result of starting the per-sidecar DBus session bus. Holding the
/// `Child` keeps the daemon alive for the lifetime of the sidecar
/// process; dropping it would let dbus-daemon exit.
pub struct DbusSession {
    pub address: String,
    /// RAII guard — keeps the dbus-daemon child process alive. Never
    /// read directly; the Drop on Child is what matters.
    #[allow(dead_code)]
    daemon: Child,
}

/// Spawn a private `dbus-daemon --session` and return its bus address.
///
/// The daemon prints its address on stdout when invoked with
/// `--print-address`. We read the first line, then leave the process
/// running for the lifetime of the sidecar. Errors are non-fatal: if
/// dbus-daemon isn't installed (e.g. local dev outside the container)
/// the sidecar continues without DBus support and AppMenu export
/// simply won't work.
///
/// Kept even though this crate bridges no menus of its own: the bus is
/// for the *children*, not for us. GTK and Qt apps under Wayland still
/// talk to it for AppMenu export, notifications and portal lookups, and
/// an absent bus makes several of them retry-and-log on every launch.
pub async fn start_dbus_session() -> Option<DbusSession> {
    let mut child = match Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("dbus-daemon not started ({e}); AppMenu export disabled");
            return None;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            warn!("dbus-daemon stdout unavailable; killing daemon");
            let _ = child.start_kill();
            return None;
        }
    };

    let mut lines = BufReader::new(stdout).lines();
    let address = match tokio::time::timeout(DBUS_STARTUP_TIMEOUT, lines.next_line()).await {
        Ok(Ok(Some(line))) => line.trim().to_string(),
        Ok(Ok(None)) => {
            warn!("dbus-daemon closed stdout before printing address");
            let _ = child.start_kill();
            return None;
        }
        Ok(Err(e)) => {
            warn!("Failed reading dbus-daemon stdout: {e}");
            let _ = child.start_kill();
            return None;
        }
        Err(_) => {
            warn!("dbus-daemon address read timed out after 5s");
            let _ = child.start_kill();
            return None;
        }
    };

    if address.is_empty() {
        warn!("dbus-daemon printed empty address");
        let _ = child.start_kill();
        return None;
    }

    info!("Started session DBus at {address}");
    Some(DbusSession {
        address,
        daemon: child,
    })
}

/// Walk up the process tree from `peer_pid` to find the first ancestor
/// that is in `spawned_pids`. Returns `Some(pid)` if found.
/// This uses /proc/<pid>/status to read PPid.
pub fn find_ancestor_pid(peer_pid: u32, spawned_pids: &[u32]) -> Option<u32> {
    if peer_pid == 0 {
        return None;
    }
    // Check if the peer itself is a spawned process
    if spawned_pids.contains(&peer_pid) {
        return Some(peer_pid);
    }
    // Walk up the tree
    let mut current = peer_pid;
    for _ in 0..MAX_PROCESS_PARENT_DEPTH {
        let ppid = get_ppid(current);
        match ppid {
            Some(0) | Some(1) | None => return None, // reached init or failed
            Some(p) => {
                if spawned_pids.contains(&p) {
                    return Some(p);
                }
                current = p;
            }
        }
    }
    None
}

/// Read the parent PID of a process from /proc/<pid>/status.
fn get_ppid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}
