use loco_rs::{environment::Environment, hash, prelude::*};

use crate::{
    authz::Role,
    models::users::{self, NewUser},
};

/// Demo accounts for development. Replaces the prototype's `seed()`; employee records, departments
/// and leave data are added to this task by the Phase 2 and 3 tickets.
pub struct SeedDemo;

const PEOPLE: &[(&str, Role)] = &[
    ("Ananya Rao", Role::HrAdmin),
    ("Naresh Vemparala", Role::Manager),
    ("Karthik Reddy", Role::Manager),
    ("Priya Menon", Role::Manager),
    ("Meera Joshi", Role::Finance),
    ("Rahul Varma", Role::Employee),
    ("Sneha Iyer", Role::Employee),
    ("Arjun Nair", Role::Employee),
    ("Daniel Brooks", Role::Employee),
    ("Emily Carter", Role::Employee),
    ("Vikram Singh", Role::Employee),
    ("Lakshmi Prasad", Role::Employee),
    ("Farhan Ali", Role::Employee),
    ("Divya Kulkarni", Role::Employee),
];

/// `Ananya Rao` becomes `ananya.rao@brightcone.ai`.
fn email_for(name: &str) -> String {
    let local: Vec<String> = name
        .to_lowercase()
        .split_whitespace()
        .map(|part| part.chars().filter(char::is_ascii_alphabetic).collect())
        .collect();
    format!("{}@brightcone.ai", local.join("."))
}

#[async_trait]
impl Task for SeedDemo {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "seed_demo".to_string(),
            detail: "Create demo accounts with random passwords, printed once. Refuses to run in production.\nUsage: cargo loco task seed_demo"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        if ctx.environment == Environment::Production {
            return Err(Error::string("seed_demo must never run in production"));
        }
        for (name, role) in PEOPLE {
            let email = email_for(name);
            if users::Model::find_by_email(&ctx.db, &email).await.is_ok() {
                println!("skip    {email} (already exists)");
                continue;
            }
            let password = format!("{}-Aa1", hash::random_string(16));
            users::Model::create_with_password(
                &ctx.db,
                &NewUser {
                    email: email.clone(),
                    name: (*name).to_owned(),
                    password: password.clone(),
                    role: *role,
                    must_change_password: false,
                },
            )
            .await?;
            println!("created {email:<34} {:<9} {password}", role.as_str());
        }
        println!("\nThese passwords are shown once.");
        Ok(())
    }
}
