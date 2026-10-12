//! Measured memory footprints (`vram_measurements`, schema v3). The rows are written after a
//! successful load and read by the estimate in [`crate::vram`].

use super::{StateDb, StateResult};
use rusqlite::params;

/// Rows kept per model file (the oldest are dropped).
pub const MAX_PER_MODEL: i64 = 16;

/// One stored row: `data_json` is opaque here (serialized `vram::Measurement`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasurementRow {
    pub model_key: String,
    pub settings_key: String,
    pub model_size: u64,
    pub measured_at: i64,
    pub data_json: String,
}

impl StateDb {
    /// Inserts or replaces the measurement of (model, settings) and trims old rows.
    pub fn put_vram_measurement(&self, row: &MeasurementRow) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "INSERT INTO vram_measurements(model_key, settings_key, model_size, measured_at, data_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(model_key, settings_key) DO UPDATE SET
                    model_size = excluded.model_size,
                    measured_at = excluded.measured_at,
                    data_json = excluded.data_json",
                params![
                    row.model_key,
                    row.settings_key,
                    row.model_size as i64,
                    row.measured_at,
                    row.data_json
                ],
            )?;
            tx.execute(
                "DELETE FROM vram_measurements WHERE model_key = ?1 AND settings_key NOT IN (
                    SELECT settings_key FROM vram_measurements WHERE model_key = ?1
                    ORDER BY measured_at DESC LIMIT ?2)",
                params![row.model_key, MAX_PER_MODEL],
            )?;
            Ok(())
        })
    }

    /// Every stored measurement, newest first.
    pub fn vram_measurements(&self) -> StateResult<Vec<MeasurementRow>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT model_key, settings_key, model_size, measured_at, data_json
                 FROM vram_measurements ORDER BY measured_at DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(MeasurementRow {
                        model_key: r.get(0)?,
                        settings_key: r.get(1)?,
                        model_size: r.get::<_, i64>(2)?.max(0) as u64,
                        measured_at: r.get(3)?,
                        data_json: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Drops the measurements of a model file (deleted or replaced).
    pub fn forget_vram_measurements(&self, model_key: &str) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM vram_measurements WHERE model_key = ?1",
                [model_key],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(settings: &str, at: i64) -> MeasurementRow {
        MeasurementRow {
            model_key: "c:/m/a.gguf".into(),
            settings_key: settings.into(),
            model_size: 10,
            measured_at: at,
            data_json: "{}".into(),
        }
    }

    #[test]
    fn upsert_and_trim() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        for i in 0..(MAX_PER_MODEL + 3) {
            db.put_vram_measurement(&row(&format!("s{i}"), i)).unwrap();
        }
        // Same key again: replaced, not duplicated.
        db.put_vram_measurement(&row("s18", 1000)).unwrap();
        let rows = db.vram_measurements().unwrap();
        assert_eq!(rows.len() as i64, MAX_PER_MODEL);
        assert_eq!(rows[0].settings_key, "s18");
        assert_eq!(rows[0].measured_at, 1000);
        assert!(!rows.iter().any(|r| r.settings_key == "s0"));
        db.forget_vram_measurements("c:/m/a.gguf").unwrap();
        assert!(db.vram_measurements().unwrap().is_empty());
    }
}
