//! Dedicated nonroot system identity. Only the exact account tied to this
//! request's receipt and random GECOS marker is ever adopted; an unmanaged
//! account of the same name, a changed uid or a changed range fails closed.
use super::receipt::{IdRange, Ident, Receipt};
use super::{MAX_SERVICE_UID, SHELL};
use crate::installation::kiri::host::{Exec, Input, Layout, Result, args};
use std::{fs, time::Duration};

const LIMIT: Duration = Duration::from_secs(60);
const MIN_SUBID_START: u64 = 100_000;
const MIN_SUBID_COUNT: u64 = 65_536;

pub(super) struct Passwd {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: String,
    pub shell: String,
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T> {
    text.parse()
        .map_err(|_| "claude_runtime_account_record_invalid".into())
}

pub(super) fn lookup_user(exec: &dyn Exec, name: &str) -> Result<Option<Passwd>> {
    let out = exec.run("getent", &args(&["passwd", name]), Input::None, LIMIT)?;
    match out.code {
        0 => {}
        2 => return Ok(None),
        _ => return Err("claude_runtime_getent_failed".into()),
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim_end_matches('\n');
    let f: Vec<&str> = line.split(':').collect();
    if line.contains('\n') || f.len() != 7 {
        return Err("claude_runtime_account_record_invalid".into());
    }
    Ok(Some(Passwd {
        name: f[0].to_owned(),
        uid: number(f[2])?,
        gid: number(f[3])?,
        gecos: f[4].to_owned(),
        home: f[5].to_owned(),
        shell: f[6].to_owned(),
    }))
}

/// `(gid, member list)` of an existing group.
pub(super) fn lookup_group(exec: &dyn Exec, name: &str) -> Result<Option<(u32, String)>> {
    let out = exec.run("getent", &args(&["group", name]), Input::None, LIMIT)?;
    match out.code {
        0 => {}
        2 => return Ok(None),
        _ => return Err("claude_runtime_getent_failed".into()),
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let f: Vec<&str> = text.trim_end_matches('\n').split(':').collect();
    if f.len() != 4 || f[0] != name {
        return Err("claude_runtime_account_record_invalid".into());
    }
    Ok(Some((number(f[2])?, f[3].to_owned())))
}

/// System user + same-named group + home + automatic subordinate ranges.
pub(super) fn create(exec: &dyn Exec, receipt: &Receipt, system_subids: bool) -> Result<()> {
    let mut a = if system_subids {
        args(&["--system", "--add-subids-for-system"])
    } else {
        // useradd owns passwd/group/subid locking and allocation. Do not edit
        // those global files or invent a second subordinate-ID allocator.
        let mut occupied = std::collections::BTreeSet::new();
        for database in ["passwd", "group"] {
            let out = exec
                .run("getent", &args(&[database]), Input::None, LIMIT)?
                .ok("claude_runtime_getent_failed")?;
            for line in std::str::from_utf8(&out)
                .map_err(|_| "claude_runtime_account_record_invalid")?
                .lines()
            {
                let id = line
                    .split(':')
                    .nth(2)
                    .ok_or("claude_runtime_account_record_invalid")?;
                occupied.insert(number::<u32>(id)?);
            }
        }
        let selected = (10000..MAX_SERVICE_UID)
            .find(|id| !occupied.contains(id))
            .ok_or("claude_runtime_identity_capacity")?;
        vec!["--uid".into(), selected.to_string()]
    };
    a.extend(args(&[
        "--user-group",
        "--no-create-home",
        "--shell",
        SHELL,
        "--home-dir",
        receipt.home.as_str(),
        "--comment",
        receipt.marker.as_str(),
        receipt.username.as_str(),
    ]));
    exec.run("useradd", &a, Input::None, LIMIT)?
        .ok("claude_runtime_useradd_failed")
        .map(|_| ())
}

/// Verify the exact managed identity (and equality with a saved receipt).
pub(super) fn verify(layout: &Layout, exec: &dyn Exec, receipt: &Receipt) -> Result<Ident> {
    let pw = lookup_user(exec, &receipt.username)?.ok_or("claude_runtime_identity_missing")?;
    if pw.name != receipt.username
        || pw.gecos != receipt.marker
        || pw.home != receipt.home
        || pw.shell != SHELL
    {
        return Err("claude_runtime_foreign_account".into());
    }
    if pw.uid == 0 || pw.uid >= MAX_SERVICE_UID || pw.gid == 0 || pw.gid >= MAX_SERVICE_UID {
        return Err("claude_runtime_identity_out_of_range".into());
    }
    let (gid, members) =
        lookup_group(exec, &receipt.username)?.ok_or("claude_runtime_foreign_account")?;
    if gid != pw.gid || !members.is_empty() {
        return Err("claude_runtime_foreign_account".into());
    }
    let ident = Ident {
        uid: pw.uid,
        gid,
        subuid: subid(
            layout,
            "/etc/subuid",
            "/etc/passwd",
            &receipt.username,
            pw.uid,
        )?,
        subgid: subid(
            layout,
            "/etc/subgid",
            "/etc/group",
            &receipt.username,
            pw.uid,
        )?,
    };
    if receipt
        .identity
        .as_ref()
        .is_some_and(|saved| *saved != ident)
    {
        return Err("claude_runtime_identity_changed".into());
    }
    Ok(ident)
}

/// Exactly one suitable range for the account, no intersection with any other
/// entry, and no existing local uid/gid inside it.
fn subid(layout: &Layout, file: &str, ids: &str, name: &str, uid: u32) -> Result<IdRange> {
    let text =
        fs::read_to_string(layout.at(file)).map_err(|_| "claude_runtime_subid_unavailable")?;
    let (mut mine, mut others) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        let range = match f.as_slice() {
            [_, start, count] => IdRange {
                start: start
                    .parse()
                    .map_err(|_| "claude_runtime_subid_file_invalid")?,
                count: count
                    .parse()
                    .map_err(|_| "claude_runtime_subid_file_invalid")?,
            },
            _ => return Err("claude_runtime_subid_file_invalid".into()),
        };
        if range.count == 0 || range.start.checked_add(range.count).is_none() {
            return Err("claude_runtime_subid_file_invalid".into());
        }
        if f[0] == name || f[0] == uid.to_string() {
            mine.push(range);
        } else {
            others.push(range);
        }
    }
    let [mine] =
        <[IdRange; 1]>::try_from(mine).map_err(|_| "claude_runtime_subid_missing_or_ambiguous")?;
    if mine.count < MIN_SUBID_COUNT || mine.start < MIN_SUBID_START {
        return Err("claude_runtime_subid_range_unsuitable".into());
    }
    let end = mine.start + mine.count;
    if others
        .iter()
        .any(|o| mine.start < o.start + o.count && o.start < end)
    {
        return Err("claude_runtime_subid_overlap".into());
    }
    let local =
        fs::read_to_string(layout.at(ids)).map_err(|_| "claude_runtime_subid_unavailable")?;
    let id_in_range = local.lines().any(|l| {
        l.split(':')
            .nth(2)
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|id| id >= mine.start && id < end)
    });
    if id_in_range {
        return Err("claude_runtime_subid_overlaps_host_ids".into());
    }
    Ok(mine)
}
