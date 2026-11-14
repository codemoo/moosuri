//! Local author files only. Never opens an installation or invokes a process.
use clap::Subcommand;
use moosuri_store::skill_author::{self, FILE_LIMIT, FILE_NAMES, TOTAL_LIMIT};
use rand::{Rng, distributions::Alphanumeric};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, OsStr},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Create a synthetic four-file instruction template in a new directory.
    Init {
        slug: String,
        /// New destination; its parent must already exist. Defaults to ./<slug>.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Check local four-file content compatibility; does not install or execute.
    Check { directory: PathBuf },
    /// Build static catalog artifacts from an exact public GitHub commit.
    Catalog {
        #[arg(long)]
        repository: String,
        #[arg(long)]
        commit: String,
        /// Explicit package directory; repeat for each selected package (maximum16).
        #[arg(long = "package", required = true, num_args = 1)]
        packages: Vec<String>,
        /// New output directory with an existing parent; never overwritten.
        #[arg(long)]
        output: PathBuf,
    },
}
type Result<T> = std::result::Result<T, String>;
fn io_error(_: std::io::Error) -> String {
    "skills_filesystem_unavailable".into()
}
fn name(value: &OsStr) -> Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| "skills_invalid_path".into())
}
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::Prefix(_)))
    {
        return Err("skills_invalid_path: parent traversal is unsupported".into());
    }
    Ok(if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(io_error)?.join(path)
    })
}
fn open_at(parent: &File, name: &CStr, flags: i32, mode: libc::c_uint) -> Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode,
        )
    };
    if fd < 0 {
        return Err("skills_invalid_path_or_file: require existing directories and single-link regular files, without symlinks".into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn stat_at(parent: &File, name: &CStr) -> Result<Option<libc::stat>> {
    let mut stat = std::mem::MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        Ok(Some(unsafe { stat.assume_init() }))
    } else if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err("skills_filesystem_unavailable".into())
    }
}
fn identity(file: &File) -> Result<(libc::dev_t, libc::ino_t)> {
    let mut stat = std::mem::MaybeUninit::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err("skills_filesystem_unavailable".into());
    }
    let stat = unsafe { stat.assume_init() };
    Ok((stat.st_dev, stat.st_ino))
}
fn outside_installation(dir: &File) -> Result<()> {
    for marker in [
        "assistant.sqlite",
        "writer.lock",
        "auth-epochs.journal",
        "knowledge-invalidations.journal",
        "destructive-intents.journal",
        "provider-secrets.key",
    ] {
        if stat_at(dir, &name(OsStr::new(marker))?)?.is_some() {
            return Err("skills_application_data_path: choose an authoring directory outside installation data".into());
        }
    }
    Ok(())
}
// A pinned chain is retained until publication/cleanup. Only root and this
// effective UID may control names/modes; same-UID/root operator mutation is a
// trusted boundary, not a threat that name-based rename can exclude.
struct Directory {
    chain: Vec<File>,
}
impl std::ops::Deref for Directory {
    type Target = File;
    fn deref(&self) -> &File {
        self.chain.last().expect("directory chain includes root")
    }
}
impl Directory {
    fn recheck(&self) -> Result<()> {
        for dir in &self.chain {
            trusted_directory(dir)?;
            outside_installation(dir)?;
        }
        Ok(())
    }
}
fn trusted_mode(owner: libc::uid_t, mode: u32, current: libc::uid_t) -> bool {
    (owner == 0 || owner == current) && (mode & 0o022 == 0 || mode & 0o1000 != 0)
}
fn trusted_directory(dir: &File) -> Result<()> {
    let metadata = dir.metadata().map_err(io_error)?;
    if !metadata.is_dir()
        || !trusted_mode(metadata.uid(), metadata.mode(), unsafe { libc::geteuid() })
    {
        return Err("skills_untrusted_directory: require root/current-user owned ancestors without group/world write unless sticky".into());
    }
    supported_directory_acl(dir)
}

