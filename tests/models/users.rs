use brightcone_hr_backend::{
    app::App,
    authz::Role,
    models::users::{self, check_password_strength, normalize_email, NewUser},
};
use loco_rs::testing::prelude::*;
use serial_test::serial;

fn new_user(email: &str) -> NewUser {
    NewUser {
        email: email.to_owned(),
        name: "Test Person".to_owned(),
        password: "correct horse battery staple 9".to_owned(),
        role: Role::Employee,
        must_change_password: false,
    }
}

#[test]
fn emails_are_trimmed_and_lowercased() {
    assert_eq!(
        normalize_email("  Rahul.Varma@Brightcone.AI "),
        "rahul.varma@brightcone.ai"
    );
}

#[test]
fn password_rules() {
    assert!(check_password_strength("short").is_err());
    assert!(check_password_strength("password1234").is_err());
    assert!(check_password_strength("PASSWORD1234").is_err());
    assert!(check_password_strength("aaaaaaaaaaaaaaaa").is_err());
    assert!(check_password_strength("correct horse battery staple").is_ok());
}

#[tokio::test]
#[serial]
async fn passwords_are_stored_hashed_and_roles_default_safely() {
    let boot = boot_test::<App>().await.unwrap();
    let user = users::Model::create_with_password(&boot.app_context.db, &new_user("a@x.ai"))
        .await
        .unwrap();
    assert!(user.password.starts_with("$argon2"));
    assert!(user.verify_password("correct horse battery staple 9"));
    assert!(!user.verify_password("something else entirely"));
    assert_eq!(user.role(), Some(Role::Employee));
    assert_eq!(user.status, "active");
}

#[tokio::test]
#[serial]
async fn the_same_email_in_another_case_is_a_duplicate() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    users::Model::create_with_password(db, &new_user("Dup@x.ai"))
        .await
        .unwrap();
    let again = users::Model::create_with_password(db, &new_user("dup@X.AI")).await;
    assert!(again.is_err());
}

#[tokio::test]
#[serial]
async fn an_invalid_email_or_name_is_rejected() {
    let boot = boot_test::<App>().await.unwrap();
    let db = &boot.app_context.db;
    assert!(
        users::Model::create_with_password(db, &new_user("not-an-email"))
            .await
            .is_err()
    );
    let mut short = new_user("b@x.ai");
    short.name = "A".to_owned();
    assert!(users::Model::create_with_password(db, &short)
        .await
        .is_err());
}
