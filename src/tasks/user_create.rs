use loco_rs::{hash, prelude::*};

use crate::{
    authz::Role,
    models::users::{self, NewUser},
};

/// Creates one account from the command line. Use it to create the very first HR Admin.
pub struct UserCreate;

#[async_trait]
impl Task for UserCreate {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "user:create".to_string(),
            detail: "Create an account. The password is generated and printed once.\nUsage:\ncargo loco task user:create email:ananya.rao@brightcone.ai name:\"Ananya Rao\" role:hr_admin"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let email = vars
            .cli_arg("email")
            .map_err(|_| Error::string("email is mandatory"))?;
        let name = vars
            .cli_arg("name")
            .map_err(|_| Error::string("name is mandatory"))?;
        let role = vars.cli_arg("role").unwrap_or("employee");
        let role = Role::parse(role).ok_or_else(|| {
            Error::string("role must be one of: employee, manager, finance, hr_admin")
        })?;

        let password = format!("{}-Aa1", hash::random_string(16));
        let user = users::Model::create_with_password(
            &ctx.db,
            &NewUser {
                email: email.to_owned(),
                name: name.to_owned(),
                password: password.clone(),
                role,
                must_change_password: true,
            },
        )
        .await?;
        println!("Created {} ({}) as {}", user.name, user.email, user.role);
        println!("Temporary password (shown once, must be changed at first sign-in): {password}");
        Ok(())
    }
}
