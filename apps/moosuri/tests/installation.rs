use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const PASSWORD: &str = "Synthetic-Installation-Password-42";
struct Service(Child);
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Service {
    fn stop(&mut self) {
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.0.try_wait().unwrap().is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "graceful shutdown timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
fn start(data: &Path, port: u16) -> Service {
    let child = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args([
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--origin",
            &format!("http://127.0.0.1:{port}"),
            "--dev",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let service = Service(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "server did not start (build web assets before Rust)"
        );
        thread::sleep(Duration::from_millis(20));
    }
    service
}
struct Reply {
    status: u16,
    headers: String,
    body: String,
}
fn request(port: u16, method: &str, path: &str, cookie: &str, body: Option<Value>) -> Reply {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let expected = if !cookie.is_empty() && path != "/api/v1/auth/session" {
        let session = request(port, "GET", "/api/v1/auth/session", cookie, None);
        if session.status == 200 {
            let json: Value = serde_json::from_str(&session.body).unwrap();
            format!(
                "X-Moosuri-Account: {}\r\n",
                json["user"]["id"].as_str().unwrap()
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let body = body.map(|v| v.to_string()).unwrap_or_default();
    write!(socket,"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nCookie: {cookie}\r\n{expected}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut raw = String::new();
    socket.read_to_string(&mut raw).unwrap();
    let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
    Reply {
        status: headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers: headers.to_owned(),
        body: body.to_owned(),
    }
}
fn cookie(reply: &Reply) -> String {
    reply
        .headers
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
#[test]
fn cli_init_embedded_app_http_session_and_restart_work_together() {
    let root = tempfile::Builder::new()
        .prefix("moosuri-install-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let data = root.path().join("data");
    let password_file = root.path().join("password");
    fs::write(&password_file, PASSWORD).unwrap();
    fs::set_permissions(&password_file, fs::Permissions::from_mode(0o600)).unwrap();
    let init = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(&data)
        .args([
            "init",
            "--name",
            "Synthetic Admin",
            "--email",
            "admin@example.test",
            "--password-file",
        ])
        .arg(&password_file)
        .output()
        .unwrap();
    fs::remove_file(&password_file).unwrap();
    assert!(init.status.success());
    assert!(!String::from_utf8_lossy(&init.stdout).contains(PASSWORD));
    assert!(!String::from_utf8_lossy(&init.stderr).contains(PASSWORD));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut service = start(&data, port);
    let html = request(port, "GET", "/app", "", None);
    assert_eq!(html.status, 200);
    assert!(html.body.contains("<script"));
    assert!(html.headers.contains("default-src 'self'"));
    assert!(!html.headers.contains("default-src 'none'"));
    assert_eq!(request(port, "GET", "/health/ready", "", None).status, 204);
    let login = request(
        port,
        "POST",
        "/api/v1/auth/login",
        "",
        Some(json!({"email":"admin@example.test","password":PASSWORD})),
    );
    assert_eq!(login.status, 200);
    let session = cookie(&login);
    assert!(login.headers.contains("HttpOnly"));
    let thread = request(
        port,
        "POST",
        "/api/v1/threads",
        &session,
        Some(json!({"title":"Synthetic durable conversation"})),
    );
    assert_eq!(thread.status, 201);
    let id = serde_json::from_str::<Value>(&thread.body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let busy = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(&data)
        .arg("doctor")
        .output()
        .unwrap();
    assert!(!busy.status.success());
    // Hyper has accepted this request and is awaiting the promised JSON body.
    // A disconnected/stalled client must not keep SIGTERM draining forever.
    let mut held = TcpStream::connect(("127.0.0.1", port)).unwrap();
    held.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    write!(held, "POST /api/v1/auth/login HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nExpect: 100-continue\r\n\r\n").unwrap();
    let mut accepted = [0u8; 25];
    held.read_exact(&mut accepted).unwrap();
    assert_eq!(&accepted, b"HTTP/1.1 100 Continue\r\n\r\n");
    held.write_all(b"{\"email\":").unwrap();
    service.stop();
    drop(held);
    drop(service);
    let mut service = start(&data, port);
    let restored = request(port, "GET", "/api/v1/threads", &session, None);
    assert_eq!(restored.status, 200);
    assert!(restored.body.contains(&id));
    assert_eq!(
        request(
            port,
            "DELETE",
            &format!("/api/v1/threads/{id}"),
            &session,
            None
        )
        .status,
        204
    );
    assert_eq!(
        request(
            port,
            "POST",
            "/api/v1/auth/logout",
            &session,
            Some(json!({}))
        )
        .status,
        204
    );
    assert_eq!(
        request(port, "GET", "/api/v1/auth/session", &session, None).status,
        401
    );
    service.stop();
    drop(service);
    let doctor = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(&data)
        .arg("doctor")
        .output()
        .unwrap();
    assert!(doctor.status.success());
}
