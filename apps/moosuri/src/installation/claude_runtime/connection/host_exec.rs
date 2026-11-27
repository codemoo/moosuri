//! The gateway retains read-only /etc account databases. Only the installer-
//! derived useradd argv runs in a bounded transient root unit, using the OS's
//! own account/subordinate-ID allocator and locks. No shell or credential input.
use crate::installation::kiri::host::{self, Exec, Input, Out, Result, System};
use std::time::Duration;

pub(super) struct HostSystem(
    pub System,
    pub Option<moosuri_connectors::model::CancellationToken>,
);
impl Exec for HostSystem {
    fn run(&self, program: &str, args: &[String], input: Input, limit: Duration) -> Result<Out> {
        if program != "useradd" || args == ["--help"] {
            return self
                .0
                .run_cancellable(program, args, input, limit, self.1.as_ref());
        }
        if !matches!(input, Input::None) || limit > Duration::from_secs(120) {
            return Err("claude_connection_identity_command_invalid".into());
        }
        let tool = host::find_tool("useradd")?;
        // `full` explicitly binds /etc read-only and defeats an equal-path
        // exception on systemd 255. `strict` makes / read-only and the more
        // specific /etc exception writable; the rest remains read-only.
        let mut command = host::args(&[
            "--quiet",
            "--wait",
            "--pipe",
            "--collect",
            "--service-type=exec",
            "--property=User=root",
            "--property=Group=root",
            "--property=UMask=0077",
            "--property=NoNewPrivileges=yes",
            "--property=ProtectSystem=strict",
            "--property=ReadWritePaths=/etc",
            "--property=ProtectHome=yes",
            "--property=MemoryMax=134217728",
            "--property=TasksMax=16",
        ]);
        command.push(format!(
            "--property=RuntimeMaxSec={}",
            limit.as_secs().max(1)
        ));
        command.push("--".into());
        command.push(
            tool.to_str()
                .ok_or("claude_connection_path_invalid")?
                .into(),
        );
        command.extend_from_slice(args);
        // Account creation is one short OS transaction. Await its bounded unit
        // rather than let cancellation release our lease before useradd exits.
        let out = self.0.run_cancellable(
            "systemd-run",
            &command,
            Input::None,
            limit + Duration::from_secs(5),
            None,
        )?;
        if out.code != 0 {
            // Only fixed error categories from the credential-free OS allocator.
            let stderr = String::from_utf8_lossy(&out.stderr);
            let category = [
                ("Read-only file system", "read_only"),
                ("Permission denied", "permission"),
                ("cannot lock", "account_lock"),
                ("already exists", "already_exists"),
                ("unrecognized option", "unsupported_option"),
                ("invalid option", "unsupported_option"),
                ("Failed to connect to bus", "service_bus"),
                ("Failed to start transient", "transient_start"),
                ("No space left", "storage_full"),
                ("Can't get unique", "identity_capacity"),
            ]
            .into_iter()
            .find_map(|(text, code)| stderr.contains(text).then_some(code))
            .unwrap_or("allocator_failed");
            eprintln!(
                "moosuri: claude_identity_create_{category} exit={}",
                out.code
            );
        }
        Ok(out)
    }
}
