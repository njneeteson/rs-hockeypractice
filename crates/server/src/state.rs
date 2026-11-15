//! Shared application state.

use crate::auth::password;
use crate::auth::ratelimit::RateLimiter;
use crate::auth::store::{AuthDb, Role, UserFields};
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::throttle::RenderGate;
use anyhow::Context;
use localapp_core::db::Database;
use localapp_core::owner::MemberId;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::SemaphorePermit;

pub type Shared = Arc<AppState>;

pub struct AppState {
    pub cfg: Config,
    /// Accounts and sessions (`auth.sqlite`).
    pub auth: AuthDb,
    /// App data (`app.sqlite`); reach it through [`AppState::db`].
    db: Arc<Mutex<Database>>,
    pub limiter: RateLimiter,
    /// Limits on PDF and preview rendering (see [`crate::throttle`]).
    pub renders: RenderGate,
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
        adopt_unowned(&auth, &db)?;
        let setup_token = (auth.count_admins()? == 0).then(crate::auth::session::new_token);
        Ok(Arc::new(Self {
            cfg,
            auth,
            db: Arc::new(Mutex::new(db)),
            limiter: RateLimiter::default(),
            renders: RenderGate::default(),
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

    /// Permission to render a PDF or preview for `user`: counts against their
    /// per-minute limit, then waits for a free render slot.
    pub async fn render_permit(&self, user: i64) -> AppResult<SemaphorePermit<'_>> {
        if !self.renders.allow(user, Instant::now()) {
            return Err(AppError::TooMany(
                "That's a lot of printing in one minute. Wait a moment and try again.",
            ));
        }
        Ok(self.renders.slot().await?)
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

/// Teams and drills from before owners existed (schema V6) go to the first
/// admin, with everyone else as team coaches, as they were shared before.
/// Waits until there is an account to give them to.
fn adopt_unowned(auth: &AuthDb, db: &Database) -> anyhow::Result<()> {
    if !db.has_unowned()? {
        return Ok(());
    }
    let users = auth.users()?;
    let first_admin = users.iter().filter(|u| u.is_admin()).map(|u| u.id).min();
    let Some(owner) = first_admin.or_else(|| users.iter().map(|u| u.id).min()) else {
        return Ok(());
    };
    let everyone: Vec<MemberId> = users
        .iter()
        .filter(|u| !u.disabled)
        .map(|u| MemberId(u.id))
        .collect();
    let done = db.adopt_unowned(MemberId(owner), &everyone)?;
    tracing::info!(
        "gave owners to existing data: {} team(s) got members, {} drill(s) copied into team \
         libraries, {} drill(s) moved to user {owner}'s library",
        done.teams,
        done.team_drills,
        done.personal_drills
    );
    Ok(())
}
