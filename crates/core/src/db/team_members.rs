//! The `team_members` table: who is in which team, and as what.
//!
//! A team always keeps at least one owner: the last one can't leave, be
//! removed or become a coach.

use super::Database;
use crate::owner::{Member, MemberId, TeamRole, limits};
use crate::practice::Team;
use rusqlite::{OptionalExtension, params};

impl Database {
    /// The member's role in a team, or `None` if they aren't in it.
    pub fn member_role(&self, team: i64, member: MemberId) -> anyhow::Result<Option<TeamRole>> {
        let role: Option<String> = self
            .conn
            .query_row(
                "SELECT role FROM team_members WHERE team_id = ?1 AND member_id = ?2",
                params![team, member.0],
                |r| r.get(0),
            )
            .optional()?;
        Ok(role.map(|r| TeamRole::from_key(&r).unwrap_or(TeamRole::Coach)))
    }

    /// A team's members: owners first, then by when they joined.
    pub fn members(&self, team: i64) -> anyhow::Result<Vec<Member>> {
        self.query_all(
            "SELECT member_id, role, joined_at FROM team_members WHERE team_id = ?1
             ORDER BY role = 'owner' DESC, joined_at, member_id",
            [team],
            |r| {
                let role: String = r.get(1)?;
                Ok(Member {
                    id: MemberId(r.get(0)?),
                    role: TeamRole::from_key(&role).unwrap_or(TeamRole::Coach),
                    joined_at: r.get(2)?,
                })
            },
        )
    }

