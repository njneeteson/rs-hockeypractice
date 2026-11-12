//! `localapp-server` command line.
//!
//! ```text
//! localapp-server                              run the web server (same as `serve`)
//! localapp-server admin create <username>      create an admin with a temporary password
//! localapp-server admin reset-password <user>  give a user a new temporary password
//! localapp-server admin list                   list accounts
//! localapp-server healthcheck                  exit 0 if the server is up
//! ```
//! Configuration comes from `LOCALAPP_*` environment variables (see `config.rs`).

use anyhow::{Context, bail};
use axum::ServiceExt;
use axum::extract::Request;
use localapp_server::cli::{self, USAGE};
use localapp_server::{AppState, Config, router};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env()?;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["serve"] => serve(cfg).await,
        ["admin", rest @ ..] => cli::admin(&cfg, rest),
        ["healthcheck"] => cli::healthcheck(&cfg),
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => bail!("{USAGE}"),
    }
}

async fn serve(cfg: Config) -> anyhow::Result<()> {
    let bind = cfg.bind;
    let st = AppState::new(cfg)?;
    if let Some(token) = st.setup_token() {
        tracing::warn!(
            "No admin account exists yet. Open {} in your browser to create one (or run \
                `localapp-server admin create <name>`).",
            st.url(&format!("/setup?token={token}"))
        );
    }
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!("LocalApp listening on http://{bind}{}/", st.cfg.base_path);
    let app = ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(router(st));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
