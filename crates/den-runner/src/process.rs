use std::path::PathBuf;
use std::process::Child;

/// A launched RetroArch process, owned here.
pub struct Running {
    pub(crate) child: Child,
    /// The private config this process was launched with.
    pub config_path: PathBuf,
}

impl Running {
    /// The process id of the launched RetroArch.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether the process is still running (non-blocking).
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the process to exit; returns its exit code.
    pub fn wait(&mut self) -> std::io::Result<Option<i32>> {
        match self.child.try_wait()? {
            Some(status) => Ok(status.code()),
            None => Ok(self.child.wait()?.code()),
        }
    }

    /// Stop the process.
    pub fn stop(&mut self) -> std::io::Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}