    /// How many teams a member owns.
    pub fn teams_owned(&self, member: MemberId) -> anyhow::Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM team_members WHERE member_id = ?1 AND role = 'owner'",
            [member.0],
            |r| r.get(0),
        )?)
    }

    fn owners(&self, team: i64) -> anyhow::Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM team_members WHERE team_id = ?1 AND role = 'owner'",
            [team],
            |r| r.get(0),
        )?)
    }

    pub fn add_member(&self, team: i64, member: MemberId, role: TeamRole) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.member_role(team, member)?.is_none(),
            "They're already in this team."
        );
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM team_members WHERE team_id = ?1",
            [team],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            count < limits::MEMBERS_PER_TEAM,
            "A team has at most {} members.",
            limits::MEMBERS_PER_TEAM
        );
        if role == TeamRole::Owner {
            anyhow::ensure!(
                self.teams_owned(member)? < limits::TEAMS_OWNED,
                "They already own {} teams, the most anyone can.",
                limits::TEAMS_OWNED
            );
        }
        self.conn.execute(
            "INSERT INTO team_members (team_id, member_id, role) VALUES (?1, ?2, ?3)",
            params![team, member.0, role.key()],
        )?;
        Ok(())
    }

    /// Make a member an owner or a coach.
    pub fn set_role(&self, team: i64, member: MemberId, role: TeamRole) -> anyhow::Result<()> {
        let current = self
            .member_role(team, member)?
            .ok_or_else(|| anyhow::anyhow!("They aren't in this team."))?;
        if current == role {
            return Ok(());
        }
        if current == TeamRole::Owner {
            anyhow::ensure!(
                self.owners(team)? > 1,
                "A team needs an owner. Make someone else an owner first."
            );
        } else {
            anyhow::ensure!(
                self.teams_owned(member)? < limits::TEAMS_OWNED,
                "They already own {} teams, the most anyone can.",
                limits::TEAMS_OWNED
            );
        }
        self.conn.execute(
            "UPDATE team_members SET role = ?3 WHERE team_id = ?1 AND member_id = ?2",
            params![team, member.0, role.key()],
        )?;
        Ok(())
    }

    /// Take someone out of a team (or let them leave). The team's practices
    /// and drills stay with the team.
    pub fn remove_member(&self, team: i64, member: MemberId) -> anyhow::Result<()> {
        let role = self
            .member_role(team, member)?
            .ok_or_else(|| anyhow::anyhow!("They aren't in this team."))?;
        if role == TeamRole::Owner {
            anyhow::ensure!(
                self.owners(team)? > 1,
                "A team needs an owner. Make someone else an owner first, or delete the team."
            );
        }
        self.conn.execute(
            "DELETE FROM team_members WHERE team_id = ?1 AND member_id = ?2",
            params![team, member.0],
        )?;
        Ok(())
    }

    /// Teams that would be left without an owner if `member` went away.
    pub fn teams_only_owned_by(&self, member: MemberId) -> anyhow::Result<Vec<Team>> {
        let ids: Vec<i64> = self.query_all(
            "SELECT team_id FROM team_members m WHERE member_id = ?1 AND role = 'owner'
               AND (SELECT COUNT(*) FROM team_members o
                    WHERE o.team_id = m.team_id AND o.role = 'owner') = 1",
            [member.0],
            |r| r.get(0),
        )?;
        let mut teams = Vec::new();
        for id in ids {
            teams.extend(self.team(id)?);
        }
        Ok(teams)
    }

    /// Everything a person has, for when their account is deleted: their
    /// memberships and their own drill library. Refused while a team would be
    /// left without an owner.
    pub fn forget_member(&self, member: MemberId) -> anyhow::Result<()> {
        let orphans = self.teams_only_owned_by(member)?;
        if let Some(t) = orphans.first() {
            anyhow::bail!(
                "They're the only owner of {}. Make someone else its owner, or delete it, first.",
                t.name
            );
        }
        self.atomic(|| {
            self.conn
                .execute("DELETE FROM team_members WHERE member_id = ?1", [member.0])?;
            self.delete_personal_drills(member)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::drills::tests::{ME, sample};
    use crate::owner::Library;
    use crate::rink::IceSize;

    #[test]
    fn a_team_always_has_an_owner() {
        let db = Database::in_memory().unwrap();
        let sam = MemberId(2);
        let team = db.create_team(ME, "U13", "").unwrap();
        assert_eq!(db.member_role(team, ME).unwrap(), Some(TeamRole::Owner));
        assert_eq!(db.member_role(team, sam).unwrap(), None);

        assert!(db.remove_member(team, ME).is_err(), "the last owner stays");
        assert!(db.set_role(team, ME, TeamRole::Coach).is_err());
        db.add_member(team, sam, TeamRole::Coach).unwrap();
        assert!(db.add_member(team, sam, TeamRole::Coach).is_err());
        assert_eq!(db.members(team).unwrap()[0].id, ME, "owners first");
        assert_eq!(db.teams_only_owned_by(ME).unwrap().len(), 1);
        assert!(db.forget_member(ME).is_err());

        // Hand the team over, then leave.
        db.set_role(team, sam, TeamRole::Owner).unwrap();
        assert!(db.teams_only_owned_by(ME).unwrap().is_empty());
        db.remove_member(team, ME).unwrap();
        assert_eq!(db.member_role(team, ME).unwrap(), None);
        assert!(db.remove_member(team, ME).is_err());

        // Forgetting someone removes them and their own drills.
        db.add_member(team, ME, TeamRole::Coach).unwrap();
        let mine = db
            .create_drill(Library::Personal(ME), sample("Mine", IceSize::HALF))
            .unwrap();
        db.forget_member(ME).unwrap();
        assert_eq!(db.member_role(team, ME).unwrap(), None);
        assert!(db.drill(mine).unwrap().is_none());
    }

    #[test]
    fn teams_have_a_member_limit() {
        let db = Database::in_memory().unwrap();
        let team = db.create_team(ME, "Big", "").unwrap();
        for i in 1..limits::MEMBERS_PER_TEAM {
            db.add_member(team, MemberId(100 + i as i64), TeamRole::Coach)
                .unwrap();
        }
        assert!(db.add_member(team, MemberId(99), TeamRole::Coach).is_err());
    }
}
