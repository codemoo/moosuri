//! Kiri image loading plus the isolated browser and Google-authentication capsules.
//! This prepares the owner/compose-labelled network and proxy that the existing
//! workers independently re-inspect before every launch; it does not bypass them.
use super::host::{self, Exec, Input, Result, args};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

pub const SOCKET: &str = "unix:///run/kiri/docker.sock";
const OWNER: &str = "io.moosuri.browser.owner";
const PROJECT: &str = "com.docker.compose.project";
const QUICK: Duration = Duration::from_secs(30);
const LOAD: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct NetPlan {
    pub subnet: String,
    pub proxy_ip: String,
    pub project: String,
}

pub struct Docker<'a> {
    pub exec: &'a dyn Exec,
}
impl Docker<'_> {
    fn run(&self, rest: &[&str], input: Input, limit: Duration) -> Result<host::Out> {
        let mut all = args(&["--host", SOCKET]);
        all.extend(rest.iter().map(|s| (*s).to_owned()));
        self.exec.run("docker", &all, input, limit)
    }
    fn json(&self, rest: &[&str]) -> Result<Option<Value>> {
        let out = self.run(rest, Input::None, QUICK)?;
        if out.code != 0 {
            return Ok(None);
        }
        serde_json::from_slice(&out.stdout)
            .map(Some)
            .map_err(|_| "kiri_install_engine_reply_invalid".into())
    }
    /// Only the actual Kiri 0.2.2 engine is accepted; Docker Engine or another Kiri is refused.
    pub fn require_kiri(&self) -> Result<()> {
        let server = self
            .json(&["version", "--format", "{{json .Server}}"])?
            .ok_or("kiri_install_engine_unavailable")?;
        let parts = server["Components"].as_array();
        let ok = server["Platform"]["Name"] == "Kiri"
            && parts.is_some_and(|p| {
                p.iter().filter(|c| c["Name"] == "Kiri").count() == 1
                    && p.iter()
                        .any(|c| c["Name"] == "Kiri" && c["Version"] == "0.2.2")
            });
        if ok {
            Ok(())
        } else {
            Err("kiri_install_engine_not_kiri_0_2_2".into())
        }
    }
    fn image(&self, id: &str) -> Result<Option<String>> {
        let out = self.run(
            &["image", "inspect", "--format", "{{.Id}}", id],
            Input::None,
            QUICK,
        )?;
        Ok((out.code == 0).then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    }
    /// Load a hash-validated archive and require the manifest's exact config ID.
    pub fn ensure_image(&self, archive: &Path, id: &str) -> Result<()> {
        if self.image(id)?.as_deref() == Some(id) {
            return Ok(());
        }
        self.run(&["load"], Input::File(archive.into()), LOAD)?
            .ok("kiri_install_image_load_failed")?;
        if self.image(id)?.as_deref() != Some(id) {
            return Err("kiri_install_image_id_mismatch".into());
        }
        Ok(())
    }
    pub fn used_subnets(&self) -> Result<Vec<(u32, u8)>> {
        let mut used = Vec::new();
        let ids = String::from_utf8_lossy(
            &self
                .run(
                    &["network", "ls", "--quiet", "--no-trunc"],
                    Input::None,
                    QUICK,
                )?
                .ok("kiri_install_network_inventory_failed")?,
        )
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        if !ids.is_empty() {
            let mut a = vec![
                "network",
                "inspect",
                "--format",
                "{{range .IPAM.Config}}{{.Subnet}} {{end}}",
            ];
            a.extend(ids.iter().map(String::as_str));
            let out = self
                .run(&a, Input::None, QUICK)?
                .ok("kiri_install_network_inventory_failed")?;
            used.extend(
                String::from_utf8_lossy(&out)
                    .split_whitespace()
                    .filter_map(cidr),
            );
        }
        Ok(used)
    }
    fn network(
        &self,
        name: &str,
        internal: bool,
        subnet: Option<&str>,
        owner: &str,
        project: &str,
        service: &str,
    ) -> Result<String> {
        let check = |v: &Value| -> Result<String> {
            let id = v["Id"].as_str().unwrap_or("");
            let labels = &v["Labels"];
            let cidr_ok = subnet.is_none_or(|s| v["IPAM"]["Config"][0]["Subnet"] == s);
            if !host::is_hex(id, 64)
                || labels[OWNER] != owner
                || labels[PROJECT] != project
                || v["Internal"] != internal
                || !cidr_ok
            {
                return Err("kiri_install_network_conflict".into());
            }
            Ok(id.to_owned())
        };
        let inspect = ["network", "inspect", "--format", "{{json .}}", name];
        if let Some(v) = self.json(&inspect)? {
            return check(&v);
        }
        let owner_label = format!("{OWNER}={owner}");
        let project_label = format!("{PROJECT}={project}");
        let service_label = format!("com.docker.compose.network={service}");
        let mut create = vec!["network", "create", "--driver", "bridge"];
        if internal {
            create.push("--internal");
        }
        if let Some(s) = subnet {
            create.extend(["--subnet", s]);
        }
        create.extend([
            "--label",
            owner_label.as_str(),
            "--label",
            project_label.as_str(),
            "--label",
            service_label.as_str(),
            name,
        ]);
        self.run(&create, Input::None, QUICK)?
            .ok("kiri_install_network_create_failed")?;
        check(
            &self
                .json(&inspect)?
                .ok_or("kiri_install_network_create_failed")?,
        )
    }
    /// Returns (network ID, proxy container ID). Existing resources must match exactly.
    pub fn capsule(&self, image: &str, plan: &NetPlan, owner: &str) -> Result<(String, String)> {
        let internal = format!("{}_browser", plan.project);
        let public = format!("{}_public", plan.project);
        let network = self.network(
            &internal,
            true,
            Some(&plan.subnet),
            owner,
            &plan.project,
            "browser",
        )?;
        let public_network = self.network(&public, false, None, owner, &plan.project, "public")?;
        let name = format!("{}-proxy-1", plan.project);
        let inspect = [
            "container",
            "inspect",
            "--format",
            "{{json .}}",
            name.as_str(),
        ];
        if self.json(&inspect)?.is_none() {
            let ip = format!("--ip={}", plan.proxy_ip);
            let owner_label = format!("{OWNER}={owner}");
            let project_label = format!("{PROJECT}={}", plan.project);
            self.run(
                &[
                    "container",
                    "create",
                    "--name",
                    &name,
                    "--entrypoint",
                    "/usr/bin/tini",
                    "--user",
                    "10001:10001",
                    "--read-only",
                    "--cap-drop",
                    "ALL",
                    "--security-opt",
                    "no-new-privileges:true",
                    "--network",
                    &internal,
                    &ip,
                    "--pids-limit",
                    "64",
                    "--memory",
                    "128m",
                    "--cpus",
                    "1",
                    "--restart",
                    "unless-stopped",
                    "--label",
                    &owner_label,
                    "--label",
                    &project_label,
                    "--label",
                    "com.docker.compose.service=proxy",
                    image,
                    "--",
                    "/usr/local/bin/browser-proxy",
                ],
                Input::None,
                QUICK,
            )?
            .ok("kiri_install_proxy_create_failed")?;
        }
        let mut v = self
            .json(&inspect)?
            .ok_or("kiri_install_proxy_unavailable")?;
        let nets = v["NetworkSettings"]["Networks"].clone();
        let id = v["Id"].as_str().unwrap_or("").to_owned();
        let running = v["State"]["Running"] == true;
        // Kiri fills the endpoint network ID and IP only on start. A stopped,
        // never-started proxy may report an empty ID; any non-empty ID must be exact.
        let id_ok = |endpoint: &Value, want: &str| match endpoint["NetworkID"].as_str() {
            Some(actual) => actual == want || (!running && actual.is_empty()),
            None => !running && endpoint["NetworkID"].is_null(),
        };
        if !host::is_hex(&id, 64)
            || v["Image"] != image
            || v["Config"]["Labels"][OWNER] != owner
            || v["Config"]["Labels"][PROJECT] != plan.project
            || nets[&internal].is_null()
            || !id_ok(&nets[&internal], &network)
            || (!nets[&public].is_null() && !id_ok(&nets[&public], &public_network))
            // A stopped newly-created proxy must already request the exact address.
            || if running {
                nets[&internal]["IPAddress"] != plan.proxy_ip
            } else {
                nets[&internal]["IPAMConfig"]["IPv4Address"] != plan.proxy_ip
                    || !matches!(nets[&internal]["IPAddress"].as_str(), Some("") | None)
                        && nets[&internal]["IPAddress"] != plan.proxy_ip
            }
        {
            return Err("kiri_install_proxy_conflict".into());
        }
        if nets[&public].is_null() {
            self.run(&["network", "connect", &public, &name], Input::None, QUICK)?
                .ok("kiri_install_proxy_network_failed")?;
        }
        if v["State"]["Running"] != true {
            self.run(&["container", "start", &name], Input::None, QUICK)?
                .ok("kiri_install_proxy_start_failed")?;
        }
        v = self
            .json(&inspect)?
            .ok_or("kiri_install_proxy_unavailable")?;
        let n = &v["NetworkSettings"]["Networks"];
        if v["State"]["Running"] != true
            || n[&internal]["IPAddress"] != plan.proxy_ip
            || n[&internal]["NetworkID"] != network
            || n[&public]["NetworkID"] != public_network
            || v["Id"] != id
        {
            return Err("kiri_install_proxy_not_ready".into());
        }
        Ok((network, id))
    }
}

