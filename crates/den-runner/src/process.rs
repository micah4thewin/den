use std::path::PathBuf;
use std::process::Child;

/// How long a polite quit is given before the process is taken down hard.
/// RetroArch writes its save RAM and, with `savestate_auto_save` on, an exit
/// state; that is worth a few seconds of waiting and no longer.
#[cfg(unix)]
pub(crate) const GRACE: std::time::Duration = std::time::Duration::from_secs(3);
#[cfg(unix)]
const GRACE_TICK: std::time::Duration = std::time::Duration::from_millis(50);

pub struct Running {
    pub(crate) child: Child,
    pub config_path: PathBuf,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn wait(&mut self) -> std::io::Result<Option<i32>> {
        match self.child.try_wait()? {
            Some(status) => Ok(status.code()),
            None => Ok(self.child.wait()?.code()),
        }
    }

    /// Ask the game to quit the way pressing Escape would, and kill it only if
    /// it will not go. RetroArch answers `SIGTERM` by shutting down cleanly;
    /// `SIGKILL` takes the save with it, so the difference is a lost session.
    pub fn quit(&mut self) -> std::io::Result<()> {
        if !self.is_running() {
            let _ = self.child.wait();
            return Ok(());
        }
        #[cfg(unix)]
        if self.asked_and_went() {
            return Ok(());
        }
        self.stop()
    }

    /// `SIGTERM`, then wait out the grace period; true if it went on its own.
    ///
    /// `std::process` can send nothing but `SIGKILL`, and sending a signal
    /// directly means `unsafe`, which this workspace denies — so ask `kill(1)`,
    /// which is in every POSIX base system. Windows has no polite signal to
    /// send, so there `quit` is `stop` under another name.
    #[cfg(unix)]
    fn asked_and_went(&mut self) -> bool {
        use std::process::{Command, Stdio};
        use std::time::Instant;

        let sent = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !sent {
            return false;
        }
        let deadline = Instant::now() + GRACE;
        while Instant::now() < deadline {
            if !self.is_running() {
                let _ = self.child.wait();
                return true;
            }
            std::thread::sleep(GRACE_TICK);
        }
        false
    }

    pub fn stop(&mut self) -> std::io::Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    fn spawn(program: &str, args: &[&str]) -> Running {
        let child = Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");
        Running {
            child,
            config_path: PathBuf::from("unused.cfg"),
        }
    }

    #[test]
    fn a_polite_quit_is_enough_for_a_process_that_takes_the_hint() {
        let mut running = spawn("sleep", &["30"]);
        let started = Instant::now();
        running.quit().expect("quit");
        assert!(!running.is_running());
        // It answered the signal, so it never waited out the whole grace.
        assert!(
            started.elapsed() < GRACE,
            "quit waited out the grace period"
        );
    }

    #[test]
    fn a_process_that_ignores_the_signal_is_still_stopped() {
        let mut running = spawn("sh", &["-c", "trap '' TERM; sleep 30"]);
        running.quit().expect("quit");
        assert!(!running.is_running());
    }

    #[test]
    fn quitting_something_already_gone_is_not_an_error() {
        let mut running = spawn("true", &[]);
        running.quit().expect("first quit");
        running.quit().expect("second quit");
        assert!(!running.is_running());
    }
}
