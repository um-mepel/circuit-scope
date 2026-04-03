use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtyPair, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

/// Session-local `ZDOTDIR` so we source the user's `~/.zprofile` / `~/.zshrc`, then prepend
/// `scripts/` to `PATH`. Otherwise `~/.zprofile` often runs `export PATH="$HOME/.cargo/bin:$PATH"`
/// after our env and **`csverilog` resolves to a stale `~/.cargo/bin/csverilog`**.
#[cfg(unix)]
fn shell_binary_is_zsh(shell_bin: &str) -> bool {
    shell_bin.ends_with("/zsh") || shell_bin == "zsh"
}

#[cfg(unix)]
fn circuit_scope_zdotdir(session_id: u64) -> Result<PathBuf> {
    let zdot = std::env::temp_dir().join(format!("circuit-scope-zdot-{session_id}"));
    if zdot.exists() {
        std::fs::remove_dir_all(&zdot)?;
    }
    std::fs::create_dir_all(&zdot)?;
    std::fs::write(
        zdot.join(".zprofile"),
        r#"# Circuit Scope integrated terminal — user login profile first
if [[ -f "$HOME/.zprofile" ]]; then
  source "$HOME/.zprofile"
fi
"#,
    )?;
    std::fs::write(
        zdot.join(".zshrc"),
        r#"# Circuit Scope integrated terminal — user interactive config, then project `csverilog`
if [[ -f "$HOME/.zshrc" ]]; then
  source "$HOME/.zshrc"
fi
if [[ -n "$CIRCUIT_SCOPE_SCRIPTS_DIR" ]]; then
  export PATH="$CIRCUIT_SCOPE_SCRIPTS_DIR:$PATH"
  hash -r 2>/dev/null || true
  typeset _want="$CIRCUIT_SCOPE_SCRIPTS_DIR/csverilog"
  typeset _res
  _res="$(command -v csverilog 2>/dev/null)" || _res=""
  if [[ -n "$_res" && -e "$_want" && ! "$_res" -ef "$_want" ]]; then
    print -u2 "[Circuit Scope] csverilog is '$_res' (expected project launcher '$_want'). Another PATH entry may be taking precedence."
  fi
  unset _want _res
fi
"#,
    )?;
    Ok(zdot)
}

/// Walk ancestors of `start` for `scripts/csverilog` (`csverilog.cmd` on Windows).
fn csverilog_scripts_dir_walking_up(mut dir: &Path) -> Option<PathBuf> {
    loop {
        let scripts = dir.join("scripts");
        #[cfg(windows)]
        if scripts.join("csverilog.cmd").is_file() {
            return Some(scripts);
        }
        #[cfg(not(windows))]
        if scripts.join("csverilog").is_file() {
            return Some(scripts);
        }
        dir = dir.parent()?;
    }
}

/// `scripts/` next to the opened folder or any ancestor (user opened a repo / subfolder).
fn workspace_csverilog_scripts_dir(project_root: &Path) -> Option<PathBuf> {
    csverilog_scripts_dir_walking_up(project_root)
}

/// **`scripts/` beside the running app** (e.g. `target/debug/circuit_scope` → repo `scripts/`) when the
/// opened project is an arbitrary folder (course lab, no `scripts/csverilog` in that tree).
fn host_csverilog_scripts_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let start = exe.parent()?;
    csverilog_scripts_dir_walking_up(start)
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SessionId(pub u64);

/// Shared child handle so the reader thread can wait() for real exit code when EOF is seen.
type ChildHandle = Arc<Mutex<Option<Box<dyn portable_pty::Child + Send>>>>;
/// Shared writer handle so we can keep the PTY stdin open across writes.
type WriterHandle = Arc<Mutex<Box<dyn Write + Send>>>;

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: WriterHandle,
    child: ChildHandle,
    /// Remove session `ZDOTDIR` after the shell exits (login zsh only).
    #[cfg(unix)]
    zdotdir: Option<PathBuf>,
}

pub struct PtyManager {
    next_id: Mutex<u64>,
    sessions: Mutex<HashMap<SessionId, PtySession>>,
}

#[derive(Debug, Serialize, Clone)]
struct PtyDataPayload {
    sessionId: u64,
    data: String,
}

