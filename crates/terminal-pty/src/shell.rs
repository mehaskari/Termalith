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

    /// The variables terminal programs look for (TERM and friends).
    pub fn with_terminal_env(self) -> Self {
        self.env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "Termalith")
    }
}

/// The user's default interactive shell for this OS.
pub fn default_shell() -> Result<ShellSpec, PtyError> {
    platform::default_shell().map(ShellSpec::with_terminal_env)
}

/// A shell installed on this machine that the user may pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellProfile {
    /// Stable identifier (the program path, or `wsl:<distro>`).
    pub id: String,
    pub name: String,
    pub spec: ShellSpec,
}

/// Installed shells, the default one first. Only these can be started:
/// the UI picks an id, never a command line.
pub fn available_shells() -> Vec<ShellProfile> {
    let default = platform::default_shell().ok();
    let mut out = platform::installed_shells();
    if let Some(d) = default {
        let id = d.program.to_string_lossy().into_owned();
        match out.iter().position(|p| p.id == id) {
            Some(i) => {
                let p = out.remove(i);
                out.insert(0, p);
            }
            None => out.insert(
                0,
                ShellProfile {
                    name: display_name(&d.program),
                    id,
                    spec: d,
                },
            ),
        }
    }
    for p in &mut out {
        p.spec = std::mem::replace(&mut p.spec, ShellSpec::new("")).with_terminal_env();
    }
    out
}

/// The installed shell with this id.
pub fn shell_by_id(id: &str) -> Result<ShellSpec, PtyError> {
    available_shells()
        .into_iter()
        .find(|p| p.id == id)
        .map(|p| p.spec)
        .ok_or(PtyError::NoShell)
}

fn display_name(program: &std::path::Path) -> String {
    program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.display().to_string())
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

    /// Existing shells listed in `/etc/shells` (same file reached through
    /// a symlink is listed once), as login shells in the home directory.
    pub(super) fn installed_shells() -> Vec<ShellProfile> {
        let etc_shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| h.is_absolute() && h.is_dir());
        let profiles = list(
            &etc_shells,
            |p| p.is_file(),
            |p| std::fs::canonicalize(p).ok(),
        )
        .into_iter()
        .map(|program| {
            let mut spec = ShellSpec::new(program.clone()).arg("-l");
            if let Some(h) = &home {
                spec = spec.cwd(h.clone());
            }
            ShellProfile {
                id: program.to_string_lossy().into_owned(),
                name: String::new(),
                spec,
            }
        })
        .collect();
        with_names(profiles)
    }

    /// "zsh", or "bash (/usr/local/bin/bash)" when names repeat.
    fn with_names(mut profiles: Vec<ShellProfile>) -> Vec<ShellProfile> {
        let names: Vec<String> = profiles
            .iter()
            .map(|p| display_name(&p.spec.program))
            .collect();
        for (p, n) in profiles.iter_mut().zip(&names) {
            p.name = if names.iter().filter(|x| *x == n).count() > 1 {
                format!("{n} ({})", p.spec.program.display())
            } else {
                n.clone()
            };
        }
        profiles
    }

    /// Absolute, existing entries of `/etc/shells`, without duplicates of
    /// the same file. `nologin`-style entries are skipped.
    pub(super) fn list(
        etc_shells: &str,
        exists: impl Fn(&Path) -> bool,
        canonical: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Vec<PathBuf> {
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut out = Vec::new();
        for l in etc_shells.lines().map(str::trim) {
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let p = Path::new(l);
            if !p.is_absolute() || !exists(p) || l.ends_with("nologin") || l.ends_with("/false") {
                continue;
            }
            let real = canonical(p).unwrap_or_else(|| p.to_path_buf());
            if seen.contains(&real) {
                continue;
            }
            seen.push(real);
            out.push(p.to_path_buf());
        }
        out
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
        fn lists_existing_unique_shells() {
            let shells =
                "/bin/bash\n/usr/bin/bash\n/bin/zsh\n/usr/sbin/nologin\nrel/sh\n/missing\n";
            let got = list(
                shells,
                |p| p != Path::new("/missing"),
                |p| {
                    Some(if p == Path::new("/usr/bin/bash") {
                        PathBuf::from("/bin/bash")
                    } else {
                        p.to_path_buf()
                    })
                },
            );
            assert_eq!(got, [PathBuf::from("/bin/bash"), PathBuf::from("/bin/zsh")]);
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
        if let Some(home) = home() {
            spec = spec.cwd(home);
        }
        Ok(spec)
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .filter(|h| h.is_dir())
    }

    fn system32() -> PathBuf {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32")
    }

    /// PowerShell 7, Windows PowerShell, cmd, Git Bash and WSL distributions
    /// that exist on this machine. Not yet verified on a real Windows
    /// desktop (CI only).
    pub(super) fn installed_shells() -> Vec<ShellProfile> {
        let program_files = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        let sys = system32();
        let candidates: [(&str, PathBuf, &[&str]); 4] = [
            (
                "PowerShell",
                program_files.join(r"PowerShell\7\pwsh.exe"),
                &["-NoLogo"],
            ),
            (
                "Windows PowerShell",
                sys.join(r"WindowsPowerShell\v1.0\powershell.exe"),
                &["-NoLogo"],
            ),
            ("Command Prompt", sys.join("cmd.exe"), &[]),
            (
                "Git Bash",
                program_files.join(r"Git\bin\bash.exe"),
                &["--login", "-i"],
            ),
        ];
        let mut out: Vec<ShellProfile> = candidates
            .into_iter()
            .filter(|(_, p, _)| p.is_file())
            .map(|(name, program, args)| {
                let mut spec = ShellSpec::new(program.clone());
                for a in args {
                    spec = spec.arg(*a);
                }
                if let Some(h) = home() {
                    spec = spec.cwd(h);
                }
                ShellProfile {
                    id: program.to_string_lossy().into_owned(),
                    name: name.to_owned(),
                    spec,
                }
            })
            .collect();
        let wsl = sys.join("wsl.exe");
        if wsl.is_file() {
            for distro in wsl_distros(&wsl) {
                out.push(ShellProfile {
                    id: format!("wsl:{distro}"),
                    name: format!("WSL: {distro}"),
                    spec: ShellSpec::new(wsl.clone())
                        .arg("-d")
                        .arg(distro.clone())
                        .arg("--cd")
                        .arg("~"),
                });
            }
        }
        out
    }

    /// `wsl.exe -l -q` prints UTF-16LE names, one per line.
    fn wsl_distros(wsl: &std::path::Path) -> Vec<String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(out) = std::process::Command::new(wsl)
            .args(["-l", "-q"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return Vec::new();
        };
        parse_wsl_list(&out.stdout)
    }

    pub(super) fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
            .lines()
            .map(|l| {
                l.trim()
                    .trim_matches('\u{feff}')
                    .trim_matches('\0')
                    .to_owned()
            })
            .filter(|l| {
                !l.is_empty()
                    && l.len() <= 64
                    && l.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn parses_wsl_list_output() {
            let text = "Ubuntu-22.04\r\ndocker-desktop\r\nbad name\r\n\r\n";
            let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert_eq!(
                super::parse_wsl_list(&bytes),
                ["Ubuntu-22.04", "docker-desktop"]
            );
        }
    }
}
