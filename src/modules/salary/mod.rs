pub mod calculator;
pub mod types;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use calculator::compute_salary_in_paise;
pub use types::*;

#[derive(Clone)]
pub struct SalaryStore {
    pub structures: Arc<RwLock<Vec<SalaryStructure>>>,
    pub revisions: Arc<RwLock<Vec<SalaryRevision>>>,
    pub increment_history: Arc<RwLock<Vec<IncrementHistoryRecord>>>,
}

impl SalaryStore {
    pub fn new() -> Self {
        let s1 = compute_salary_in_paise(85000.0, true, "new");
        let s2 = compute_salary_in_paise(120000.0, true, "new");
        let s3 = compute_salary_in_paise(65000.0, true, "old");

        let initial_structures = vec![
            SalaryStructure {
                id: "STR-001".to_string(),
                employee_id: "EMP-001".to_string(),
                employee_name: "Ananya Rao".to_string(),
                department: "People Ops".to_string(),
                effective_from: "2026-04-01".to_string(),
                gross_monthly: s1.gross_monthly_rupees,
                basic: s1.basic_monthly_rupees,
                hra: s1.hra_monthly_rupees,
                special_allowance: s1.special_allowance_rupees,
                employee_pf: s1.employee_pf_rupees,
                professional_tax: s1.professional_tax_rupees,
                estimated_tds: s1.estimated_tds_rupees,
                net_take_home: s1.net_take_home_rupees,
                annual_ctc: s1.annual_ctc_rupees,
                pf_opt_out: false,
                tax_regime: "New".to_string(),
                is_active: true,
                superseded_by: None,
                created_at: "2026-04-01T00:00:00Z".to_string(),
            },
            SalaryStructure {
                id: "STR-002".to_string(),
                employee_id: "EMP-002".to_string(),
                employee_name: "Karthik Nair".to_string(),
                department: "Engineering".to_string(),
                effective_from: "2026-04-01".to_string(),
                gross_monthly: s2.gross_monthly_rupees,
                basic: s2.basic_monthly_rupees,
                hra: s2.hra_monthly_rupees,
                special_allowance: s2.special_allowance_rupees,
                employee_pf: s2.employee_pf_rupees,
                professional_tax: s2.professional_tax_rupees,
                estimated_tds: s2.estimated_tds_rupees,
                net_take_home: s2.net_take_home_rupees,
                annual_ctc: s2.annual_ctc_rupees,
                pf_opt_out: false,
                tax_regime: "New".to_string(),
                is_active: true,
                superseded_by: None,
                created_at: "2026-04-01T00:00:00Z".to_string(),
            },
            SalaryStructure {
                id: "STR-003".to_string(),
                employee_id: "EMP-003".to_string(),
                employee_name: "Priya Sharma".to_string(),
                department: "Product & Design".to_string(),
                effective_from: "2026-04-01".to_string(),
                gross_monthly: s3.gross_monthly_rupees,
                basic: s3.basic_monthly_rupees,
                hra: s3.hra_monthly_rupees,
                special_allowance: s3.special_allowance_rupees,
                employee_pf: s3.employee_pf_rupees,
                professional_tax: s3.professional_tax_rupees,
                estimated_tds: s3.estimated_tds_rupees,
                net_take_home: s3.net_take_home_rupees,
                annual_ctc: s3.annual_ctc_rupees,
                pf_opt_out: false,
                tax_regime: "Old".to_string(),
                is_active: true,
                superseded_by: None,
                created_at: "2026-04-01T00:00:00Z".to_string(),
            },
        ];

        let initial_revisions = vec![
            SalaryRevision {
                id: "REV-101".to_string(),
                employee_id: "EMP-001".to_string(),
                employee_name: "Ananya Rao".to_string(),
                current_gross: 85000.0,
                proposed_gross: 100000.0,
                increment_percent: 17.65,
                effective_date: "2026-11-01".to_string(),
                reason: "Annual Appraisal".to_string(),
                notes: "Exceeded Q2 & Q3 organizational hiring and portal delivery benchmarks."
                    .to_string(),
                status: "Under Review".to_string(),
                approved_by: None,
                approved_at: None,
                created_at: "2026-10-10T11:00:00Z".to_string(),
            },
            SalaryRevision {
                id: "REV-102".to_string(),
                employee_id: "EMP-002".to_string(),
                employee_name: "Karthik Nair".to_string(),
                current_gross: 120000.0,
                proposed_gross: 140000.0,
                increment_percent: 16.67,
                effective_date: "2026-11-01".to_string(),
                reason: "Promotion".to_string(),
                notes: "Promotion to Principal Rust Engineer & Platform Architect.".to_string(),
                status: "Approved".to_string(),
                approved_by: Some("Siddharth Patel (CEO)".to_string()),
                approved_at: Some("2026-10-14T16:00:00Z".to_string()),
                created_at: "2026-10-05T09:30:00Z".to_string(),
            },
        ];

        let initial_history = vec![
            IncrementHistoryRecord {
                id: "INC-01".to_string(),
                employee_id: "EMP-001".to_string(),
                employee_name: "Ananya Rao".to_string(),
                previous_gross: 72000.0,
                revised_gross: 85000.0,
                increment_amount: 13000.0,
                increment_percentage: 18.06,
                effective_date: "2026-04-01".to_string(),
                change_type: "Annual Appraisal".to_string(),
                approved_by: "HR Compensation Committee".to_string(),
                approved_at: "2026-03-25T14:00:00Z".to_string(),
                notes: "FY26 merit-based appraisal cycle.".to_string(),
            },
            IncrementHistoryRecord {
                id: "INC-02".to_string(),
                employee_id: "EMP-002".to_string(),
                employee_name: "Karthik Nair".to_string(),
                previous_gross: 100000.0,
                revised_gross: 120000.0,
                increment_amount: 20000.0,
                increment_percentage: 20.0,
                effective_date: "2026-04-01".to_string(),
                change_type: "Promotion".to_string(),
                approved_by: "Engineering VP".to_string(),
                approved_at: "2026-03-20T10:00:00Z".to_string(),
                notes: "Promotion to Senior Tech Lead.".to_string(),
            },
            IncrementHistoryRecord {
                id: "INC-03".to_string(),
                employee_id: "EMP-003".to_string(),
                employee_name: "Priya Sharma".to_string(),
                previous_gross: 55000.0,
                revised_gross: 65000.0,
                increment_amount: 10000.0,
                increment_percentage: 18.18,
                effective_date: "2026-04-01".to_string(),
                change_type: "Annual Appraisal".to_string(),
                approved_by: "HR Committee".to_string(),
                approved_at: "2026-03-24T12:00:00Z".to_string(),
                notes: "Exceeded design system unification milestones.".to_string(),
            },
        ];

        Self {
            structures: Arc::new(RwLock::new(initial_structures)),
            revisions: Arc::new(RwLock::new(initial_revisions)),
            increment_history: Arc::new(RwLock::new(initial_history)),
        }
    }
}

