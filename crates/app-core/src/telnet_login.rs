//! Automatic Telnet login (ADR-044): watches what the device prints and
//! types the user name at a login prompt and the password at a password
//! prompt, once each, within the first minute.
//!
//! Telnet has no login result, so success is guessed from the output: after
//! the password, a line that ends like a command prompt (`#`, `>`, `$`, `%`)
//! and no failure message. Only then may a typed password be saved.

use std::time::{Duration, Instant};

use zeroize::Zeroizing;

/// How long after connecting the prompts are answered.
pub const LOGIN_WINDOW: Duration = Duration::from_secs(60);
/// Characters of recent output kept for matching.
const TAIL_CHARS: usize = 256;

/// What the watcher wants done after a chunk of output.
#[derive(Debug, PartialEq, Eq)]
pub enum LoginStep {
    /// Nothing to do.
    Wait,
    /// Type these bytes (a user name or password, with Enter).
    Send(Zeroizing<Vec<u8>>),
    /// The device accepted the login.
    Succeeded,
    /// The device refused it (or asked again); the user takes over.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for a login or password prompt.
    Prompts,
    /// Password sent; waiting for a command prompt or a refusal.
    Checking,
    /// Finished (either way): stays silent.
    Done,
}

/// Automatic login for a Telnet terminal: the watcher and what to do once
/// the device accepted the login (e.g. save a typed password).
pub struct AutoLogin {
    pub login: TelnetLogin,
    pub on_success: Option<Box<dyn FnOnce() + Send>>,
}

/// Login state for one Telnet connection.
pub struct TelnetLogin {
    username: String,
    password: Option<Zeroizing<String>>,
    phase: Phase,
    user_sent: bool,
    tail: String,
    /// In escape sequence: skip until its final byte.
    in_escape: bool,
    started: Instant,
}

impl TelnetLogin {
    pub fn new(username: &str, password: Option<Zeroizing<String>>) -> Self {
        Self {
            username: username.trim().to_owned(),
            password,
            phase: Phase::Prompts,
            user_sent: false,
            tail: String::new(),
            in_escape: false,
            started: Instant::now(),
        }
    }

    /// Whether the watcher still acts.
    pub fn active(&self) -> bool {
        self.phase != Phase::Done
    }

    /// Feeds terminal output (after Telnet decoding).
    pub fn feed(&mut self, data: &[u8]) -> LoginStep {
        if self.phase == Phase::Done {
            return LoginStep::Wait;
        }
        if self.started.elapsed() > LOGIN_WINDOW {
            self.phase = Phase::Done;
            return LoginStep::Wait;
        }
        self.push(data);
        let tail = self.tail.to_ascii_lowercase();
        let line = last_line(&tail);
        match self.phase {
            Phase::Prompts => {
                if is_password_prompt(line) {
                    self.tail.clear();
                    return match &self.password {
                        Some(p) => {
                            self.phase = Phase::Checking;
                            LoginStep::Send(with_enter(p.as_bytes()))
                        }
                        // No password: the user types it; nothing to check.
                        None => {
                            self.phase = Phase::Done;
                            LoginStep::Wait
                        }
                    };
                }
                if is_login_prompt(line) {
                    self.tail.clear();
                    if self.user_sent {
                        // Asked again before any password: refused.
                        self.phase = Phase::Done;
                        return LoginStep::Failed;
                    }
                    self.user_sent = true;
                    return LoginStep::Send(with_enter(self.username.as_bytes()));
                }
                LoginStep::Wait
            }
            Phase::Checking => {
                if is_failure(&tail) || is_login_prompt(line) || is_password_prompt(line) {
                    self.phase = Phase::Done;
                    return LoginStep::Failed;
                }
                if is_command_prompt(line) {
                    self.phase = Phase::Done;
                    return LoginStep::Succeeded;
                }
                LoginStep::Wait
            }
            Phase::Done => LoginStep::Wait,
        }
    }

    /// Appends printable text, dropping escape sequences and other controls.
    fn push(&mut self, data: &[u8]) {
        for ch in String::from_utf8_lossy(data).chars() {
            if self.in_escape {
                // CSI and similar sequences end with a letter or `~`.
                if ch.is_ascii_alphabetic() || ch == '~' {
                    self.in_escape = false;
                }
                continue;
            }
            match ch {
                '\u{1b}' => self.in_escape = true,
                '\n' => self.tail.push('\n'),
                c if c.is_control() => {}
                c => self.tail.push(c),
            }
        }
        let extra = self.tail.chars().count().saturating_sub(TAIL_CHARS);
        if extra > 0 {
            self.tail = self.tail.chars().skip(extra).collect();
        }
    }
}

fn with_enter(text: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut v = Zeroizing::new(Vec::with_capacity(text.len() + 1));
    v.extend_from_slice(text);
    v.push(b'\r');
    v
}

