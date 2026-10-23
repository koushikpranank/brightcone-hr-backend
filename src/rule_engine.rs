use loco_rs::prelude::*;
use sea_orm::{ConnectionTrait, FromQueryResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    database::sql,
    workers::rule_dispatch::{RuleDispatchArgs, RuleDispatchWorker},
};

const RULES: [&str; 9] = [
    "employee.created",
    "employee.updated",
    "employee.archived",
    "leave.submitted",
    "leave.decided",
    "expense.submitted",
    "expense.decided",
    "candidate.stage_changed",
    "payroll.completed",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RuleEventPayload {
    pub record_id: Uuid,
    pub record_type: String,
    pub actor_user_id: i64,
}

pub fn event_key(
    record_type: &str,
    action: &str,
    before: Option<&serde_json::Value>,
    after: &serde_json::Value,
) -> Option<&'static str> {
    match (record_type, action) {
        ("employee", "created") => Some("employee.created"),
        ("employee", "updated") => Some("employee.updated"),
        ("employee", "archived") => Some("employee.archived"),
        ("leave_request", "created") if status(after) == Some("pending") => Some("leave.submitted"),
        ("leave_request", "updated") if transitioned(before, after, &["approved", "rejected"]) => {
            Some("leave.decided")
        }
        ("expense_claim", "created") if status(after) == Some("pending") => {
            Some("expense.submitted")
        }
        ("expense_claim", "updated") if transitioned(before, after, &["approved", "rejected"]) => {
            Some("expense.decided")
        }
        ("candidate", "updated")
            if before.and_then(|v| v.get("stage")) != after.get("stage")
                && after.get("stage").is_some() =>
        {
            Some("candidate.stage_changed")
        }
        ("payroll_run", "updated")
            if status(after) == Some("completed")
                && before.and_then(status) != Some("completed") =>
        {
            Some("payroll.completed")
        }
        _ => None,
    }
}

fn status(value: &serde_json::Value) -> Option<&str> {
    value.get("status").and_then(serde_json::Value::as_str)
}

fn transitioned(
    before: Option<&serde_json::Value>,
    after: &serde_json::Value,
    targets: &[&str],
) -> bool {
    let current = status(after);
    current.is_some_and(|value| targets.contains(&value)) && before.and_then(status) != current
}

#[derive(FromQueryResult)]
struct EventIdRow {
    event_id: Uuid,
}

pub async fn record_event(
    connection: &impl ConnectionTrait,
    key: &str,
    payload: &RuleEventPayload,
) -> std::result::Result<Option<Uuid>, sea_orm::DbErr> {
    if !RULES.contains(&key) {
        return Ok(None);
    }
    let event_id = Uuid::new_v4();
    let json = serde_json::to_value(payload).expect("rule event payload is serializable");
    sql::optional::<EventIdRow>(connection,
        "INSERT INTO hr_rule_events(event_id, action_key, payload) SELECT $1,$2,$3 WHERE EXISTS (SELECT 1 FROM hr_rules WHERE rule_key=$2 AND enabled=true) RETURNING event_id",
        vec![event_id.into(), key.to_owned().into(), json.into()]).await.map(|row| row.map(|event| event.event_id))
}

#[derive(FromQueryResult)]
struct PendingEvent {
    event_id: Uuid,
}

pub async fn dispatch(ctx: &AppContext, event_id: Option<Uuid>) {
    let Some(event_id) = event_id else { return };
    if let Err(error) = RuleDispatchWorker::perform_later(ctx, RuleDispatchArgs { event_id }).await
    {
        tracing::error!(%event_id, error = %error, "rule event remains in the durable outbox");
    }
}

pub async fn enqueue_pending(ctx: &AppContext) -> Result<()> {
    let pending = sql::all::<PendingEvent>(
        &ctx.db,
        "SELECT event_id FROM hr_rule_events WHERE status='pending' ORDER BY created_at LIMIT 500",
        Vec::new(),
    )
    .await?;
    let args = pending
        .into_iter()
        .map(|row| RuleDispatchArgs {
            event_id: row.event_id,
        })
        .collect();
    RuleDispatchWorker::perform_all_later(ctx, args).await?;
    Ok(())
}

#[derive(FromQueryResult)]
struct AttemptRow {
    attempts: i32,
}

#[derive(FromQueryResult)]
struct EventRow {
    event_id: Uuid,
    action_key: String,
    payload: serde_json::Value,
    enabled: bool,
}

