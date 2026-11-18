use super::*;
fn plan() -> Plan {
    Plan {
        v: 1,
        root: "/srv/moosuri-storage-test".into(),
        request: Uuid::new_v4(),
        account: Uuid::new_v4().to_string(),
        tenant: Uuid::new_v4().to_string(),
        volume: Uuid::new_v4().to_string(),
        generation: 1,
        uid: 10061,
        cap: 64 * 1024 * 1024,
    }
}
#[test]
fn generated_units_bind_only_owned_paths_and_have_reboot_dependencies() {
    let p = plan();
    let (a, b) = units(&p);
    assert!(a.contains("Options=loop,rw,nosuid,nodev,noexec"));
    assert!(a.contains(&format!("What={}", p.image().display())));
    assert!(b.contains(&format!("Requires={}", unit_name(&p.mount()))));
    assert!(b.contains(&format!("After={}", unit_name(&p.mount()))));
    assert!(b.contains("WantedBy=local-fs.target"));
    assert!(b.contains(&format!("What={}/ipc", p.mount().display())));
    assert_eq!(
        unit_name(Path::new("/srv/my-root/mounts/test-id")),
        "srv-my\\x2droot-mounts-test\\x2did.mount"
    );
    for path in ["relative/path", "/srv/bad name", "/srv/$HOME", "/srv/%n"] {
        assert!(plain(Path::new(path)).is_err());
    }
}
#[test]
fn mount_identity_and_security_flags_are_not_inferred_from_directory_name() {
    let p = Path::new("/srv/test/mounts/a");
    let line =
        "42 1 7:2 / /srv/test/mounts/a rw,nosuid,nodev,noexec,relatime - ext4 /dev/loop2 rw\n";
    let m = parse_mount(line, p).unwrap().unwrap();
    assert!(flags(&m));
    assert_eq!(m.device, "7:2");
    assert_eq!(m.root, "/");
    assert!(parse_mount(&format!("{line}{line}"), p).is_err());
    assert!(parse_mount("bad mountinfo", p).is_err());
    assert!(!flags(
        &parse_mount(&line.replace("nodev,", ""), p)
            .unwrap()
            .unwrap()
    ));
    assert!(
        parse_mount(line, Path::new("/srv/test/mounts/b"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn prepared_result_and_plan_replay_are_exact_and_non_linux_entry_is_inert() {
    let p = plan();
    let mut changed = p.clone();
    changed.cap *= 2;
    assert!(p != changed);
    changed = p.clone();
    changed.generation += 1;
    assert!(p != changed);
    let out = p.prepared();
    assert_eq!(out.ipc_root, p.root.join("ipc"));
    assert_eq!(out.member_data, p.mount().join("member-data"));
    assert_eq!(out.capacity_bytes, p.cap);
    assert_eq!(out.uid, 10061);
    let serialized = serde_json::to_string(&Record {
        plan: p,
        phase: Phase::Formatting,
        image_identity: Some((1, 2)),
    })
    .unwrap();
    let r: Record = serde_json::from_str(&serialized).unwrap();
    assert!(r.phase == Phase::Formatting);
    if !cfg!(target_os = "linux") {
        let a = Allocation {
            account_id: r.plan.account.clone(),
            tenant_id: r.plan.tenant.clone(),
            volume_id: r.plan.volume.clone(),
            generation: 1,
        };
        let o = Options {
            experimental: true,
            registry_directory: "/definitely/not/a/registry".into(),
            request_id: r.plan.request,
            email: "synthetic@example.test".into(),
            root: r.plan.root.clone(),
            uid: 10061,
            capacity_bytes: r.plan.cap,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(
            runtime
                .block_on(run(&o))
                .unwrap_err()
                .contains("linux_root_only")
        );
        assert!(
            prepare(&o, &a, 10061)
                .unwrap_err()
                .contains("linux_root_only")
        );
    }
}
