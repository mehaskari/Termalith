//! Platform boundary for pseudo-terminals.
//!
//! [`NativePtyBackend`] uses `portable-pty` (Unix PTY / Windows ConPTY). It is
//! kept behind [`PtyBackend`] so it can be replaced without touching the
//! session manager or the UI layer.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::{ExitInfo, PtyError, ShellSpec, TerminalSize};

/// A freshly spawned PTY: the output stream and a control handle.
pub struct SpawnedPty {
    pub reader: Box<dyn Read + Send>,
    pub control: Box<dyn PtyControl>,
}

/// Creates PTY sessions. Implementations must not perform shell selection;
/// they run exactly the [`ShellSpec`] they are given.
pub trait PtyBackend: Send + Sync {
    fn spawn(&self, shell: &ShellSpec, size: TerminalSize) -> Result<SpawnedPty, PtyError>;
}

/// Control half of a PTY session.
pub trait PtyControl: Send {
    fn write_all(&mut self, data: &[u8]) -> Result<(), PtyError>;
    fn resize(&self, size: TerminalSize) -> Result<(), PtyError>;
    fn try_wait(&mut self) -> Result<Option<ExitInfo>, PtyError>;
    fn pid(&self) -> Option<u32>;
    /// Ask the child to exit, escalating to a forced kill after `grace`.
    /// Returns once the child has exited or the forced kill was issued.
    fn terminate(&mut self, grace: Duration) -> Result<(), PtyError>;
    /// Release the PTY master and input writer once the child has exited.
    /// Output already produced remains readable until EOF. Required on
    /// Windows, where the ConPTY output pipe only reports EOF after the
    /// pseudo console is closed. May block while the pseudo console flushes.
    fn release_pty(&mut self);
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NativePtyBackend;

impl PtyBackend for NativePtyBackend {
    fn spawn(&self, shell: &ShellSpec, size: TerminalSize) -> Result<SpawnedPty, PtyError> {
        let pair = native_pty_system()
            .openpty(to_pty_size(size))
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        if let Some(cwd) = &shell.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &shell.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        // Drop our copy of the slave so reads on the master see EOF once the
        // child (and its descendants) close it.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Open(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Open(e.to_string()))?;

        Ok(SpawnedPty {
            reader,
            control: Box::new(NativeControl {
                master: Some(pair.master),
                writer: Some(writer),
                child,
            }),
        })
    }
}

struct NativeControl {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
}

impl PtyControl for NativeControl {
    fn write_all(&mut self, data: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or_else(released)?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    fn resize(&self, size: TerminalSize) -> Result<(), PtyError> {
        self.master
            .as_ref()
            .ok_or_else(released)?
            .resize(to_pty_size(size))
            .map_err(|e| PtyError::Io(std::io::Error::other(e.to_string())))
    }

    fn try_wait(&mut self) -> Result<Option<ExitInfo>, PtyError> {
        Ok(self.child.try_wait()?.map(|s| ExitInfo {
            code: s.exit_code(),
            signal: s.signal().map(str::to_owned),
        }))
    }

    fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    fn terminate(&mut self, grace: Duration) -> Result<(), PtyError> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        // Unix: SIGHUP (what a closing terminal sends). Windows: TerminateProcess.
        // Either may fail if the child exited in the meantime.
        let _ = self.child.kill();

        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        force_kill(self.pid());
        Ok(())
    }

    fn release_pty(&mut self) {
        self.writer = None;
        self.master = None;
    }
}

fn released() -> PtyError {
    PtyError::Io(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pseudo-terminal already released",
    ))
}

#[cfg(unix)]
fn force_kill(pid: Option<u32>) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

#[cfg(windows)]
fn force_kill(_pid: Option<u32>) {
    // portable-pty's kill() on Windows is already TerminateProcess.
}

fn to_pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows(),
        cols: size.cols(),
        pixel_width: 0,
        pixel_height: 0,
    }
}
