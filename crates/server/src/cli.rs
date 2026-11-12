//! Command-line helpers besides serving: account recovery and the container
//! health check.

use crate::auth::password;
use crate::auth::store::{AuthDb, Role, UserFields};
use crate::config::Config;
use anyhow::{Context, bail};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const USAGE: &str = "usage: localapp-server [serve]
       localapp-server admin create <username>
       localapp-server admin reset-password <username>
       localapp-server admin list
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
