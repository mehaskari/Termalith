//! Shell selection. The UI never supplies a program path; the backend picks
//! one from OS configuration.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::PtyError;

/// A fully resolved program to run inside a PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl ShellSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    fn with_terminal_env(self) -> Self {
        self.env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "Termalith")
    }
}

/// The user's default interactive shell for this OS.
pub fn default_shell() -> Result<ShellSpec, PtyError> {
    platform::default_shell().map(ShellSpec::with_terminal_env)
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    const FALLBACKS: [&str; 3] = ["/bin/zsh", "/bin/bash", "/bin/sh"];

    pub(super) fn default_shell() -> Result<ShellSpec, PtyError> {
        let etc_shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let program = resolve(std::env::var_os("SHELL").as_deref(), &etc_shells, |p| {
            p.is_file()
        })?;
        // Login shell, matching common macOS terminal behaviour, so PATH is
        // initialised even when the app is launched from Finder.
        let mut spec = ShellSpec::new(program).arg("-l");
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
            && home.is_absolute()
            && home.is_dir()
        {
            spec = spec.cwd(home);
        }
        Ok(spec)
    }

    /// Accepts `$SHELL` only if it is absolute, listed in `/etc/shells` and
    /// exists; otherwise falls back to the first listed, existing standard
    /// shell.
    pub(super) fn resolve(
        env_shell: Option<&OsStr>,
        etc_shells: &str,
        exists: impl Fn(&Path) -> bool,
    ) -> Result<PathBuf, PtyError> {
        let listed: Vec<&Path> = etc_shells
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(Path::new)
            .collect();
        let acceptable = |p: &Path| p.is_absolute() && listed.contains(&p) && exists(p);

        if let Some(candidate) = env_shell.map(Path::new)
            && acceptable(candidate)
        {
            return Ok(candidate.to_path_buf());
        }
        FALLBACKS
            .iter()
            .map(Path::new)
            .find(|p| acceptable(p))
            .map(Path::to_path_buf)
            .ok_or(PtyError::NoShell)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const SHELLS: &str = "# comment\n/bin/bash\n/bin/zsh\n\n/bin/sh\n/opt/homebrew/bin/fish\n";

        fn all_exist(_: &Path) -> bool {
            true
        }

        #[test]
        fn uses_listed_env_shell() {
            let p = resolve(
                Some(OsStr::new("/opt/homebrew/bin/fish")),
                SHELLS,
                all_exist,
            );
            assert_eq!(p.unwrap(), PathBuf::from("/opt/homebrew/bin/fish"));
        }

        #[test]
        fn rejects_unlisted_env_shell() {
            let p = resolve(Some(OsStr::new("/tmp/evil")), SHELLS, all_exist);
            assert_eq!(p.unwrap(), PathBuf::from("/bin/zsh"));
        }

        #[test]
        fn rejects_relative_env_shell() {
            let p = resolve(
                Some(OsStr::new("bin/bash")),
                "bin/bash\n/bin/sh\n",
                all_exist,
            );
            assert_eq!(p.unwrap(), PathBuf::from("/bin/sh"));
        }

        #[test]
        fn rejects_missing_env_shell() {
            let p = resolve(Some(OsStr::new("/bin/bash")), SHELLS, |p| {
                p != Path::new("/bin/bash")
            });
            assert_eq!(p.unwrap(), PathBuf::from("/bin/zsh"));
        }

        #[test]
        fn errors_when_nothing_usable() {
            assert!(matches!(
                resolve(None, "", all_exist),
                Err(PtyError::NoShell)
            ));
        }
    }
}

// Not yet exercised on Windows; validated in Phase 0b on a native runner.
#[cfg(windows)]
mod platform {
    use super::*;

    pub(super) fn default_shell() -> Result<ShellSpec, PtyError> {
        let comspec = std::env::var_os("COMSPEC").map(PathBuf::from);
        let program = comspec
            .filter(|p| p.is_absolute() && p.is_file())
            .or_else(|| {
                let p = PathBuf::from(r"C:\Windows\System32\cmd.exe");
                p.is_file().then_some(p)
            })
            .ok_or(PtyError::NoShell)?;
        let mut spec = ShellSpec::new(program);
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from)
            && home.is_dir()
        {
            spec = spec.cwd(home);
        }
        Ok(spec)
    }
}
