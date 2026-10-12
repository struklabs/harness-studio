//! Library user state (`model_state`, schema v2): favorites, last use and arrival time of
//! local model files. Keyed by the normalized launch path (`library::state_key`).

use super::{now_ms, StateDb, StateResult};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelState {
    pub model_id: String,
    pub favorite: bool,
    /// Unix ms of the last start of a server with this file.
    pub last_used_at: Option<i64>,
    /// Definition that started it last.
    pub last_definition_id: Option<String>,
    /// That definition's default inference preset at the time.
    pub last_preset_id: Option<String>,
    /// Unix ms the file arrived (download completion / copy time; first seen otherwise).
    pub added_at: Option<i64>,
}

/// Most recent conversation (by `updated_at`) that talked to a served model name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasUse {
    pub preset_id: Option<String>,
    pub updated_at: i64,
}

fn row_to_state(r: &Row<'_>) -> rusqlite::Result<ModelState> {
    Ok(ModelState {
        model_id: r.get(0)?,
        favorite: r.get::<_, i64>(1)? != 0,
        last_used_at: r.get(2)?,
        last_definition_id: r.get(3)?,
        last_preset_id: r.get(4)?,
        added_at: r.get(5)?,
    })
}

const COLUMNS: &str =
    "model_id, favorite, last_used_at, last_definition_id, last_preset_id, added_at";

impl StateDb {
    /// Every stored row.
    pub fn model_states(&self) -> StateResult<Vec<ModelState>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM model_state"))?;
            let rows = stmt
                .query_map([], row_to_state)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn model_state(&self, model_id: &str) -> StateResult<Option<ModelState>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM model_state WHERE model_id = ?1"),
                [model_id],
                row_to_state,
            )
            .optional()?)
        })
    }

    pub fn set_model_favorite(&self, model_id: &str, favorite: bool) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "INSERT INTO model_state(model_id, favorite) VALUES (?1, ?2)
                 ON CONFLICT(model_id) DO UPDATE SET favorite = excluded.favorite",
                params![model_id, favorite as i64],
            )?;
            Ok(())
        })
    }

    /// A server was started with this file (by `definition_id`, using `preset_id` by default).
    pub fn record_model_use(
        &self,
        model_id: &str,
        definition_id: Option<&str>,
        preset_id: Option<&str>,
    ) -> StateResult<()> {
        self.record_model_use_at(model_id, definition_id, preset_id, now_ms())
    }

    pub(crate) fn record_model_use_at(
        &self,
        model_id: &str,
        definition_id: Option<&str>,
        preset_id: Option<&str>,
        at: i64,
    ) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "INSERT INTO model_state(model_id, last_used_at, last_definition_id, last_preset_id)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(model_id) DO UPDATE SET
                    last_used_at = excluded.last_used_at,
                    last_definition_id = excluded.last_definition_id,
                    last_preset_id = excluded.last_preset_id",
                params![model_id, at, definition_id, preset_id],
            )?;
            Ok(())
        })
    }

    /// Stores when files arrived, for rows that don't have it yet (first value wins).
    pub fn note_models_added(&self, added: &[(String, i64)]) -> StateResult<()> {
        if added.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO model_state(model_id, added_at) VALUES (?1, ?2)
                 ON CONFLICT(model_id) DO UPDATE SET added_at = excluded.added_at
                 WHERE model_state.added_at IS NULL",
            )?;
            for (id, at) in added {
                stmt.execute(params![id, at])?;
            }
            Ok(())
        })
    }

    /// The file moved (`migrate_model`): its state follows it. An existing row at the new id
    /// is replaced.
    pub fn rename_model_state(&self, old_id: &str, new_id: &str) -> StateResult<()> {
        if old_id == new_id {
            return Ok(());
        }
        self.write(|tx| {
            tx.execute(
                "UPDATE OR REPLACE model_state SET model_id = ?2 WHERE model_id = ?1",
                params![old_id, new_id],
            )?;
            Ok(())
        })
    }

    /// The file was deleted.
    pub fn forget_model_state(&self, model_id: &str) -> StateResult<()> {
        self.write(|tx| {
            tx.execute("DELETE FROM model_state WHERE model_id = ?1", [model_id])?;
            Ok(())
        })
    }

    /// Latest conversation per served model name (lower-cased alias): which preset it used.
    pub fn latest_alias_uses(&self) -> StateResult<HashMap<String, AliasUse>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT lower(model_alias), preset_id, updated_at FROM conversations
                 WHERE model_alias IS NOT NULL AND model_alias <> ''
                 ORDER BY updated_at ASC",
            )?;
            let mut out = HashMap::new();
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for row in rows {
                let (alias, preset_id, updated_at) = row?;
                out.insert(
                    alias,
                    AliasUse {
                        preset_id,
                        updated_at,
                    },
                );
            }
            Ok(out)
        })
    }
}