// macOS ACL grants can permit delete-child or permission changes independently
// of Unix mode bits. Support deny-only ACLs (including the normal home-directory
// delete denial); reject all grant ACLs instead of inferring principal trust.
#[cfg(target_os = "macos")]
fn supported_directory_acl(dir: &File) -> Result<()> {
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            entry: libc::c_int,
            result: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    struct Acl(*mut libc::c_void);
    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }
    let acl = unsafe { acl_get_fd_np(dir.as_raw_fd(), 0x100) }; // ACL_TYPE_EXTENDED
    if acl.is_null() {
        // Darwin reports ENOENT for a valid pinned directory with no extended
        // ACL, including ordinary root ancestry. Other inspection failures fail.
        if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOENT | libc::ENOTSUP)
        ) {
            return Ok(());
        }
        return Err("skills_directory_acl: ACL inspection unavailable".into());
    }
    let acl = Acl(acl);
    // Darwin returns -1/EINVAL at end-of-list. The descriptor yields a native
    // valid ACL; cap entries rather than consuming an unbounded list.
    for index in 0..=128 {
        let mut entry = std::ptr::null_mut();
        if unsafe { acl_get_entry(acl.0, if index == 0 { 0 } else { -1 }, &mut entry) } != 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                Ok(())
            } else {
                Err("skills_directory_acl: ACL inspection unavailable".into())
            };
        }
        let mut tag = 0;
        if index == 128 || unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != 2 {
            return Err("skills_directory_acl: grant or unsupported directory ACL; choose an ordinary trusted directory".into());
        }
    }
    Err("skills_directory_acl: unsupported ACL bounds".into())
}
// Reject Linux extended/default/NFSv4 ACLs rather than claiming that mode bits
// describe every filesystem ACL policy. No attribute contents/principals leak.
#[cfg(target_os = "linux")]
fn supported_directory_acl(dir: &File) -> Result<()> {
    for attribute in [
        c"system.posix_acl_access",
        c"system.posix_acl_default",
        c"system.nfs4_acl",
    ] {
        if unsafe { libc::fgetxattr(dir.as_raw_fd(), attribute.as_ptr(), std::ptr::null_mut(), 0) }
            >= 0
        {
            return Err("skills_directory_acl: extended directory ACL is unsupported; choose an ordinary trusted directory".into());
        }
        let error = std::io::Error::last_os_error().raw_os_error();
        if error != Some(libc::ENODATA) && error != Some(libc::ENOTSUP) {
            return Err("skills_directory_acl: ACL inspection unavailable".into());
        }
    }
    Ok(())
}
fn directory(path: &Path) -> Result<Directory> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(io_error)?;
    trusted_directory(&root)?;
    outside_installation(&root)?;
    let mut result = Directory { chain: vec![root] };
    for part in path.components() {
        match part {
            Component::Normal(value) => {
                let dir = open_at(
                    &result,
                    &name(value)?,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                trusted_directory(&dir)?;
                outside_installation(&dir)?;
                result.chain.push(dir);
            }
            Component::RootDir | Component::CurDir => (),
            _ => return Err("skills_invalid_path".into()),
        }
    }
    Ok(result)
}