pub fn cidr(text: &str) -> Option<(u32, u8)> {
    let (ip, len) = text.split_once('/').unwrap_or((text, "32"));
    let ip: std::net::Ipv4Addr = ip.parse().ok()?;
    let len: u8 = len.parse().ok().filter(|l| *l <= 32)?;
    Some((u32::from(ip), len))
}
pub fn overlaps(a: (u32, u8), b: (u32, u8)) -> bool {
    let len = a.1.min(b.1);
    let mask = if len == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(len))
    };
    a.0 & mask == b.0 & mask
}
/// Parse `ip -4 route/addr` text, retaining only IPv4 networks and addresses.
pub fn host_networks(text: &str) -> Vec<(u32, u8)> {
    text.split_whitespace()
        .filter(|t| t.bytes().next().is_some_and(|b| b.is_ascii_digit()))
        .filter_map(cidr)
        .collect()
}
/// A fixed /24 per capsule, `10.N.17.0/24`, avoiding every host, route and engine network.
pub fn plan(used: &[(u32, u8)], taken: &[NetPlan], project: String) -> Result<NetPlan> {
    for second in 100u32..=250 {
        let base = (10u32 << 24) | (second << 16) | (17 << 8);
        let candidate = (base, 24u8);
        if used.iter().any(|u| overlaps(*u, candidate))
            || taken
                .iter()
                .any(|t| cidr(&t.subnet).is_some_and(|c| overlaps(c, candidate)))
        {
            continue;
        }
        let ip = |n: u32| std::net::Ipv4Addr::from(base + n).to_string();
        return Ok(NetPlan {
            subnet: format!("{}/24", ip(0)),
            proxy_ip: ip(2),
            project,
        });
    }
    Err("kiri_install_no_free_private_subnet".into())
}

