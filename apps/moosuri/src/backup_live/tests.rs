use super::*;
#[test]
fn fixed_ssh_command_rejects_shell_and_option_injection() {
    for s in [
        "",
        "-oProxyCommand=x",
        "a;b",
        "a b",
        "a/b",
        "a@host",
        "a\nnext",
        "$(x)",
    ] {
        assert!(ssh_command(s, "target").is_err());
        assert!(ssh_command("host", s).is_err());
    }
    let c = ssh_command("backup-host", "target-1").unwrap();
    let args = c
        .get_args()
        .map(|v| v.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.ends_with(&[
        "--",
        "backup-host",
        "moosuri",
        "backup",
        "receive-journal",
        "--target-id",
        "target-1"
    ]));
}