// Inventory uses the pinned descriptor, so renaming a path cannot redirect reads.
fn inventory(dir: &File) -> Result<()> {
    let duplicate = dir.try_clone().map_err(io_error)?;
    let fd = std::os::fd::IntoRawFd::into_raw_fd(duplicate);
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err("skills_filesystem_unavailable".into());
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let stream = Directory(stream);
    let mut count = 0;
    loop {
        #[cfg(target_os = "linux")]
        let errno = unsafe { libc::__errno_location() };
        #[cfg(target_os = "macos")]
        let errno = unsafe { libc::__error() };
        unsafe {
            *errno = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *errno } != 0 {
                return Err("skills_filesystem_unavailable".into());
            }
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        count += 1;
        if count > FILE_NAMES.len() || !FILE_NAMES.iter().any(|f| f.as_bytes() == bytes) {
            return Err("skills_file_inventory: require exactly SKILL.md, moosuri.toml, examples.json and LICENSE".into());
        }
    }
    if count != FILE_NAMES.len() {
        return Err("skills_file_inventory: missing required package file".into());
    }
    Ok(())
}
fn read_package(dir: &File) -> Result<BTreeMap<String, String>> {
    inventory(dir)?;
    let mut files = BTreeMap::new();
    let mut total = 0;
    for file_name in FILE_NAMES {
        let file = open_at(dir, &name(OsStr::new(file_name))?, libc::O_RDONLY, 0)?;
        let stat = file.metadata().map_err(io_error)?;
        if !stat.is_file() || stat.nlink() != 1 || stat.mode() & 0o7111 != 0 {
            return Err("skills_file_mode: require regular nonexecutable files without links or special mode bits".into());
        }
        if stat.len() > FILE_LIMIT as u64 {
            return Err("skills_file_limit: maximum32KiB per file".into());
        }
        let mut bytes = Vec::new();
        file.take(FILE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        total += bytes.len();
        if bytes.len() > FILE_LIMIT || total > TOTAL_LIMIT {
            return Err("skills_file_limit: maximum32KiB per file and64KiB total".into());
        }
        files.insert(
            file_name.into(),
            String::from_utf8(bytes)
                .map_err(|_| "skills_file_encoding: require UTF-8".to_string())?,
        );
    }
    Ok(files)
}

struct Partial {
    parent: Directory,
    name: CString,
    dir: File,
    files: Vec<(CString, libc::dev_t, libc::ino_t)>,
    published: bool,
}
impl Drop for Partial {
    fn drop(&mut self) {
        if self.published || self.parent.recheck().is_err() || trusted_directory(&self.dir).is_err()
        {
            return;
        }
        let Ok(own) = identity(&self.dir) else {
            return;
        };
        let Ok(Some(current)) = stat_at(&self.parent, &self.name) else {
            return;
        };
        if (current.st_dev, current.st_ino) != own {
            return;
        }
        // Remove only files we created with the same inode, never unknown paths.
        for (file, dev, ino) in &self.files {
            if let Ok(Some(current)) = stat_at(&self.dir, file)
                && current.st_dev == *dev
                && current.st_ino == *ino
            {
                unsafe {
                    libc::unlinkat(self.dir.as_raw_fd(), file.as_ptr(), 0);
                }
            }
        }
        // An unexpected file makes rmdir fail; do not recursively delete it.
        unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            );
        }
    }
}
fn publish(partial: &Partial, destination: &CStr) -> Result<()> {
    partial.parent.recheck()?;
    trusted_directory(&partial.dir)?;
    let own = identity(&partial.dir)?;
    let current = stat_at(&partial.parent, &partial.name)?.ok_or("skills_staging_changed")?;
    if (current.st_dev, current.st_ino) != own {
        return Err("skills_staging_changed".into());
    }
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            partial.parent.as_raw_fd(),
            partial.name.as_ptr(),
            partial.parent.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            partial.parent.as_raw_fd(),
            partial.name.as_ptr(),
            partial.parent.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result != 0 {
        return Err(
            "skills_destination_exists_or_publish_failed: destination is never replaced".into(),
        );
    }
    Ok(())
}
fn write_new_directory(files: &BTreeMap<String, String>, output: &Path) -> Result<()> {
    let output = absolute(output)?;
    let destination = name(output.file_name().ok_or("skills_invalid_path")?)?;
    let parent = directory(output.parent().ok_or("skills_invalid_path")?)?;
    if stat_at(&parent, &destination)?.is_some() {
        return Err("skills_destination_exists: choose a new directory".into());
    }
    let random: String = rand::rngs::OsRng
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    let staged = name(OsStr::new(&format!(".moosuri-skill-{random}.partial")))?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), staged.as_ptr(), 0o700) } != 0 {
        return Err("skills_filesystem_unavailable".into());
    }
    let dir = open_at(&parent, &staged, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    let mut partial = Partial {
        parent,
        name: staged,
        dir,
        files: vec![],
        published: false,
    };
    for (file_name, text) in files {
        let file_name = name(OsStr::new(&file_name))?;
        let mut file = open_at(
            &partial.dir,
            &file_name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o644,
        )?;
        let (dev, ino) = identity(&file)?;
        partial.files.push((file_name, dev, ino));
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
    }
    partial.dir.sync_all().map_err(io_error)?;
    outside_installation(&partial.parent)?;
    publish(&partial, &destination)?;
    partial.published = true;
    partial.parent.sync_all().map_err(io_error)?;
    Ok(())
}
fn init(slug: &str, output: Option<&Path>) -> Result<()> {
    let files = skill_author::template(slug).map_err(|e| e.to_string())?;
    write_new_directory(&files, output.unwrap_or_else(|| Path::new(slug)))?;
    println!(
        "Created four synthetic authoring files. Edit and review them, then run moosuri skills check DIRECTORY. Nothing was installed or published remotely."
    );
    Ok(())
}
pub(crate) async fn run(command: &Command) -> Result<()> {
    run_with_catalog(command, |selection| async move {
        skill_author::catalog::generate(&selection)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}
// Private injection seam tests real dispatch on the existing Tokio runtime;
// production always supplies the fixed adapter-backed generator above.
async fn run_with_catalog<F, Fut>(command: &Command, generate: F) -> Result<()>
where
    F: FnOnce(skill_author::catalog::Selection) -> Fut,
    Fut: std::future::Future<Output = Result<skill_author::catalog::Artifacts>>,
{
    match command {
        Command::Init { slug, output } => init(slug, output.as_deref()),
        Command::Catalog {
            repository,
            commit,
            packages,
            output,
        } => {
            let selection =
                skill_author::catalog::Selection::new(repository, commit, packages.clone())
                    .map_err(|e| e.to_string())?;
            // Reject unusable/conflicting destinations before any public request.
            let path = absolute(output)?;
            let destination = name(path.file_name().ok_or("skills_invalid_path")?)?;
            let parent = directory(path.parent().ok_or("skills_invalid_path")?)?;
            if stat_at(&parent, &destination)?.is_some() {
                return Err("skills_destination_exists: choose a new directory".into());
            }
            let artifacts = generate(selection).await?;
            write_new_directory(artifacts.files(), output)?;
            println!(
                "Created {} immutable public package artifacts and catalog.json. No installation or remote publication occurred.",
                artifacts.package_count()
            );
            Ok(())
        }
        Command::Check { directory: path } => {
            let dir = directory(&absolute(path)?)?;
            let files = read_package(&dir)?;
            let slug = files["SKILL.md"]
                .lines()
                .find_map(|line| line.strip_prefix("name: "))
                .unwrap_or("")
                .to_owned();
            let checked = skill_author::check(&slug, files).map_err(|e| e.to_string())?;
            println!(
                "Compatible instruction package: {} {} ({} declared capabilities). Content compatibility only; not safety, license-rights or task-quality certification. Nothing was installed.",
                checked.name,
                checked.version,
                checked.capabilities.len()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ancestor_trust_requires_owner_even_for_sticky_directories() {
        let current = 1000;
        for owner in [0, current] {
            for mode in [0o700, 0o755, 0o1755, 0o1777] {
                assert!(trusted_mode(owner, mode, current));
            }
            for mode in [0o770, 0o775, 0o777] {
                assert!(!trusted_mode(owner, mode, current));
            }
        }
        for mode in [0o700, 0o755, 0o1777] {
            assert!(!trusted_mode(2000, mode, current));
        }
    }
    #[test]
    fn publication_rechecks_the_entire_pinned_chain_and_cleanup_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let ancestor = root.path().join("ancestor");
        let nested = ancestor.join("nested");
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::create_dir(&nested).unwrap();
        let parent = directory(&nested).unwrap();
        let stage = name(OsStr::new("owned-stage")).unwrap();
        assert_eq!(
            unsafe { libc::mkdirat(parent.as_raw_fd(), stage.as_ptr(), 0o700) },
            0
        );
        let dir = open_at(&parent, &stage, libc::O_RDONLY | libc::O_DIRECTORY, 0).unwrap();
        let partial = Partial {
            parent,
            name: stage,
            dir,
            files: vec![],
            published: false,
        };
        // Even though the immediate parent and stage remain private, a changed
        // earlier ancestor invalidates publication and conservative cleanup.
        std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            publish(&partial, &name(OsStr::new("release")).unwrap())
                .unwrap_err()
                .contains("skills_untrusted_directory")
        );
        drop(partial);
        assert!(!nested.join("release").exists());
        assert!(nested.join("owned-stage").is_dir());
        std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[tokio::test]
    async fn catalog_dispatch_awaits_on_existing_runtime_and_failed_acquisition_publishes_nothing()
    {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let output = root.path().join("public-release");
        let command = Command::Catalog {
            repository: "https://github.com/codemoo/moosuri".into(),
            commit: "7f91c0ad8ef38049c5e0e601658da9b99b5f824e".into(),
            packages: vec!["community/skills/source-briefing".into()],
            output: output.clone(),
        };
        let result = run_with_catalog(&command, |_| async {
            tokio::task::yield_now().await;
            Err("skills_catalog: synthetic unavailable public source".into())
        })
        .await;
        assert_eq!(
            result.unwrap_err(),
            "skills_catalog: synthetic unavailable public source"
        );
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn static_catalog_output_is_atomic_and_failure_preserves_existing_destinations() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let destination = root.path().join("catalog-release");
        let files = BTreeMap::from([
            ("catalog.json".into(), "{\"synthetic\":true}".into()),
            (
                "package-synthetic.json".into(),
                "synthetic public package".into(),
            ),
        ]);
        let mut fail = files.clone();
        fail.insert(
            "z\0invalid".into(),
            "synthetic failure after first writes".into(),
        );
        assert!(write_new_directory(&fail, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        write_new_directory(&files, &destination).unwrap();
        assert!(write_new_directory(&fail, &destination).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        for (name, text) in files {
            assert_eq!(
                std::fs::read_to_string(destination.join(name)).unwrap(),
                text
            );
        }
    }
    #[test]
    fn failed_staging_cleanup_removes_only_owned_inodes_and_preserves_unknown_paths() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        for replaced in [false, true] {
            let path = root
                .path()
                .join(if replaced { "replaced" } else { "partial" });
            std::fs::create_dir(&path).unwrap();
            let parent = directory(root.path()).unwrap();
            let stage_name = name(path.file_name().unwrap()).unwrap();
            let dir = open_at(&parent, &stage_name, libc::O_RDONLY | libc::O_DIRECTORY, 0).unwrap();
            let file_name = name(OsStr::new("SKILL.md")).unwrap();
            let mut file = open_at(
                &dir,
                &file_name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o644,
            )
            .unwrap();
            file.write_all(b"synthetic partial file").unwrap();
            let (dev, ino) = identity(&file).unwrap();
            drop(file);
            let partial = Partial {
                parent,
                name: stage_name,
                dir,
                files: vec![(file_name, dev, ino)],
                published: false,
            };
            if replaced {
                std::fs::rename(&path, root.path().join("moved")).unwrap();
                std::fs::create_dir(&path).unwrap();
            }
            std::fs::write(path.join("unknown"), b"synthetic retained unknown bytes").unwrap();
            drop(partial);
            assert_eq!(
                std::fs::read(path.join("unknown")).unwrap(),
                b"synthetic retained unknown bytes"
            );
            assert!(!path.join("SKILL.md").exists());
            if replaced {
                assert!(root.path().join("moved/SKILL.md").exists());
            }
        }
    }
}