#[derive(Debug, Serialize, Clone)]
struct PtyExitPayload {
    sessionId: u64,
    code: i32,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            next_id: Mutex::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn create_session(
        &self,
        app: &tauri::AppHandle,
        shell: Option<String>,
        cwd: Option<String>,
    ) -> Result<SessionId> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let id = {
            let mut next_id = self.next_id.lock().unwrap();
            let id = SessionId(*next_id);
            *next_id += 1;
            id
        };

        #[cfg(unix)]
        let mut zdotdir: Option<PathBuf> = None;

        let shell_bin = shell.as_deref().unwrap_or("/bin/zsh").to_string();
        let mut cmd = CommandBuilder::new(shell_bin.clone());
        cmd.args(&["-l"]);
        if let Some(ref dir) = cwd {
            let root = Path::new(dir);
            cmd.cwd(root);
            cmd.env("CIRCUIT_SCOPE_PROJECT_ROOT", dir);
            let scripts_dir = workspace_csverilog_scripts_dir(root).or_else(host_csverilog_scripts_dir);
            if let Some(scripts_dir) = scripts_dir {
                let scripts_abs = std::fs::canonicalize(&scripts_dir).unwrap_or_else(|_| scripts_dir.clone());
                let scripts_for_env = scripts_abs
                    .to_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| scripts_dir.to_string_lossy().into_owned());

                #[cfg(unix)]
                {
                    if shell_binary_is_zsh(&shell_bin) {
                        match circuit_scope_zdotdir(id.0) {
                            Ok(z) => {
                                cmd.env("ZDOTDIR", z.to_string_lossy().as_ref());
                                cmd.env("CIRCUIT_SCOPE_SCRIPTS_DIR", &scripts_for_env);
                                zdotdir = Some(z);
                            }
                            Err(_) => {
                                let mut path_var = std::env::var("PATH").unwrap_or_default();
                                path_var = format!("{}:{}", scripts_abs.display(), path_var);
                                cmd.env("PATH", path_var);
                            }
                        }
                    } else {
                        let mut path_var = std::env::var("PATH").unwrap_or_default();
                        path_var = format!("{}:{}", scripts_abs.display(), path_var);
                        cmd.env("PATH", path_var);
                    }
                }

                #[cfg(windows)]
                {
                    let mut path_var = std::env::var("PATH").unwrap_or_default();
                    path_var = format!("{};{}", scripts_abs.display(), path_var);
                    cmd.env("PATH", path_var);
                }
            }
        }

        let PtyPair { mut master, slave } = pair;
        let mut reader = master.try_clone_reader()?;
        let writer = master.take_writer()?;
        let child = slave.spawn_command(cmd)?;
        let child_handle: ChildHandle = Arc::new(Mutex::new(Some(child)));
        let writer_handle: WriterHandle = Arc::new(Mutex::new(writer));

        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(
                id,
                PtySession {
                    master,
                    writer: Arc::clone(&writer_handle),
                    child: Arc::clone(&child_handle),
                    #[cfg(unix)]
                    zdotdir,
                },
            );
        }
        let app_handle = app.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let payload = PtyDataPayload {
                            sessionId: id.0,
                            data,
                        };
                        let _ = app_handle.emit("pty-data", payload);
                    }
                    Err(_) => break,
                }
            }
            let code = child_handle
                .lock()
                .ok()
                .and_then(|mut opt| opt.take())
                .and_then(|mut c| c.wait().ok())
                .map(|status| status.exit_code() as i32)
                .unwrap_or(-1);
            let payload = PtyExitPayload {
                sessionId: id.0,
                code,
            };
            let _ = app_handle.emit("pty-exit", payload);
        });

        Ok(id)
    }

    pub fn write(&self, session: SessionId, data: &str) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.get(&session) {
            if let Ok(mut writer) = s.writer.lock() {
                writer.write_all(data.as_bytes())?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    pub fn resize(&self, session: SessionId, cols: u16, rows: u16) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.get(&session) {
            s.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }
        Ok(())
    }

    pub fn close(&self, session: SessionId) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.remove(&session) {
            #[cfg(unix)]
            if let Some(ref z) = s.zdotdir {
                let _ = std::fs::remove_dir_all(z);
            }
            if let Ok(mut opt) = s.child.lock() {
                if let Some(mut c) = opt.take() {
                    let _ = c.kill();
                }
            }
        }
        Ok(())
    }
}

