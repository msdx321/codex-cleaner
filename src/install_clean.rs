use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;
use walkdir::WalkDir;

use crate::summary::Summary;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    layout_version: u64,
    version: String,
    target: String,
}

pub fn clean_installations(home: &Path, apply: bool, summary: &mut Summary) {
    if let Err(err) = clean_daemon_releases(home, apply, summary) {
        summary.warn(format!("app-server installation cleanup failed: {err:#}"));
    }
}

fn clean_daemon_releases(home: &Path, apply: bool, summary: &mut Summary) -> anyhow::Result<()> {
    let root = home.join("packages/app-server-daemon");
    if !root.try_exists()? {
        return Ok(());
    }
    for path in [
        home.join("packages"),
        root.clone(),
        root.join("releases"),
        root.join("install.lock"),
    ] {
        anyhow::ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "refusing symlink {}",
            path.display()
        );
    }
    let lock = File::open(root.join("install.lock"))?;
    lock.try_lock()
        .context("Codex package installer is busy; retry when it is idle")?;
    let releases = root.join("releases");
    let current = root
        .join("current")
        .canonicalize()
        .context("resolve current daemon package")?;
    anyhow::ensure!(
        current.parent() == Some(releases.as_path()),
        "current package is outside the releases directory"
    );
    let current_package = read_package(&current)?;
    let current_version =
        version(&current_package.version).context("current package has an unsupported version")?;
    let mut protected = running_releases(&releases)?;
    protected.insert(current);
    let auto_update = root.join("auto-update-version");
    if auto_update.try_exists()? {
        anyhow::ensure!(
            !fs::symlink_metadata(&auto_update)?.file_type().is_symlink(),
            "refusing symlinked auto-update version"
        );
        let name = fs::read_to_string(auto_update)?;
        protected.insert(releases.join(name.trim()));
    }
    for entry in fs::read_dir(&releases)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let bucket = summary.bucket_mut("app-server-installations");
        if protected.contains(&path) {
            bucket.skipped += 1;
            continue;
        }
        let package = read_package(&path)?;
        if package.target != current_package.target
            || entry.file_name() != format!("{}-{}", package.version, package.target).as_str()
            || !version(&package.version).is_some_and(|version| version < current_version)
        {
            bucket.skipped += 1;
            continue;
        }
        let mut files = 0;
        let mut bytes = 0;
        let mut dirs = 0;
        for item in WalkDir::new(&path)
            .follow_links(false)
            .follow_root_links(false)
        {
            let item = item?;
            if item.file_type().is_dir() {
                dirs += 1;
            } else if item.file_type().is_file() || item.file_type().is_symlink() {
                files += 1;
                if item.file_type().is_file() {
                    bytes += item.metadata()?.len();
                }
            } else {
                anyhow::bail!(
                    "unexpected special file in package {}",
                    item.path().display()
                );
            }
        }
        bucket.matched_files += files;
        bucket.matched_bytes += bytes;
        if apply {
            fs::remove_dir_all(&path)
                .with_context(|| format!("remove obsolete package {}", path.display()))?;
            bucket.deleted_files += files;
            bucket.deleted_bytes += bytes;
            bucket.deleted_dirs += dirs;
        }
    }
    Ok(())
}

fn read_package(path: &Path) -> anyhow::Result<Package> {
    let manifest = path.join("codex-package.json");
    anyhow::ensure!(
        !fs::symlink_metadata(&manifest)?.file_type().is_symlink(),
        "refusing symlinked package manifest"
    );
    let package: Package = serde_json::from_slice(&fs::read(manifest)?)?;
    anyhow::ensure!(
        package.layout_version == 1,
        "unsupported package layout in {}",
        path.display()
    );
    Ok(package)
}

fn version(raw: &str) -> Option<[u64; 3]> {
    let mut parts = raw.split('.');
    let result = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(result)
}

fn running_releases(releases: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let mut result = BTreeSet::new();
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .output()
            .context("inspect running executable paths")?;
        anyhow::ensure!(
            output.status.success(),
            "could not inspect running executable paths"
        );
        for executable in std::str::from_utf8(&output.stdout)?.lines() {
            if let Ok(relative) = Path::new(executable.trim()).strip_prefix(releases)
                && let Some(name) = relative.components().next()
            {
                result.insert(releases.join(name));
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let owner = fs::metadata(releases)?.uid();
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue;
            }
            let path = entry.path();
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            if metadata.uid() != owner {
                continue;
            }
            let executable = match fs::read_link(path.join("exe")) {
                Ok(executable) => executable,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err).context("inspect running executable path"),
            };
            if let Ok(relative) = executable.strip_prefix(releases)
                && let Some(name) = relative.components().next()
            {
                result.insert(releases.join(name));
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    anyhow::bail!("installation cleanup requires macOS or Linux process inspection");
    Ok(result)
}
