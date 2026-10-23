use async_trait::async_trait;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

#[derive(Debug)]
pub struct EmptyMigrator;

#[async_trait]
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
