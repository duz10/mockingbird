//! Dictionary CRUD for the UI.

use serde::Deserialize;
use tauri::State;

use crate::commands::types::DictionaryEntryDto;
use crate::commands::{into_err, lock_db, AppStateHandle};
use crate::db::dictionary;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertEntry {
    /// Optional — when set, updates the row; when None inserts a new one.
    pub id: Option<i64>,
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,
    pub confidence: f64,
    pub app_context: Option<String>,
}

fn to_dto(e: dictionary::DictionaryEntry) -> DictionaryEntryDto {
    DictionaryEntryDto {
        id: e.id,
        term: e.term,
        canonical: e.canonical,
        source: e.source,
        confidence: e.confidence,
        app_context: e.app_context,
        use_count: e.use_count,
        last_used_at: e.last_used_at,
        created_at: e.created_at,
    }
}

#[tauri::command]
pub fn list_dictionary(db: State<'_, AppStateHandle>) -> Result<Vec<DictionaryEntryDto>, String> {
    let conn = lock_db(&db)?;
    let rows = dictionary::list_all(&conn).map_err(into_err)?;
    Ok(rows.into_iter().map(to_dto).collect())
}

#[tauri::command]
pub fn upsert_dictionary_entry(
    db: State<'_, AppStateHandle>,
    entry: UpsertEntry,
) -> Result<i64, String> {
    let conn = lock_db(&db)?;
    if let Some(id) = entry.id {
        dictionary::update(
            &conn,
            id,
            &dictionary::DictionaryEntryUpdate {
                canonical: Some(entry.canonical),
                confidence: Some(entry.confidence),
                app_context: Some(entry.app_context),
            },
        )
        .map_err(into_err)?;
        Ok(id)
    } else {
        dictionary::insert(
            &conn,
            &dictionary::NewDictionaryEntry {
                term: entry.term,
                canonical: entry.canonical,
                source: entry.source,
                confidence: Some(entry.confidence),
                app_context: entry.app_context,
            },
        )
        .map_err(into_err)
    }
}

#[tauri::command]
pub fn delete_dictionary_entry(db: State<'_, AppStateHandle>, id: i64) -> Result<(), String> {
    let conn = lock_db(&db)?;
    dictionary::delete(&conn, id).map_err(into_err)
}