/// The text after the last line break, without trailing spaces.
fn last_line(tail: &str) -> &str {
    tail.rsplit('\n').next().unwrap_or("").trim_end()
}

fn is_login_prompt(line: &str) -> bool {
    ["login:", "username:", "user name:", "user:"]
        .iter()
        .any(|p| line.ends_with(p))
}

fn is_password_prompt(line: &str) -> bool {
    line.ends_with("password:") || line.ends_with("passcode:")
}

/// A short line ending like a CLI prompt: `switch#`, `router>`, `user@host:~$`.
fn is_command_prompt(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && line.chars().count() <= 80
        && line.ends_with(['#', '>', '$', '%'])
        // "Last login: …" is a message; "user@host:~$" is a prompt.
        && (!line.contains(':') || line.contains('@'))
}

fn is_failure(tail: &str) -> bool {
    [
        "incorrect",
        "invalid",
        "failed",
        "failure",
        "denied",
        "bad password",
        "authentication error",
    ]
    .iter()
    .any(|w| tail.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> Option<Zeroizing<String>> {
        Some(Zeroizing::new(s.to_owned()))
    }

    fn sent(step: LoginStep) -> Vec<u8> {
        match step {
            LoginStep::Send(b) => b.to_vec(),
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn logs_in_to_a_cisco_style_switch() {
        let secret = format!("p{}", std::process::id());
        let mut l = TelnetLogin::new(" admin ", pw(&secret));
        assert_eq!(
            l.feed(b"\r\nUser Access Verification\r\n\r\n"),
            LoginStep::Wait
        );
        assert_eq!(sent(l.feed(b"Username: ")), b"admin\r");
        assert_eq!(l.feed(b"admin\r\n"), LoginStep::Wait);
        assert_eq!(
            sent(l.feed(b"Password: ")),
            format!("{secret}\r").into_bytes()
        );
        assert_eq!(l.feed(b"\r\n\r\ncore-sw1#"), LoginStep::Succeeded);
        assert!(!l.active());
        assert_eq!(l.feed(b"Username: "), LoginStep::Wait, "done means silent");
    }

    #[test]
    fn prompts_split_over_chunks_and_colors_are_ignored() {
        let mut l = TelnetLogin::new("root", pw("x"));
        assert_eq!(l.feed(b"\x1b[1mlog"), LoginStep::Wait);
        assert_eq!(sent(l.feed(b"in:\x1b[0m ")), b"root\r");
        assert_eq!(l.feed(b"Pass"), LoginStep::Wait);
        assert_eq!(sent(l.feed(b"word: ")), b"x\r");
    }

    #[test]
    fn a_refused_password_is_a_failure_and_nothing_is_sent_again() {
        let mut l = TelnetLogin::new("admin", pw("wrong"));
        sent(l.feed(b"login: "));
        sent(l.feed(b"Password: "));
        assert_eq!(
            l.feed(b"\r\n% Login invalid\r\n\r\nUsername: "),
            LoginStep::Failed
        );
        assert_eq!(l.feed(b"Username: "), LoginStep::Wait);

        let mut l = TelnetLogin::new("admin", pw("wrong"));
        sent(l.feed(b"login: "));
        sent(l.feed(b"Password: "));
        assert_eq!(l.feed(b"\r\nPassword: "), LoginStep::Failed, "asked again");
    }

    #[test]
    fn without_a_password_only_the_user_name_is_typed() {
        let mut l = TelnetLogin::new("admin", None);
        assert_eq!(sent(l.feed(b"login: ")), b"admin\r");
        assert_eq!(l.feed(b"Password: "), LoginStep::Wait);
        assert!(!l.active());
    }

    #[test]
    fn a_second_login_prompt_before_the_password_stops() {
        let mut l = TelnetLogin::new("admin", pw("x"));
        sent(l.feed(b"login: "));
        assert_eq!(l.feed(b"\r\nlogin: "), LoginStep::Failed);
    }

    #[test]
    fn devices_that_ask_only_for_a_password() {
        let mut l = TelnetLogin::new("admin", pw("x"));
        assert_eq!(sent(l.feed(b"\r\nPassword:")), b"x\r");
        assert_eq!(l.feed(b"\r\nrouter>"), LoginStep::Succeeded);
    }

    #[test]
    fn shell_prompts_count_as_success_but_messages_with_colons_do_not() {
        let mut l = TelnetLogin::new("pi", pw("x"));
        sent(l.feed(b"raspberrypi login: "));
        sent(l.feed(b"Password: "));
        assert_eq!(
            l.feed(b"\r\nLast login: Mon Nov 23 from 10.0.0.5\r\n"),
            LoginStep::Wait
        );
        assert_eq!(l.feed(b"pi@raspberrypi:~ $ "), LoginStep::Succeeded);
    }
}
