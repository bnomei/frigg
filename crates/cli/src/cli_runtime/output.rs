//! Small CLI output policy layer for stable stdout results and stderr diagnostics.

use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Quiet,
    Normal,
    Verbose,
}

impl OutputMode {
    pub(crate) fn resolve(quiet: bool, verbose: bool) -> io::Result<Self> {
        match (quiet, verbose) {
            (true, true) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--quiet and --verbose cannot be used together",
            )),
            (true, false) => Ok(Self::Quiet),
            (false, true) => Ok(Self::Verbose),
            (false, false) => Ok(Self::Normal),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CliOutput {
    mode: OutputMode,
}

impl CliOutput {
    pub(crate) const fn new(mode: OutputMode) -> Self {
        Self { mode }
    }

    #[cfg(test)]
    pub(crate) const fn normal() -> Self {
        Self::new(OutputMode::Normal)
    }

    pub(crate) fn from_flags(quiet: bool, verbose: bool) -> io::Result<Self> {
        Ok(Self::new(OutputMode::resolve(quiet, verbose)?))
    }

    pub(crate) const fn is_quiet(self) -> bool {
        matches!(self.mode, OutputMode::Quiet)
    }

    pub(crate) const fn is_verbose(self) -> bool {
        matches!(self.mode, OutputMode::Verbose)
    }

    pub(crate) fn result(self, message: impl AsRef<str>) -> io::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        write_stdout_line(message.as_ref())
    }

    pub(crate) fn summary(self, message: impl AsRef<str>) -> io::Result<()> {
        self.result(message)
    }

    #[allow(dead_code)]
    pub(crate) fn warning(self, message: impl AsRef<str>) -> io::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        write_stderr_line(message.as_ref())
    }

    pub(crate) fn error(self, message: impl AsRef<str>) -> io::Result<()> {
        write_stderr_line(message.as_ref())
    }

    pub(crate) fn progress(self, message: impl AsRef<str>) -> io::Result<()> {
        if !self.is_verbose() {
            return Ok(());
        }
        write_stderr_line(message.as_ref())
    }

    pub(crate) fn diagnostic(self, message: impl AsRef<str>) -> io::Result<()> {
        self.progress(message)
    }

    pub(crate) fn advisory(self, message: impl AsRef<str>) -> io::Result<()> {
        self.progress(message)
    }
}

fn write_stdout_line(message: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{message}")
}

fn write_stderr_line(message: &str) -> io::Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    writeln!(handle, "{message}")
}

#[cfg(test)]
mod tests {
    use super::OutputMode;

    #[test]
    fn output_mode_resolves_quiet() {
        assert_eq!(OutputMode::resolve(true, false).unwrap(), OutputMode::Quiet);
    }

    #[test]
    fn output_mode_resolves_normal() {
        assert_eq!(
            OutputMode::resolve(false, false).unwrap(),
            OutputMode::Normal
        );
    }

    #[test]
    fn output_mode_resolves_verbose() {
        assert_eq!(
            OutputMode::resolve(false, true).unwrap(),
            OutputMode::Verbose
        );
    }

    #[test]
    fn output_mode_rejects_quiet_verbose_conflict() {
        let error = OutputMode::resolve(true, true)
            .expect_err("quiet and verbose must not be accepted together");
        assert!(error.to_string().contains("--quiet and --verbose"));
    }
}
