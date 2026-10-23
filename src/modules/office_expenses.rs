use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OfficeExpense {
    pub id: String,
    pub vendor_name: String,
    pub category: String, // "Office Rent", "Internet & Telecom", "Hardware & IT", "Office Supplies", "Electricity & Utilities"
    pub invoice_no: String,
    pub invoice_date: String,
    pub base_amount: f64,
    pub gst_rate: f64, // e.g. 18.0 for 18%
    pub gst_amount: f64,
    pub total_amount: f64,
    pub status: String, // "Paid", "Pending"
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateOfficeExpenseDto {
    pub vendor_name: String,
    pub category: String,
    pub invoice_no: String,
    pub invoice_date: String,
    pub base_amount: f64,
    pub gst_rate: f64,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OfficeExpenseAnalytics {
    pub total_spend: f64,
    pub total_gst_credit: f64,
    pub total_base: f64,
    pub pending_invoices_amount: f64,
    pub by_category: HashMap<String, f64>,
}

#[derive(Clone, Default)]
pub struct OfficeExpensesStore {
    pub expenses: Arc<RwLock<Vec<OfficeExpense>>>,
}

impl OfficeExpensesStore {
    pub fn new() -> Self {
        let initial_expenses = vec![
            OfficeExpense {
                id: "OE-1".to_string(),
                vendor_name: "WeWork Cyber City".to_string(),
                category: "Office Rent".to_string(),
                invoice_no: "WW-2026-10-892".to_string(),
                invoice_date: "2026-10-01".to_string(),
                base_amount: 450000.0,
                gst_rate: 18.0,
                gst_amount: 81000.0,
                total_amount: 531000.0,
                status: "Paid".to_string(),
            },
            OfficeExpense {
                id: "OE-2".to_string(),
                vendor_name: "Airtel Business Enterprise".to_string(),
                category: "Internet & Telecom".to_string(),
                invoice_no: "AB-TEL-9981".to_string(),
                invoice_date: "2026-10-04".to_string(),
                base_amount: 32000.0,
                gst_rate: 18.0,
                gst_amount: 5760.0,
                total_amount: 37760.0,
                status: "Paid".to_string(),
            },
            OfficeExpense {
                id: "OE-3".to_string(),
                vendor_name: "Dell Technologies India".to_string(),
                category: "Hardware & IT".to_string(),
                invoice_no: "DEL-IN-2026-44".to_string(),
                invoice_date: "2026-10-11".to_string(),
                base_amount: 280000.0,
                gst_rate: 18.0,
                gst_amount: 50400.0,
                total_amount: 330400.0,
                status: "Pending".to_string(),
            },
            OfficeExpense {
                id: "OE-4".to_string(),
                vendor_name: "Amazon Business".to_string(),
                category: "Office Supplies".to_string(),
                invoice_no: "AMZ-SUP-112".to_string(),
                invoice_date: "2026-10-13".to_string(),
                base_amount: 14500.0,
                gst_rate: 12.0,
                gst_amount: 1740.0,
                total_amount: 16240.0,
                status: "Pending".to_string(),
            },
        ];

        Self {
            expenses: Arc::new(RwLock::new(initial_expenses)),
        }
    }
}

pub async fn list_office_expenses(
    State(store): State<OfficeExpensesStore>,
) -> Json<Vec<OfficeExpense>> {
    let items = store.expenses.read().await;
    Json(items.clone())
}

pub async fn create_office_expense(
    State(store): State<OfficeExpensesStore>,
    Json(dto): Json<CreateOfficeExpenseDto>,
) -> (StatusCode, Json<OfficeExpense>) {
    let mut items = store.expenses.write().await;
    let gst_amount = (dto.base_amount * dto.gst_rate / 100.0).round();
    let total_amount = dto.base_amount + gst_amount;

    let new_expense = OfficeExpense {
        id: format!("OE-{}", Uuid::new_v4().to_string()[..5].to_uppercase()),
        vendor_name: dto.vendor_name,
        category: dto.category,
        invoice_no: dto.invoice_no,
        invoice_date: dto.invoice_date,
        base_amount: dto.base_amount,
        gst_rate: dto.gst_rate,
        gst_amount,
        total_amount,
        status: dto.status.unwrap_or_else(|| "Pending".to_string()),
    };

    items.insert(0, new_expense.clone());
    (StatusCode::CREATED, Json(new_expense))
}

pub async fn get_analytics(
    State(store): State<OfficeExpensesStore>,
) -> Json<OfficeExpenseAnalytics> {
    let items = store.expenses.read().await;
    let mut total_spend = 0.0;
    let mut total_gst_credit = 0.0;
    let mut total_base = 0.0;
    let mut pending_invoices_amount = 0.0;
    let mut by_category: HashMap<String, f64> = HashMap::new();

    for exp in items.iter() {
        total_spend += exp.total_amount;
        total_gst_credit += exp.gst_amount;
        total_base += exp.base_amount;
        if exp.status == "Pending" {
            pending_invoices_amount += exp.total_amount;
        }
        *by_category.entry(exp.category.clone()).or_insert(0.0) += exp.total_amount;
    }

    Json(OfficeExpenseAnalytics {
        total_spend,
        total_gst_credit,
        total_base,
        pending_invoices_amount,
        by_category,
    })
}
