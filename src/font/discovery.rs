//! Font discovery and stable application-local font identifiers.

use std::collections::HashMap;

use fontdb::{Database, Family, Query, ID};

/// Errors produced while initializing the system font database.
#[derive(Debug)]
pub enum FontDiscoveryError {
    /// No fonts were discovered on the current system.
    NoSystemFonts,
}

/// Maps fontdb face identifiers into compact IDs used by the renderer.
pub struct FontDiscovery {
    database: Database,
    app_to_db: Vec<ID>,
    default_font_id: u32,
}

impl FontDiscovery {
    /// Loads system fonts and prepares a stable application-local ID mapping.
    pub fn new() -> Result<Self, FontDiscoveryError> {
        let mut database = Database::new();
        database.load_system_fonts();

        let app_to_db: Vec<ID> = database.faces().map(|face| face.id).collect();
        if app_to_db.is_empty() {
            return Err(FontDiscoveryError::NoSystemFonts);
        }

        let db_to_app = app_to_db
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index as u32))
            .collect::<HashMap<_, _>>();

        let default_face = database
            .query(&Query {
                families: &[Family::SansSerif],
                ..Query::default()
            })
            .or_else(|| app_to_db.first().copied())
            .ok_or(FontDiscoveryError::NoSystemFonts)?;

        let default_font_id = *db_to_app
            .get(&default_face)
            .ok_or(FontDiscoveryError::NoSystemFonts)?;

        Ok(Self {
            database,
            app_to_db,
            default_font_id,
        })
    }

    /// Returns the default sans-serif font chosen for the current system.
    pub fn default_font_id(&self) -> u32 {
        self.default_font_id
    }

    /// Resolves an application-local font ID back to the originating fontdb face.
    pub fn face_info(&self, font_id: u32) -> Option<&fontdb::FaceInfo> {
        self.app_to_db
            .get(font_id as usize)
            .copied()
            .and_then(|db_id| self.database.face(db_id))
    }
}
