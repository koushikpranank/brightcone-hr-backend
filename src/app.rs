use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::Queue,
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::truncate_table,
    environment::Environment,
    task::Tasks,
    Result,
};
use std::path::Path;

use crate::{controllers, initializers::SchemaSyncInitializer, models::_entities::users, tasks};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        if environment == &Environment::Test {
            let test_database = crate::database::Database::connect(true).await?;
            test_database.close().await?;
            let test_database_url = std::env::var("TEST_DATABASE_URL").map_err(|_| {
                loco_rs::Error::string("TEST_DATABASE_URL is required in test environment")
            })?;
            crate::database::reset_test_data(&test_database_url)
                .await
                .map_err(|error| loco_rs::Error::string(&error))?;
        }
        create_app::<Self, crate::database::EmptyMigrator>(mode, environment, config).await
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(SchemaSyncInitializer)])
    }

    fn routes(ctx: &AppContext) -> AppRoutes {
        let routes = AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::health::routes())
            .add_route(controllers::auth::routes(ctx))
            .add_route(controllers::me::routes())
            .add_route(controllers::admin_users::routes())
            .add_route(controllers::audit_log::routes())
            .add_route(controllers::people::routes())
            .add_route(controllers::people_mutations::routes())
            .add_route(controllers::people_references::routes())
            .add_route(controllers::people_reference_mutations::routes())
            .add_route(controllers::people_csv::routes())
            .add_route(controllers::people_security::routes())
            .add_route(controllers::records::routes())
            .add_route(controllers::record_types::routes())
            .add_route(controllers::exports::routes())
            .add_route(controllers::files::routes())
            .add_route(controllers::notifications::routes())
            .add_route(controllers::rules::routes());
        // API docs are for developers, so they are not served in production.
        if ctx.environment == Environment::Production {
            routes
        } else {
            routes.add_route(controllers::docs::routes())
        }
    }
    async fn connect_workers(_ctx: &AppContext, _queue: &Queue) -> Result<()> {
        // Workers are registered here as tickets add them (PDF generation, CSV import, ...).
        Ok(())
    }

    #[allow(unused_variables)]
    fn register_tasks(tasks: &mut Tasks) {
        // tasks-inject (do not remove)
        tasks.register(tasks::user_create::UserCreate);
        tasks.register(tasks::seed_demo::SeedDemo);
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(_ctx: &AppContext, _base: &Path) -> Result<()> {
        // Demo data comes from the seed_demo task, which generates random passwords.
        Ok(())
    }
}
