//! Signed-in sessions (only token hashes are stored).

use super::*;

impl AuthDb {
    pub fn insert_session(
        &self,
        token_hash: &str,
        user_id: i64,
        persistent: bool,
        expires_at: i64,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp();
        self.conn().execute(
            "INSERT INTO sessions (token_hash, user_id, persistent, created_at, expires_at, \
                last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
            params![token_hash, user_id, persistent, now, expires_at],
        )?;
        Ok(())
    }

    /// The signed-in user for a session, if it's unexpired and the user is enabled.
    /// Returns `(user, persistent, last_seen)`.
    pub fn session_user(
        &self,
        token_hash: &str,
        now: i64,
    ) -> anyhow::Result<Option<(User, bool, i64)>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {USER_COLS}, s.persistent, s.last_seen FROM sessions s JOIN users u \
                        ON u.id = s.user_id
                     WHERE s.token_hash=?1 AND s.expires_at > ?2 AND u.disabled = 0"
                ),
                params![token_hash, now],
                |r| {
                    Ok((
                        user_from_row(r)?,
                        r.get(USER_COL_COUNT)?,
                        r.get(USER_COL_COUNT + 1)?,
                    ))
                },
            )
            .optional()?)
    }

    pub fn touch_session(&self, token_hash: &str, now: i64, expires_at: i64) -> anyhow::Result<()> {
        self.conn().execute(
            "UPDATE sessions SET last_seen=?1, expires_at=?2 WHERE token_hash=?3",
            params![now, expires_at, token_hash],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, token_hash: &str) -> anyhow::Result<()> {
        self.conn()
            .execute("DELETE FROM sessions WHERE token_hash=?1", [token_hash])?;
        Ok(())
    }

    /// Sign a user out everywhere except (optionally) one session.
    pub fn delete_sessions_for(&self, user_id: i64, keep: Option<&str>) -> anyhow::Result<()> {
        self.conn().execute(
            "DELETE FROM sessions WHERE user_id=?1 AND token_hash<>IFNULL(?2, '')",
            params![user_id, keep],
        )?;
        Ok(())
    }

    pub fn session_count(&self, user_id: i64) -> anyhow::Result<i64> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM sessions WHERE user_id=?1",
            [user_id],
            |r| r.get(0),
        )?)
    }

    pub fn purge_expired_sessions(&self) -> anyhow::Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM sessions WHERE expires_at <= ?1",
            [Utc::now().timestamp()],
        )?)
    }
}
