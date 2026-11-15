//! Command-line helpers besides serving: account recovery, backups and the
//! container health check.

use crate::auth::password;
use crate::auth::store::{AuthDb, Role, UserFields};
use crate::config::Config;
use anyhow::{Context, bail};
use rusqlite::{Connection, OpenFlags};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USAGE: &str = "usage: localapp-server [serve]
       localapp-server admin create <username>
       localapp-server admin reset-password <username>
       localapp-server admin list
       localapp-server backup <dir> [--keep N]
       localapp-server backup-check <dir>
       localapp-server healthcheck";

/// `localapp-server admin …`: manage accounts without the web UI
/// (e.g. `docker compose exec localapp localapp-server admin create alice`).
pub fn admin(cfg: &Config, args: &[&str]) -> anyhow::Result<()> {
    let auth = AuthDb::open(&cfg.auth_db_path())?;
    match args {
        ["create", username] => {
            let temp = password::generate_temporary();
            let fields = UserFields {
                username: username.to_string(),
                role: Some(Role::Admin),
                ..Default::default()
            };
            auth.create_user(&fields, &password::hash(&temp)?, true)?;
            println!("Created admin {username:?}. Temporary password: {temp}");
            println!("You'll be asked to choose a new password when you first sign in.");
        }
        ["reset-password", username] => {
            let (user, _) = auth
                .credentials(username)?
                .with_context(|| format!("no user called {username:?}"))?;
            let temp = password::generate_temporary();
            auth.set_password(user.id, &password::hash(&temp)?, true)?;
            println!("New temporary password for {}: {temp}", user.username);
        }
        ["list"] => {
            for u in auth.users()? {
                let status = if u.disabled { "disabled" } else { "active" };
                println!("{:<24} {:<6} {status}", u.username, u.role.key());
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// `localapp-server healthcheck`: exit 0 if the server answers `/healthz`.
/// Used by the container's HEALTHCHECK (the image has no curl or shell).
pub fn healthcheck(cfg: &Config) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.bind.port()));
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&addr, timeout).context("connecting")?;
    stream.set_read_timeout(Some(timeout))?;
    let request = format!(
        "GET {}/healthz HTTP/1.0\r\nHost: localhost\r\n\r\n",
        cfg.base_path
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.lines().next().unwrap_or_default();
    if status.contains(" 200 ") {
        Ok(())
    } else {
        bail!("unhealthy: {status}")
    }
}

/// The databases a backup holds: `(file in the data directory, backup prefix)`.
const DATABASES: [(&str, &str); 2] = [("auth.sqlite", "auth"), (localapp_core::db::DB_FILE, "app")];

/// `localapp-server backup <dir> [--keep N]`: write a consistent copy of both
/// databases to `<dir>` as `app-<time>.sqlite` and `auth-<time>.sqlite`, then
/// delete all but the newest N sets (default 14). Safe while the server runs.
pub fn backup(cfg: &Config, args: &[&str]) -> anyhow::Result<()> {
    let (dir, keep) = match args {
        [dir] => (*dir, 14),
        [dir, "--keep", n] => (
            *dir,
            n.parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .context("--keep needs a number above 0")?,
        ),
        _ => bail!("{USAGE}"),
    };
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    for (file, prefix) in DATABASES {
        let src = cfg.data_dir.join(file);
        anyhow::ensure!(src.exists(), "{} doesn't exist", src.display());
        let dest = dir.join(format!("{prefix}-{stamp}.sqlite"));
        anyhow::ensure!(!dest.exists(), "{} already exists", dest.display());
        // Read-only: never touches the live database, even to migrate it.
        let conn = Connection::open_with_flags(&src, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {}", src.display()))?;
        conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])
            .with_context(|| format!("writing {}", dest.display()))?;
        println!("{}", dest.display());
        for old in backups(dir, prefix)?.into_iter().rev().skip(keep) {
            std::fs::remove_file(&old).with_context(|| format!("removing {}", old.display()))?;
        }
    }
    Ok(())
}

/// Backup files for one database, oldest first (the names sort by time).
fn backups(dir: &Path, prefix: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{prefix}-")) && n.ends_with(".sqlite"))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// `localapp-server backup-check <dir>`: open the newest backup of each
/// database and run SQLite's integrity check, so you know they'd restore.
pub fn backup_check(args: &[&str]) -> anyhow::Result<()> {
    let [dir] = args else { bail!("{USAGE}") };
    for (_, prefix) in DATABASES {
        let newest = backups(Path::new(dir), prefix)?
            .pop()
            .with_context(|| format!("no {prefix}-*.sqlite backup in {dir}"))?;
        let conn = Connection::open_with_flags(&newest, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        anyhow::ensure!(
            result == "ok",
            "{}: integrity check failed: {result}",
            newest.display()
        );
        println!("{}: ok (schema version {version})", newest.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_up_keeps_the_newest_and_checks() {
        let data = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut cfg = Config::defaults();
        cfg.data_dir = data.path().to_path_buf();
        crate::AppState::new(cfg.clone()).unwrap();
        let dir = out.path().to_str().unwrap();
        // Older sets, as earlier runs would have left them.
        for t in ["20200101-000000", "20200102-000000"] {
            for (_, prefix) in DATABASES {
                std::fs::write(out.path().join(format!("{prefix}-{t}.sqlite")), b"old").unwrap();
            }
        }
        backup(&cfg, &[dir, "--keep", "2"]).unwrap();
        for (_, prefix) in DATABASES {
            let files = backups(out.path(), prefix).unwrap();
            assert_eq!(files.len(), 2, "{files:?}");
            assert!(!files[0].ends_with(format!("{prefix}-20200101-000000.sqlite")));
        }
        backup_check(&[dir]).unwrap();
        assert!(backup(&cfg, &[dir, "--keep", "0"]).is_err());
    }
}
