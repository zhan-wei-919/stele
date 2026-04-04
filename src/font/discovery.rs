use std::collections::HashMap;

use fontdb::{Database, Family, Query, ID};

#[derive(Debug)]
pub enum FontDiscoveryError {
    NoSystemFonts,
}

pub struct FontDiscovery {
    database: Database,
    app_to_db: Vec<ID>,
    db_to_app: HashMap<ID, u32>,
    default_font_id: u32,
}

impl FontDiscovery {
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
            db_to_app,
            default_font_id,
        })
    }

    pub fn find_font_id(&self, family_name: &str) -> u32 {
        self.find_font_id_opt(family_name)
            .unwrap_or(self.default_font_id)
    }

    pub fn find_font_id_opt(&self, family_name: &str) -> Option<u32> {
        let families = [Family::Name(family_name)];
        self.database
            .query(&Query {
                families: &families,
                ..Query::default()
            })
            .and_then(|db_id| self.db_to_app.get(&db_id).copied())
    }

    pub fn resolve_font_id(&self, family_name: Option<&str>) -> u32 {
        family_name
            .and_then(|name| self.find_font_id_opt(name))
            .unwrap_or(self.default_font_id)
    }

    pub fn default_font_id(&self) -> u32 {
        self.default_font_id
    }

    pub fn db_id_for(&self, font_id: u32) -> Option<ID> {
        self.app_to_db.get(font_id as usize).copied()
    }

    pub fn face_info(&self, font_id: u32) -> Option<&fontdb::FaceInfo> {
        self.db_id_for(font_id)
            .and_then(|db_id| self.database.face(db_id))
    }

    pub fn database(&self) -> &Database {
        &self.database
    }
}
