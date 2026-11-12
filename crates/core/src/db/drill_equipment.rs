//! The `drill_equipment` table: what each drill needs, with quantities.

use super::Database;
use crate::drill::{Drill, Equipment};
use rusqlite::params;
use std::collections::HashMap;

impl Database {
    /// Replace a drill's equipment list (callers run this inside their transaction).
    pub(super) fn set_equipment(
        &self,
        drill_id: i64,
        items: &[(Equipment, u32)],
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM drill_equipment WHERE drill_id = ?1",
            [drill_id],
        )?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO drill_equipment (drill_id, item, qty) VALUES (?1, ?2, ?3)",
        )?;
        for (e, q) in items.iter().filter(|(_, q)| *q > 0) {
            stmt.execute(params![drill_id, e.key(), q])?;
        }
        Ok(())
    }

    /// Fill in `equipment` for each drill.
    pub(super) fn attach_equipment(&self, drills: &mut [Drill]) -> anyhow::Result<()> {
        if drills.is_empty() {
            return Ok(());
        }
        let rows = self.query_all("SELECT drill_id, item, qty FROM drill_equipment", [], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
            ))
        })?;
        let mut by_drill: HashMap<i64, Vec<(Equipment, u32)>> = HashMap::new();
        for (id, item, qty) in rows {
            if let Some(e) = Equipment::from_key(&item) {
                by_drill.entry(id).or_default().push((e, qty));
            }
        }
        for d in drills {
            let mut items = by_drill.remove(&d.id).unwrap_or_default();
            items.sort_by_key(|(e, _)| *e);
            d.equipment = items;
        }
        Ok(())
    }
}