pub async fn process_event(ctx: &AppContext, event_id: Uuid) -> Result<()> {
    let attempt = sql::optional::<AttemptRow>(&ctx.db,
        "UPDATE hr_rule_events SET attempts=attempts+1 WHERE event_id=$1 AND status='pending' RETURNING attempts", vec![event_id.into()]).await?;
    let Some(attempt) = attempt else {
        return Ok(());
    };
    if attempt.attempts > 5 {
        sql::execute(&ctx.db, "UPDATE hr_rule_events SET status='failed', last_error='retry limit exceeded' WHERE event_id=$1", vec![event_id.into()]).await?;
        return Ok(());
    }
    ctx.db.transaction::<_, (), sea_orm::DbErr>(|tx| {
        Box::pin(async move {
            let Some(event) = sql::optional::<EventRow>(tx,
                "SELECT e.event_id, e.action_key, e.payload, r.enabled FROM hr_rule_events e JOIN hr_rules r ON r.rule_key=e.action_key WHERE e.event_id=$1 AND e.status='pending' FOR UPDATE OF e", vec![event_id.into()]).await? else { return Ok(()) };
            if event.enabled {
                if let Some(roles) = recipient_roles(&event.action_key) {
                    for role in roles {
                        sql::execute(tx,
                            "INSERT INTO hr_notifications(user_id,event_id,rule_key,title,body) SELECT id,$1,$2,$3,$4 FROM users WHERE role=$5 AND status='active' ON CONFLICT(event_id,user_id) DO NOTHING",
                            vec![event.event_id.into(), event.action_key.clone().into(), notification_title(&event.action_key).into(), notification_body(&event.payload).into(), (*role).into()]).await?;
                    }
                }
            }
            sql::execute(tx, "UPDATE hr_rule_events SET status='complete', completed_at=now() WHERE event_id=$1", vec![event_id.into()]).await?;
            Ok(())
        })
    }).await.map_err(|error| {
        tracing::error!(%error, "rule event transaction failed");
        Error::string("Rule event processing failed")
    })
}

pub async fn record_failure(ctx: &AppContext, event_id: Uuid) -> Result<()> {
    sql::execute(&ctx.db,
        "UPDATE hr_rule_events SET last_error='Rule action failed; see application logs.', status=CASE WHEN attempts >= 5 THEN 'failed' ELSE status END WHERE event_id=$1 AND status='pending'",
        vec![event_id.into()]).await?;
    Ok(())
}

fn recipient_roles(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "employee.created" | "employee.updated" | "employee.archived" => Some(&["hr_admin"]),
        "leave.submitted" | "leave.decided" => Some(&["hr_admin", "manager"]),
        "expense.submitted" | "expense.decided" => Some(&["hr_admin", "finance"]),
        "candidate.stage_changed" => Some(&["hr_admin", "recruiter"]),
        "payroll.completed" => Some(&["hr_admin", "payroll_admin"]),
        _ => None,
    }
}

fn notification_title(key: &str) -> &'static str {
    match key {
        "employee.created" => "Employee record created",
        "employee.updated" => "Employee record updated",
        "employee.archived" => "Employee record archived",
        "leave.submitted" => "Leave request submitted",
        "leave.decided" => "Leave request decided",
        "expense.submitted" => "Expense claim submitted",
        "expense.decided" => "Expense claim decided",
        "candidate.stage_changed" => "Candidate stage changed",
        "payroll.completed" => "Payroll run completed",
        _ => "HR activity",
    }
}

fn notification_body(payload: &serde_json::Value) -> String {
    format!(
        "A {} record was updated. Reference: {}.",
        payload
            .get("record_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("HR"),
        payload
            .get("record_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unavailable")
    )
}

#[cfg(test)]
mod tests {
    use super::{event_key, recipient_roles};
    use serde_json::json;

    #[test]
    fn emits_events_for_domain_transitions_and_separates_recipient_groups() {
        assert_eq!(
            event_key("employee", "created", None, &json!({})),
            Some("employee.created")
        );
        assert_eq!(
            event_key(
                "leave_request",
                "created",
                None,
                &json!({"status":"pending"})
            ),
            Some("leave.submitted")
        );
        assert_eq!(
            event_key(
                "leave_request",
                "updated",
                Some(&json!({"status":"pending"})),
                &json!({"status":"approved"})
            ),
            Some("leave.decided")
        );
        assert_eq!(
            event_key(
                "candidate",
                "updated",
                Some(&json!({"stage":"applied"})),
                &json!({"stage":"interview"})
            ),
            Some("candidate.stage_changed")
        );
        assert_eq!(
            recipient_roles("payroll.completed"),
            Some(&["hr_admin", "payroll_admin"][..])
        );
        assert_eq!(
            recipient_roles("candidate.stage_changed"),
            Some(&["hr_admin", "recruiter"][..])
        );
        assert_eq!(
            recipient_roles("expense.decided"),
            Some(&["hr_admin", "finance"][..])
        );
    }
}