impl Default for SalaryStore {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn calculate_salary(Json(dto): Json<CalculateSalaryDto>) -> Json<SalaryBreakdown> {
    let pf_capped = dto.pf_capped.unwrap_or(true);
    let regime = dto.tax_regime.unwrap_or_else(|| "new".to_string());
    let breakdown = compute_salary_in_paise(dto.gross_monthly, pf_capped, &regime);
    Json(breakdown)
}

pub async fn list_structures(State(store): State<SalaryStore>) -> Json<Vec<SalaryStructure>> {
    let list = store.structures.read().await;
    Json(list.clone())
}

pub async fn create_structure(
    State(store): State<SalaryStore>,
    Json(dto): Json<CreateSalaryStructureDto>,
) -> (StatusCode, Json<SalaryStructure>) {
    let mut structures = store.structures.write().await;
    let pf_capped = !dto.pf_opt_out.unwrap_or(false);
    let regime = dto.tax_regime.clone().unwrap_or_else(|| "New".to_string());
    let calc = compute_salary_in_paise(dto.gross_monthly, pf_capped, &regime);

    let new_id = format!("STR-{}", Uuid::new_v4().to_string()[..6].to_uppercase());

    for s in structures.iter_mut() {
        if s.employee_id == dto.employee_id && s.is_active {
            s.is_active = false;
            s.superseded_by = Some(new_id.clone());
        }
    }

    let new_structure = SalaryStructure {
        id: new_id,
        employee_id: dto.employee_id,
        employee_name: dto.employee_name,
        department: dto.department,
        effective_from: dto.effective_from,
        gross_monthly: calc.gross_monthly_rupees,
        basic: calc.basic_monthly_rupees,
        hra: calc.hra_monthly_rupees,
        special_allowance: calc.special_allowance_rupees,
        employee_pf: calc.employee_pf_rupees,
        professional_tax: calc.professional_tax_rupees,
        estimated_tds: calc.estimated_tds_rupees,
        net_take_home: calc.net_take_home_rupees,
        annual_ctc: calc.annual_ctc_rupees,
        pf_opt_out: dto.pf_opt_out.unwrap_or(false),
        tax_regime: regime,
        is_active: true,
        superseded_by: None,
        created_at: Utc::now().to_rfc3339(),
    };

    structures.insert(0, new_structure.clone());
    (StatusCode::CREATED, Json(new_structure))
}

pub async fn list_revisions(State(store): State<SalaryStore>) -> Json<Vec<SalaryRevision>> {
    let list = store.revisions.read().await;
    Json(list.clone())
}

pub async fn propose_revision(
    State(store): State<SalaryStore>,
    Json(dto): Json<ProposeRevisionDto>,
) -> (StatusCode, Json<SalaryRevision>) {
    let mut revisions = store.revisions.write().await;
    let increment_amount = dto.proposed_gross - dto.current_gross;
    let increment_percent = if dto.current_gross > 0.0 {
        ((increment_amount / dto.current_gross) * 10000.0).round() / 100.0
    } else {
        0.0
    };

    let new_rev = SalaryRevision {
        id: format!("REV-{}", Uuid::new_v4().to_string()[..5].to_uppercase()),
        employee_id: dto.employee_id,
        employee_name: dto.employee_name,
        current_gross: dto.current_gross,
        proposed_gross: dto.proposed_gross,
        increment_percent,
        effective_date: dto.effective_date,
        reason: dto.reason,
        notes: dto.notes,
        status: "Under Review".to_string(),
        approved_by: None,
        approved_at: None,
        created_at: Utc::now().to_rfc3339(),
    };

    revisions.insert(0, new_rev.clone());
    (StatusCode::CREATED, Json(new_rev))
}

pub async fn approve_revision(
    State(store): State<SalaryStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut revisions = store.revisions.write().await;
    let rev_idx = match revisions.iter().position(|r| r.id == id) {
        Some(idx) => idx,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Revision not found." })),
            )
        }
    };

    let rev = &mut revisions[rev_idx];
    if rev.status == "Approved" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Revision already approved." })),
        );
    }

    rev.status = "Approved".to_string();
    rev.approved_by = Some("HR Compensation Committee".to_string());
    rev.approved_at = Some(Utc::now().to_rfc3339());

    let emp_id = rev.employee_id.clone();
    let emp_name = rev.employee_name.clone();
    let prev_gross = rev.current_gross;
    let rev_gross = rev.proposed_gross;
    let inc_amt = rev_gross - prev_gross;
    let inc_pct = rev.increment_percent;
    let eff_date = rev.effective_date.clone();
    let reason = rev.reason.clone();
    let notes = rev.notes.clone();

    let mut history = store.increment_history.write().await;
    let history_entry = IncrementHistoryRecord {
        id: format!("INC-{}", Uuid::new_v4().to_string()[..5].to_uppercase()),
        employee_id: emp_id.clone(),
        employee_name: emp_name.clone(),
        previous_gross: prev_gross,
        revised_gross: rev_gross,
        increment_amount: inc_amt,
        increment_percentage: inc_pct,
        effective_date: eff_date.clone(),
        change_type: reason,
        approved_by: "HR Compensation Committee".to_string(),
        approved_at: Utc::now().to_rfc3339(),
        notes,
    };
    history.insert(0, history_entry);

    let mut structures = store.structures.write().await;
    let new_struct_id = format!("STR-{}", Uuid::new_v4().to_string()[..6].to_uppercase());

    let mut department = "Engineering".to_string();
    for s in structures.iter_mut() {
        if s.employee_id == emp_id && s.is_active {
            s.is_active = false;
            s.superseded_by = Some(new_struct_id.clone());
            department = s.department.clone();
        }
    }

    let calc = compute_salary_in_paise(rev_gross, true, "New");
    let new_structure = SalaryStructure {
        id: new_struct_id,
        employee_id: emp_id,
        employee_name: emp_name,
        department,
        effective_from: eff_date,
        gross_monthly: calc.gross_monthly_rupees,
        basic: calc.basic_monthly_rupees,
        hra: calc.hra_monthly_rupees,
        special_allowance: calc.special_allowance_rupees,
        employee_pf: calc.employee_pf_rupees,
        professional_tax: calc.professional_tax_rupees,
        estimated_tds: calc.estimated_tds_rupees,
        net_take_home: calc.net_take_home_rupees,
        annual_ctc: calc.annual_ctc_rupees,
        pf_opt_out: false,
        tax_regime: "New".to_string(),
        is_active: true,
        superseded_by: None,
        created_at: Utc::now().to_rfc3339(),
    };
    structures.insert(0, new_structure);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Salary revision approved. New active structure created and increment history updated.",
            "revision": revisions[rev_idx]
        })),
    )
}

pub async fn list_increment_history(
    State(store): State<SalaryStore>,
) -> Json<Vec<IncrementHistoryRecord>> {
    let list = store.increment_history.read().await;
    Json(list.clone())
}
