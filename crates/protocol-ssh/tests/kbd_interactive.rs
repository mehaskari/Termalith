//! Keyboard-interactive authentication against an in-process russh server
//! (OpenSSH needs PAM for this, which an unprivileged test sshd lacks).

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol_ssh::{
    Auth, ConnectOptions, FileKnownHosts, KbdAnswers, KbdInteractive, KbdRequest, KnownHostsStore,
    RejectUnknown, SshError, SshSession, Target,
};
use russh::server::{self, Auth as ServerAuth, Response};
use russh::{MethodKind, MethodSet};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Only keyboard-interactive: "Password:" then "Verification code:".
    KbdOnly,
    /// Password first (partial success), then a code via keyboard-interactive.
    PasswordThenCode,
}

struct Server {
    mode: Mode,
    stage: usize,
    password_ok: bool,
}

fn code_prompt() -> ServerAuth {
    ServerAuth::Partial {
        name: Cow::Borrowed("2FA"),
        instructions: Cow::Borrowed("Enter the code from your app"),
        prompts: Cow::Owned(vec![(Cow::Borrowed("Verification code: "), true)]),
    }
}

impl server::Handler for Server {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        _user: &str,
        password: &str,
    ) -> Result<ServerAuth, Self::Error> {
        self.password_ok = password == "pw";
        Ok(if self.mode == Mode::PasswordThenCode && self.password_ok {
            ServerAuth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: true,
            }
        } else {
            ServerAuth::reject()
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<ServerAuth, Self::Error> {
        let answers: Vec<String> = response
            .map(|r| {
                r.map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let (stage, mode) = (self.stage, self.mode);
        self.stage += 1;
        Ok(match (mode, stage) {
            (Mode::KbdOnly, 0) => ServerAuth::Partial {
                name: Cow::Borrowed(""),
                instructions: Cow::Borrowed(""),
                prompts: Cow::Owned(vec![(Cow::Borrowed("Password: "), false)]),
            },
            (Mode::KbdOnly, 1) if answers == ["pw"] => code_prompt(),
            (Mode::PasswordThenCode, 0) if self.password_ok => code_prompt(),
            (Mode::KbdOnly, 2) | (Mode::PasswordThenCode, 1) if answers == ["123456"] => {
                ServerAuth::Accept
            }
            _ => ServerAuth::reject(),
        })
    }
}

struct TestServer {
    port: u16,
    key: russh::keys::PublicKey,
}

async fn start(mode: Mode) -> TestServer {
    let key =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let public = key.public_key().clone();
    let methods: &[MethodKind] = match mode {
        Mode::KbdOnly => &[MethodKind::KeyboardInteractive],
        Mode::PasswordThenCode => &[MethodKind::Password, MethodKind::KeyboardInteractive],
    };
    let config = Arc::new(server::Config {
        keys: vec![key],
        methods: MethodSet::from(methods),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let config = Arc::clone(&config);
            tokio::spawn(async move {
                if let Ok(session) = server::run_stream(
                    config,
                    tcp,
                    Server {
                        mode,
                        stage: 0,
                        password_ok: false,
                    },
                )
                .await
                {
                    let _ = session.await;
                }
            });
        }
    });
    TestServer { port, key: public }
}

/// Answers every request with fixed answers and records what it was asked.
struct Answers {
    answers: Option<Vec<&'static str>>,
    asked: Mutex<Vec<KbdRequest>>,
}

impl KbdInteractive for Answers {
    fn answer<'a>(
        &'a self,
        request: &'a KbdRequest,
    ) -> Pin<Box<dyn Future<Output = KbdAnswers> + Send + 'a>> {
        self.asked.lock().unwrap().push(request.clone());
        let a = self
            .answers
            .as_ref()
            .map(|v| v.iter().map(|s| Zeroizing::new((*s).to_owned())).collect());
        Box::pin(async move { a })
    }
}

async fn connect(
    s: &TestServer,
    password: &str,
    answers: Option<Vec<&'static str>>,
) -> (Result<SshSession, SshError>, Vec<KbdRequest>) {
    let dir = tempfile::tempdir().unwrap();
    let kh = FileKnownHosts::new(dir.path().join("known_hosts"));
    kh.add("127.0.0.1", s.port, &s.key).unwrap();
    let handler = Arc::new(Answers {
        answers,
        asked: Mutex::new(Vec::new()),
    });
    let mut opts = ConnectOptions::new(
        Arc::new(kh) as Arc<dyn KnownHostsStore>,
        Arc::new(RejectUnknown),
    );
    opts.timeout = Duration::from_secs(10);
    opts.kbd = Some(handler.clone());
    let target = Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: "me".into(),
    };
    let r = SshSession::connect(
        &target,
        &Auth::Password(Zeroizing::new(password.into())),
        &opts,
    )
    .await;
    let asked = handler.asked.lock().unwrap().clone();
    (r, asked)
}

#[tokio::test]
async fn kbd_only_server_gets_the_password_and_asks_the_user_for_the_code() {
    let s = start(Mode::KbdOnly).await;
    let (r, asked) = connect(&s, "pw", Some(vec!["123456"])).await;
    let session = r.expect("authenticated");
    // The password prompt was answered automatically; only the code was asked.
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].prompts.len(), 1);
    assert_eq!(asked[0].prompts[0].text, "Verification code: ");
    assert!(asked[0].prompts[0].echo);
    assert_eq!(asked[0].instructions, "Enter the code from your app");
    assert_eq!(asked[0].host, "127.0.0.1");
    session.disconnect().await.unwrap();
}

#[tokio::test]
async fn password_then_one_time_code() {
    let s = start(Mode::PasswordThenCode).await;
    let (r, asked) = connect(&s, "pw", Some(vec!["123456"])).await;
    r.expect("authenticated").disconnect().await.unwrap();
    assert_eq!(asked.len(), 1);

    let (wrong_code, _) = connect(&s, "pw", Some(vec!["000000"])).await;
    assert!(matches!(wrong_code, Err(SshError::AuthFailed)));
}

#[tokio::test]
async fn wrong_password_is_not_retried_through_keyboard_interactive() {
    let s = start(Mode::PasswordThenCode).await;
    let (r, asked) = connect(&s, "nope", Some(vec!["123456"])).await;
    assert!(matches!(r, Err(SshError::AuthFailed)));
    assert!(
        asked.is_empty(),
        "the user is not asked for a code after a wrong password"
    );
}

#[tokio::test]
async fn cancelling_the_prompt_cancels_the_login() {
    let s = start(Mode::KbdOnly).await;
    let (r, asked) = connect(&s, "pw", None).await;
    assert!(matches!(r, Err(SshError::Cancelled)), "{:?}", r.err());
    assert_eq!(asked.len(), 1);
}
