//! User accounts: create, edit, delete, passwords and preferences.

use super::*;

impl AuthDb {
    pub fn users(&self) -> anyhow::Result<Vec<User>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {USER_COLS} FROM users u ORDER BY u.username COLLATE NOCASE"
        ))?;
        Ok(stmt
            .query_map([], user_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn user(&self, id: i64) -> anyhow::Result<Option<User>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {USER_COLS} FROM users u WHERE u.id=?1"),
                [id],
                user_from_row,
            )
            .optional()?)
    }

    /// The user and their password hash, for signing in.
    pub fn credentials(&self, username: &str) -> anyhow::Result<Option<(User, String)>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {USER_COLS}, u.password_hash FROM users u \
                     WHERE u.username=?1 COLLATE NOCASE"
                ),
                [username.trim()],
                |r| Ok((user_from_row(r)?, r.get(USER_COL_COUNT)?)),
            )
            .optional()?)
    }

    pub fn password_hash(&self, user_id: i64) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT password_hash FROM users WHERE id=?1",
                [user_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn count_admins(&self) -> anyhow::Result<i64> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM users WHERE role='admin' AND disabled=0",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn create_user(
        &self,
        f: &UserFields,
        password_hash: &str,
        must_change_password: bool,
    ) -> anyhow::Result<i64> {
        validate_username(f.username.trim())?;
        let conn = self.conn();
        let taken: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM users WHERE username=?1 COLLATE NOCASE",
            [f.username.trim()],
            |r| r.get(0),
        )?;
        if taken {
            bail!("The username “{}” is already taken.", f.username.trim());
        }
        conn.execute(
            "INSERT INTO users (username, display_name, password_hash, role, disabled, \
                must_change_password, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                f.username.trim(),
                f.display_name.trim(),
                password_hash,
                f.role.unwrap_or(Role::User).key(),
                f.disabled,
                must_change_password,
                now_text()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update profile, role and status. Refuses to remove the last active admin.
    pub fn update_user(&self, id: i64, f: &UserFields) -> anyhow::Result<()> {
        validate_username(f.username.trim())?;
        let before = self.user(id)?.context("user not found")?;
        let role = f.role.unwrap_or(before.role);
        let loses_admin =
            before.role == Role::Admin && !before.disabled && (role != Role::Admin || f.disabled);
        if loses_admin && self.count_admins()? <= 1 {
            bail!(
                "{} is the only active admin. Make someone else an admin first.",
                before.username
            );
        }
        let conn = self.conn();
        let taken: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM users WHERE username=?1 COLLATE NOCASE AND id<>?2",
            params![f.username.trim(), id],
            |r| r.get(0),
        )?;
        if taken {
            bail!("The username “{}” is already taken.", f.username.trim());
        }
        conn.execute(
            "UPDATE users SET username=?1, display_name=?2, role=?3, disabled=?4 WHERE id=?5",
            params![
                f.username.trim(),
                f.display_name.trim(),
                role.key(),
                f.disabled,
                id
            ],
        )?;
        if f.disabled {
            conn.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
        }
        Ok(())
    }

    /// Delete a user. Refuses to delete the last active admin.
    pub fn delete_user(&self, id: i64) -> anyhow::Result<()> {
        let u = self.user(id)?.context("user not found")?;
        if u.is_admin() && !u.disabled && self.count_admins()? <= 1 {
            bail!(
                "{} is the only active admin and can't be deleted.",
                u.username
            );
        }
        self.conn().execute("DELETE FROM users WHERE id=?1", [id])?;
        Ok(())
    }

    /// Replace the password and sign the user out everywhere.
    pub fn set_password(
        &self,
        id: i64,
        password_hash: &str,
        must_change_password: bool,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE users SET password_hash=?1, must_change_password=?2 WHERE id=?3",
            params![password_hash, must_change_password, id],
        )?;
        conn.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
        Ok(())
    }

    pub fn set_preferences(&self, id: i64, display_name: &str, theme: Theme) -> anyhow::Result<()> {
        self.conn().execute(
            "UPDATE users SET display_name=?1, theme=?2 WHERE id=?3",
            params![display_name.trim(), theme.key(), id],
        )?;
        Ok(())
    }

    pub fn record_login(&self, id: i64) -> anyhow::Result<()> {
        self.conn().execute(
            "UPDATE users SET last_login=?1 WHERE id=?2",
            params![now_text(), id],
        )?;
        Ok(())
    }
}
