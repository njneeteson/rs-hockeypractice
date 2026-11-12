use super::*;

fn fields(name: &str, role: Role) -> UserFields {
    UserFields {
        username: name.into(),
        role: Some(role),
        ..Default::default()
    }
}

#[test]
fn usernames_are_unique_ignoring_case() {
    let db = AuthDb::in_memory().unwrap();
    db.create_user(&fields("Alice", Role::Admin), "h", false)
        .unwrap();
    assert!(
        db.create_user(&fields("alice", Role::User), "h", false)
            .is_err()
    );
    assert!(
        db.create_user(&fields("bad name", Role::User), "h", false)
            .is_err()
    );
    let (u, hash) = db.credentials("ALICE").unwrap().unwrap();
    assert_eq!((u.username.as_str(), hash.as_str()), ("Alice", "h"));
}

#[test]
fn the_last_admin_is_protected() {
    let db = AuthDb::in_memory().unwrap();
    let a = db
        .create_user(&fields("alice", Role::Admin), "h", false)
        .unwrap();
    assert!(db.update_user(a, &fields("alice", Role::User)).is_err());
    let mut disabled = fields("alice", Role::Admin);
    disabled.disabled = true;
    assert!(db.update_user(a, &disabled).is_err());
    assert!(db.delete_user(a).is_err());

    let b = db
        .create_user(&fields("bob", Role::Admin), "h", false)
        .unwrap();
    db.update_user(a, &fields("alice", Role::User)).unwrap();
    assert_eq!(db.count_admins().unwrap(), 1);
    assert!(db.delete_user(b).is_err());
    db.delete_user(a).unwrap();
}

#[test]
fn sessions_expire_and_end_with_password_changes() {
    let db = AuthDb::in_memory().unwrap();
    let id = db
        .create_user(&fields("alice", Role::User), "h", false)
        .unwrap();
    let now = Utc::now().timestamp();
    db.insert_session("live", id, false, now + 60).unwrap();
    db.insert_session("old", id, false, now - 1).unwrap();
    assert!(db.session_user("live", now).unwrap().is_some());
    assert!(db.session_user("old", now).unwrap().is_none());
    assert_eq!(db.purge_expired_sessions().unwrap(), 1);

    db.set_password(id, "h2", false).unwrap();
    assert!(db.session_user("live", now).unwrap().is_none());
}

#[test]
fn disabling_a_user_signs_them_out() {
    let db = AuthDb::in_memory().unwrap();
    db.create_user(&fields("admin", Role::Admin), "h", false)
        .unwrap();
    let id = db
        .create_user(&fields("bob", Role::User), "h", false)
        .unwrap();
    let now = Utc::now().timestamp();
    db.insert_session("s", id, true, now + 60).unwrap();
    let mut f = fields("bob", Role::User);
    f.disabled = true;
    db.update_user(id, &f).unwrap();
    assert!(db.session_user("s", now).unwrap().is_none());
    assert_eq!(db.session_count(id).unwrap(), 0);
}
