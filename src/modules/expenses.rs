use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpenseClaim {
    pub id: String,
    pub employee_id: String,
    pub employee_name: String,
    pub category: String, // Travel, Meals, Internet, Client entertainment, Learning, Office supplies, Other
    pub amount: f64,
    pub date: String,
    pub description: String,
    pub receipt_url: Option<String>,
    pub status: String, // "Pending", "Approved", "Paid", "Rejected"
    pub paid_at: Option<String>,
    pub rejection_reason: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateExpenseClaimDto {
    pub employee_id: Option<String>,
    pub employee_name: Option<String>,
    pub category: String,
    pub amount: f64,
    pub date: String,
    pub description: String,
    pub receipt_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RejectExpenseClaimDto {
    pub reason: String,
}

#[derive(Clone, Default)]
pub struct ExpensesStore {
    pub claims: Arc<RwLock<Vec<ExpenseClaim>>>,
}

impl ExpensesStore {
    pub fn new() -> Self {
        let initial_claims = vec![
            ExpenseClaim {
                id: "E-101".to_string(),
                employee_id: "EMP-001".to_string(),
                employee_name: "Ananya Rao".to_string(),
                category: "Travel".to_string(),
                amount: 3450.0,
                date: "2026-10-12".to_string(),
                description: "Client visit cab fare and regional rail".to_string(),
                receipt_url: Some("/receipts/cab-101.pdf".to_string()),
                status: "Approved".to_string(),
                paid_at: None,
                rejection_reason: None,
                created_at: "2026-10-12T09:30:00Z".to_string(),
            },
            ExpenseClaim {
                id: "E-102".to_string(),
                employee_id: "EMP-001".to_string(),
                employee_name: "Ananya Rao".to_string(),
                category: "Internet".to_string(),
                amount: 1499.0,
                date: "2026-10-05".to_string(),
                description: "Monthly broadband reimbursement - WFH".to_string(),
                receipt_url: Some("/receipts/broadband-oct.pdf".to_string()),
                status: "Paid".to_string(),
                paid_at: Some("2026-10-08T14:20:00Z".to_string()),
                rejection_reason: None,
                created_at: "2026-10-05T10:15:00Z".to_string(),
            },
            ExpenseClaim {
                id: "E-103".to_string(),
                employee_id: "EMP-002".to_string(),
                employee_name: "Karthik Nair".to_string(),
                category: "Meals".to_string(),
                amount: 2150.0,
                date: "2026-10-14".to_string(),
                description: "Team sprint completion lunch".to_string(),
                receipt_url: Some("/receipts/team-lunch.pdf".to_string()),
                status: "Pending".to_string(),
                paid_at: None,
                rejection_reason: None,
                created_at: "2026-10-14T15:45:00Z".to_string(),
            },
            ExpenseClaim {
                id: "E-104".to_string(),
                employee_id: "EMP-003".to_string(),
                employee_name: "Priya Sharma".to_string(),
                category: "Learning".to_string(),
                amount: 5000.0,
                date: "2026-10-10".to_string(),
                description: "Cloud Architecture Certification Exam Voucher".to_string(),
                receipt_url: Some("/receipts/exam-fee.pdf".to_string()),
                status: "Pending".to_string(),
                paid_at: None,
                rejection_reason: None,
                created_at: "2026-10-10T11:00:00Z".to_string(),
            },
        ];

        Self {
            claims: Arc::new(RwLock::new(initial_claims)),
        }
    }
}

// Handlers
pub async fn list_claims(State(store): State<ExpensesStore>) -> Json<Vec<ExpenseClaim>> {
    let claims = store.claims.read().await;
    Json(claims.clone())
}

pub async fn create_claim(
    State(store): State<ExpensesStore>,
    Json(dto): Json<CreateExpenseClaimDto>,
) -> (StatusCode, Json<ExpenseClaim>) {
    let mut claims = store.claims.write().await;
    let new_claim = ExpenseClaim {
        id: format!("E-{}", Uuid::new_v4().to_string()[..6].to_uppercase()),
        employee_id: dto.employee_id.unwrap_or_else(|| "EMP-001".to_string()),
        employee_name: dto
            .employee_name
            .unwrap_or_else(|| "Ananya Rao".to_string()),
        category: dto.category,
        amount: dto.amount,
        date: dto.date,
        description: dto.description,
        receipt_url: dto.receipt_url,
        status: "Pending".to_string(),
        paid_at: None,
        rejection_reason: None,
        created_at: Utc::now().to_rfc3339(),
    };
    claims.insert(0, new_claim.clone());
    (StatusCode::CREATED, Json(new_claim))
}

pub async fn approve_claim(
    State(store): State<ExpensesStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut claims = store.claims.write().await;
    if let Some(claim) = claims.iter_mut().find(|c| c.id == id) {
        if claim.status == "Paid" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Paid claims cannot be modified." })),
            );
        }
        claim.status = "Approved".to_string();
        (StatusCode::OK, Json(serde_json::json!(claim)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Claim not found." })),
        )
    }
}

pub async fn reject_claim(
    State(store): State<ExpensesStore>,
    Path(id): Path<String>,
    Json(dto): Json<RejectExpenseClaimDto>,
) -> impl IntoResponse {
    let mut claims = store.claims.write().await;
    if let Some(claim) = claims.iter_mut().find(|c| c.id == id) {
        if claim.status == "Paid" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Paid claims cannot be modified." })),
            );
        }
        claim.status = "Rejected".to_string();
        claim.rejection_reason = Some(dto.reason);
        (StatusCode::OK, Json(serde_json::json!(claim)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Claim not found." })),
        )
    }
}

// EXP-02: Finance pays only after approval and only once. Paid claims become read-only.
pub async fn pay_claim(
    State(store): State<ExpensesStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut claims = store.claims.write().await;
    if let Some(claim) = claims.iter_mut().find(|c| c.id == id) {
        if claim.status == "Paid" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Claim is already paid. Cannot pay twice." })),
            );
        }
        if claim.status != "Approved" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Only approved claims can be disbursed/paid." })),
            );
        }
        claim.status = "Paid".to_string();
        claim.paid_at = Some(Utc::now().to_rfc3339());
        (StatusCode::OK, Json(serde_json::json!(claim)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Claim not found." })),
        )
    }
}
