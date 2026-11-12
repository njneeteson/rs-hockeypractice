//! Shared application state.

use crate::auth::password;
use crate::auth::ratelimit::RateLimiter;
use crate::auth::store::{AuthDb, Role, UserFields};
use crate::config::Config;
use anyhow::Context;
use localapp_core::db::Database;
use std::sync::{Arc, Mutex};

pub type Shared = Arc<AppState>;

pub struct AppState {
    pub cfg: Config,
    /// Accounts and sessions (`auth.sqlite`).
    pub auth: AuthDb,
    /// App data (`app.sqlite`); reach it through [`AppState::db`].
    db: Arc<Mutex<Database>>,
    pub limiter: RateLimiter,
    /// One-time token for creating the first admin in the browser; `None` once an admin exists.
    setup_token: Mutex<Option<String>>,
}

impl AppState {
    /// Open (or create) the data directory and prepare the server.
    pub fn new(cfg: Config) -> anyhow::Result<Shared> {
        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("creating data directory {}", cfg.data_dir.display()))?;
        let auth = AuthDb::open(&cfg.auth_db_path())?;
        let db = Database::open(&cfg.app_db_path())?;
        let purged = auth.purge_expired_sessions()?;
        if purged > 0 {
            tracing::debug!("removed {purged} expired sessions");
        }
        if auth.count_admins()? == 0
            && let Some((user, pass)) = &cfg.bootstrap_admin
        {
            password::validate_new(pass, pass)
                .map_err(anyhow::Error::msg)
                .context(crate::config::ENV_ADMIN_PASSWORD)?;
            let fields = UserFields {
                username: user.clone(),
                role: Some(Role::Admin),
                ..Default::default()
            };
            auth.create_user(&fields, &password::hash(pass)?, false)?;
            tracing::info!("created admin account {user:?} from the environment");
        }
        let setup_token = (auth.count_admins()? == 0).then(crate::auth::session::new_token);
        Ok(Arc::new(Self {
            cfg,
            auth,
            db: Arc::new(Mutex::new(db)),
            limiter: RateLimiter::default(),
            setup_token: Mutex::new(setup_token),
        }))
    }

    /// An absolute URL path under the base path: `url("/login")` → `/localapp/login`.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.cfg.base_path)
    }

    /// Run blocking database work on a worker thread, with the database locked.
    ///
    /// ```ignore
    /// let greeting = cx.db(|db| db.setting("greeting")).await?;
    /// ```
    pub async fn db<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> anyhow::Result<T> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let guard = db.lock().unwrap_or_else(|e| e.into_inner());
            f(&guard)
        })
        .await?
    }

    pub fn setup_token(&self) -> Option<String> {
        self.setup_token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn clear_setup_token(&self) {
        *self.setup_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
