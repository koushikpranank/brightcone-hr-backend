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
pub struct Announcement {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub author_name: String,
    pub date: String,
    pub category: String, // "Company News", "Policy", "Townhall", "Celebration"
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateAnnouncementDto {
    pub title: String,
    pub body: String,
    pub pinned: Option<bool>,
    pub author_name: Option<String>,
    pub category: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateAnnouncementDto {
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub category: Option<String>,
}

#[derive(Clone, Default)]
pub struct AnnouncementsStore {
    pub announcements: Arc<RwLock<Vec<Announcement>>>,
}

impl AnnouncementsStore {
    pub fn new() -> Self {
        let initial_items = vec![
            Announcement {
                id: "A-1".to_string(),
                title: "Brightcone Q3 Town Hall & Performance Reviews".to_string(),
                body: "Join us this Friday at 4:00 PM IST on Zoom for the quarterly company all-hands. Executive updates on client expansions, followed by Q&A with leadership.".to_string(),
                pinned: true,
                author_name: "Ananya Rao (HR Admin)".to_string(),
                date: "2026-10-14".to_string(),
                category: "Townhall".to_string(),
            },
            Announcement {
                id: "A-2".to_string(),
                title: "Updated WFH & Broadband Reimbursement Policy 2026".to_string(),
                body: "Starting this month, broadband reimbursement limit has been raised to Rs 1,500/month. Claims must be submitted via the Expenses portal before the 15th of each month.".to_string(),
                pinned: true,
                author_name: "Finance & People Team".to_string(),
                date: "2026-10-10".to_string(),
                category: "Policy".to_string(),
            },
            Announcement {
                id: "A-3".to_string(),
                title: "Welcome 14 New Team Members to Engineering & AI Teams".to_string(),
                body: "Please give a warm welcome to our October cohort joining Hyderabad and Bengaluru engineering campuses today! Check the employee directory to connect.".to_string(),
                pinned: false,
                author_name: "People Ops".to_string(),
                date: "2026-10-06".to_string(),
                category: "Company News".to_string(),
            },
        ];

        Self {
            announcements: Arc::new(RwLock::new(initial_items)),
        }
    }
}

pub async fn list_announcements(
    State(store): State<AnnouncementsStore>,
) -> Json<Vec<Announcement>> {
    let items = store.announcements.read().await;
    let mut sorted = items.clone();
    // Pinned first, then by date descending
    sorted.sort_by(|a, b| match (b.pinned, a.pinned) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => b.date.cmp(&a.date),
    });
    Json(sorted)
}

pub async fn create_announcement(
    State(store): State<AnnouncementsStore>,
    Json(dto): Json<CreateAnnouncementDto>,
) -> (StatusCode, Json<Announcement>) {
    let mut items = store.announcements.write().await;
    let new_item = Announcement {
        id: format!("A-{}", Uuid::new_v4().to_string()[..5].to_uppercase()),
        title: dto.title,
        body: dto.body,
        pinned: dto.pinned.unwrap_or(false),
        author_name: dto.author_name.unwrap_or_else(|| "HR Admin".to_string()),
        date: Utc::now().format("%Y-%m-%d").to_string(),
        category: dto.category.unwrap_or_else(|| "Company News".to_string()),
    };
    items.insert(0, new_item.clone());
    (StatusCode::CREATED, Json(new_item))
}

pub async fn update_announcement(
    State(store): State<AnnouncementsStore>,
    Path(id): Path<String>,
    Json(dto): Json<UpdateAnnouncementDto>,
) -> impl IntoResponse {
    let mut items = store.announcements.write().await;
    if let Some(item) = items.iter_mut().find(|a| a.id == id) {
        item.title = dto.title;
        item.body = dto.body;
        item.pinned = dto.pinned;
        if let Some(cat) = dto.category {
            item.category = cat;
        }
        (StatusCode::OK, Json(serde_json::json!(item)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Announcement not found." })),
        )
    }
}

pub async fn toggle_pin(
    State(store): State<AnnouncementsStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut items = store.announcements.write().await;
    if let Some(item) = items.iter_mut().find(|a| a.id == id) {
        item.pinned = !item.pinned;
        (StatusCode::OK, Json(serde_json::json!(item)))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Announcement not found." })),
        )
    }
}

pub async fn delete_announcement(
    State(store): State<AnnouncementsStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut items = store.announcements.write().await;
    let initial_len = items.len();
    items.retain(|a| a.id != id);
    if items.len() < initial_len {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