pub struct Capsule<'a> {
    pub image_id: &'a str,
    pub network_id: &'a str,
    pub proxy_container_id: &'a str,
    pub plan: &'a NetPlan,
    pub owner: &'a str,
}
/// Unverified input for the actual installation probe. Never publish this config.
pub fn capsule_json(c: &Capsule) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "engine": "kiri",
        "image_id": c.image_id,
        "network_id": c.network_id,
        "proxy_container_id": c.proxy_container_id,
        "proxy_ip": c.plan.proxy_ip,
        "owner": c.owner,
        "project": c.plan.project,
        "seccomp_file": "/etc/moosuri/seccomp.json",
        "verified": false,
        "smoke_evidence_sha256": "",
        "javascript_enabled": true,
    }))
    .unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installation::kiri::host::fake::Fake;

    fn endpoint(id: &str, ip: &str, ipam: bool) -> Value {
        let mut e = json!({"NetworkID": id, "IPAddress": ip});
        if ipam {
            e["IPAMConfig"] = json!({"IPv4Address": "10.102.17.2"});
        }
        e
    }
    fn proxy(running: bool, internal: Value, public: Option<Value>, image: &str) -> String {
        let mut nets = json!({"p_browser": internal});
        if let Some(p) = public {
            nets["p_public"] = p;
        }
        json!({"Id": "c".repeat(64), "Image": image, "State": {"Running": running},
            "Config": {"Labels": {(OWNER): "own", (PROJECT): "p"}},
            "NetworkSettings": {"Networks": nets}})
        .to_string()
    }
    fn net(id: &str, internal: bool, subnet: bool) -> String {
        let mut v = json!({"Id": id, "Internal": internal,
            "Labels": {(OWNER): "own", (PROJECT): "p"}});
        if subnet {
            v["IPAM"] = json!({"Config": [{"Subnet": "10.102.17.0/24"}]});
        }
        v.to_string()
    }
    fn plan_p() -> NetPlan {
        NetPlan {
            subnet: "10.102.17.0/24".into(),
            proxy_ip: "10.102.17.2".into(),
            project: "p".into(),
        }
    }

    #[test]
    fn proxy_accepts_empty_or_exact_prestart_ids_and_verifies_both_after_start() {
        let (n1, n2) = ("a".repeat(64), "b".repeat(64));
        let image = format!("sha256:{}", "1".repeat(64));
        let plan = plan_p();
        let run = |prestart: String, poststart: Option<String>| {
            let fake = Fake::default();
            fake.reply(0, &net(&n1, true, true));
            fake.reply(0, &net(&n2, false, false));
            fake.reply(0, &prestart);
            fake.reply(0, &prestart);
            if let Some(post) = poststart {
                fake.reply(0, ""); // network connect (public not attached yet)
                fake.reply(0, ""); // container start
                fake.reply(0, &post);
            }
            Docker { exec: &fake }.capsule(&image, &plan, "own")
        };
        let running = |pub_id: &str, ip: &str| {
            proxy(
                true,
                endpoint(&n1, ip, true),
                Some(endpoint(pub_id, "172.18.0.2", false)),
                &image,
            )
        };
        // Empty IDs before first start, then exact IDs after.
        let stopped =
            |internal_id: &str| proxy(false, endpoint(internal_id, "", true), None, &image);
        assert!(run(stopped(""), Some(running(&n2, "10.102.17.2"))).is_ok());
        assert!(run(stopped(&n1), Some(running(&n2, "10.102.17.2"))).is_ok());
        // Wrong non-empty pre-start ID, wrong post-start public ID or IP are refused.
        assert_eq!(
            run(stopped(&"d".repeat(64)), None).unwrap_err(),
            "kiri_install_proxy_conflict"
        );
        assert_eq!(
            run(stopped(""), Some(running(&"d".repeat(64), "10.102.17.2"))).unwrap_err(),
            "kiri_install_proxy_not_ready"
        );
        assert_eq!(
            run(stopped(""), Some(running(&n2, "10.102.17.9"))).unwrap_err(),
            "kiri_install_proxy_not_ready"
        );
        // A container that is already running may not have an empty ID.
        let live_empty = proxy(
            true,
            endpoint("", "10.102.17.2", true),
            Some(endpoint(&n2, "172.18.0.2", false)),
            &image,
        );
        assert_eq!(
            run(live_empty, None).unwrap_err(),
            "kiri_install_proxy_conflict"
        );
        // Stopped with the wrong requested address.
        let mut wrong_ip = endpoint("", "", true);
        wrong_ip["IPAMConfig"]["IPv4Address"] = json!("10.102.17.3");
        assert_eq!(
            run(proxy(false, wrong_ip, None, &image), None).unwrap_err(),
            "kiri_install_proxy_conflict"
        );
    }

    #[test]
    fn subnet_plan_skips_every_overlap_and_other_capsule() {
        let used = [
            cidr("10.100.17.0/24").unwrap(),
            cidr("10.101.0.0/16").unwrap(),
        ];
        let first = plan(&used, &[], "moosuri-browser-a".into()).unwrap();
        assert_eq!(first.subnet, "10.102.17.0/24");
        assert_eq!(first.proxy_ip, "10.102.17.2");
        let second = plan(&used, &[first], "moosuri-google-a".into()).unwrap();
        assert_eq!(second.subnet, "10.103.17.0/24");
        assert!(plan(&[cidr("10.0.0.0/8").unwrap()], &[], "p".into()).is_err());
        assert_eq!(
            host_networks("default via 10.1.2.3 dev eth0\n10.9.0.0/16 dev x"),
            vec![cidr("10.1.2.3").unwrap(), cidr("10.9.0.0/16").unwrap()]
        );
    }
    #[test]
    fn exact_kiri_version_and_image_id_are_required() {
        let fake = Fake::default();
        let docker = Docker { exec: &fake };
        fake.reply(
            0,
            r#"{"Platform":{"Name":"Docker Engine"},"Components":[]}"#,
        );
        assert_eq!(
            docker.require_kiri().unwrap_err(),
            "kiri_install_engine_not_kiri_0_2_2"
        );
        fake.reply(
            0,
            r#"{"Platform":{"Name":"Kiri"},"Components":[{"Name":"Kiri","Version":"0.2.1"}]}"#,
        );
        assert!(docker.require_kiri().is_err());
        fake.reply(
            0,
            r#"{"Platform":{"Name":"Kiri"},"Components":[{"Name":"Kiri","Version":"0.2.2"}]}"#,
        );
        docker.require_kiri().unwrap();
        let id = format!("sha256:{}", "a".repeat(64));
        // Missing locally, load succeeds, but the engine reports a different config ID.
        fake.reply(1, "");
        fake.reply(0, "");
        fake.reply(0, &format!("sha256:{}\n", "b".repeat(64)));
        assert_eq!(
            docker
                .ensure_image(Path::new("/nonexistent.tar"), &id)
                .unwrap_err(),
            "kiri_install_image_id_mismatch"
        );
    }
    #[test]
    fn capsule_config_matches_existing_worker_schema() {
        let plan = NetPlan {
            subnet: "10.102.17.0/24".into(),
            proxy_ip: "10.102.17.2".into(),
            project: "moosuri-browser-a".into(),
        };
        let bytes = capsule_json(&Capsule {
            image_id: &format!("sha256:{}", "a".repeat(64)),
            network_id: &"b".repeat(64),
            proxy_container_id: &"c".repeat(64),
            plan: &plan,
            owner: "11111111-1111-4111-8111-111111111111",
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["engine"], "kiri");
        assert_eq!(v["verified"], false);
        assert!(v.get("recover_workspaces").is_none());
        assert_eq!(v["smoke_evidence_sha256"], "");
    }
}
