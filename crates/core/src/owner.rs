//! Who owns what: team members, drill libraries and the limits on both.
//!
//! The core stores members by the server's user id ([`MemberId`]) but never
//! authenticates anyone: the server decides who is asking, and checks with
//! these types what they may see and change.

/// Someone using the app, by the server's user id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemberId(pub i64);

str_enum! {
    /// What a member may do in a team. Every member plans practices and edits
    /// the team's drills; owners also manage the team and its members.
    pub enum TeamRole {
        Owner => ("owner", "Owner"),
        Coach => ("coach", "Coach"),
    }
}

/// A team member.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: MemberId,
    pub role: TeamRole,
    pub joined_at: String,
}

/// The library a drill lives in: one person's, or a team's (shared by its
/// members). Practices only use their team's drills; a drill from anywhere
/// else is copied into the team's library first, so editing a drill never
/// changes anyone else's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Library {
    Personal(MemberId),
    Team(i64),
}

impl Library {
    /// A stable form for URLs and forms: `me` (with the member it means) or
    /// `team-<id>`.
    pub fn key(self) -> String {
        match self {
            Library::Personal(_) => "me".into(),
            Library::Team(t) => format!("team-{t}"),
        }
    }

    /// Parse [`Library::key`]; `me` is the given member's own library.
    pub fn from_key(key: &str, me: MemberId) -> Option<Library> {
        match key.trim() {
            "" | "me" => Some(Library::Personal(me)),
            k => k
                .strip_prefix("team-")
                .and_then(|t| t.parse().ok())
                .map(Library::Team),
        }
    }

    pub fn team(self) -> Option<i64> {
        match self {
            Library::Team(t) => Some(t),
            Library::Personal(_) => None,
        }
    }
}

/// Limits that keep one account from filling the server.
pub mod limits {
    pub const DRILLS_PER_LIBRARY: u32 = 1000;
    pub const PRACTICES_PER_TEAM: u32 = 500;
    pub const TEAMS_OWNED: u32 = 20;
    pub const MEMBERS_PER_TEAM: u32 = 40;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_keys_round_trip() {
        let me = MemberId(7);
        for lib in [Library::Personal(me), Library::Team(3)] {
            assert_eq!(Library::from_key(&lib.key(), me), Some(lib));
        }
        assert_eq!(Library::from_key("", me), Some(Library::Personal(me)));
        assert_eq!(Library::from_key("team-x", me), None);
        assert_eq!(Library::from_key("other", me), None);
    }
}
