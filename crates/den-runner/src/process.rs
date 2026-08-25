use std::path::PathBuf;
use std::process::Child;

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

    pub fn stop(&mut self) -> std::io::Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}
