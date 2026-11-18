//! Settings and schema-cache discovery for the language server.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use banshee_config::BansheeConfig;
use banshee_lsp::DocumentSettings;

use super::{Cli, exit, schema};

pub fn run(cli: &Cli) -> anyhow::Result<u8> {
    if let Some(path) = &cli.config {
        let config = BansheeConfig::load(path)
            .with_context(|| format!("could not load {}", path.display()))?;
        let schema = schema::resolve(&config)?;
        banshee_lsp::run_stdio(config, schema)?;
    } else {
        let mut settings = ProjectSettings::new(schema::base_dir());
        banshee_lsp::run_stdio_with_settings(move |file| settings.load(file))?;
    }
    Ok(exit::OK)
}

struct LoadedSettings {
    text: String,
    settings: Arc<DocumentSettings>,
}

struct ProjectSettings {
    launch_dir: PathBuf,
    defaults: Arc<DocumentSettings>,
    configs: HashMap<PathBuf, LoadedSettings>,
}

impl ProjectSettings {
    fn new(launch_dir: PathBuf) -> Self {
        Self {
            launch_dir,
            defaults: Arc::new(DocumentSettings::default()),
            configs: HashMap::new(),
        }
    }

    fn load(&mut self, file: Option<&Path>) -> anyhow::Result<Arc<DocumentSettings>> {
        let dir = file.and_then(Path::parent).unwrap_or(&self.launch_dir);
        let Some(path) = BansheeConfig::find_config_file(dir) else {
            return Ok(Arc::clone(&self.defaults));
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        if let Some(loaded) = self.configs.get(&path).filter(|loaded| loaded.text == text) {
            return Ok(Arc::clone(&loaded.settings));
        }
        let config = BansheeConfig::from_toml(&text)
            .with_context(|| format!("could not load {}", path.display()))?;
        let project_dir = path.parent().unwrap_or(dir);
        let schema = match schema::resolve_at(&config, project_dir) {
            Ok(schema) => schema,
            Err(_) => {
                eprintln!(
                    "warning: could not load the database schema for {}",
                    path.display()
                );
                None
            }
        };
        let settings = Arc::new(DocumentSettings::from_cache(config, schema));
        self.configs.insert(
            path,
            LoadedSettings {
                text,
                settings: Arc::clone(&settings),
            },
        );
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_schema::{SchemaSnapshot, TableEntry, TableKind, cache};

    #[test]
    fn document_settings_follow_the_nearest_config_and_its_cache() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let nested = dir.path().join("nested space");
        std::fs::create_dir(&nested)?;
        let root = dir.path().join("banshee.toml");
        let child = nested.join("banshee.toml");
        std::fs::write(&root, "[format]\nkeyword-case = 'upper'\n")?;
        let child_text = "[format]\nkeyword-case = 'lower'\n[database]\nurl = 'postgresql://localhost/fixture'\nschema = 'app'\n";
        std::fs::write(&child, child_text)?;
        let fingerprint = schema::config_fingerprint(&BansheeConfig::from_toml(child_text)?)
            .expect("configured database");
        let mut snapshot = SchemaSnapshot::new(fingerprint, "app");
        snapshot.tables.push(TableEntry {
            schema: "app".into(),
            name: "project_table".into(),
            kind: TableKind::Table,
            columns: Vec::new(),
        });
        cache::save(&nested, &snapshot)?;

        let mut resolver = ProjectSettings::new(dir.path().to_owned());
        let root_settings = resolver.load(None)?;
        assert!(
            root_settings
                .config
                .format("select 1;")
                .starts_with("SELECT")
        );
        assert!(root_settings.schema.is_none());

        let new_document = nested.join("new file.sql");
        let child_settings = resolver.load(Some(&new_document))?;
        assert!(
            child_settings
                .config
                .format("SELECT 1;")
                .starts_with("select")
        );
        assert_eq!(
            child_settings
                .schema
                .as_ref()
                .and_then(|schema| schema.lookup_table(None, "project_table"))
                .and_then(|table| table.schema),
            Some("app".to_owned())
        );
        assert!(Arc::ptr_eq(
            &child_settings,
            &resolver.load(Some(&new_document))?
        ));

        std::fs::write(&child, "[format]\nkeyword-case = 'bad'\n")?;
        assert!(resolver.load(Some(&new_document)).is_err());
        assert!(Arc::ptr_eq(&root_settings, &resolver.load(None)?));

        std::fs::write(&child, "[format]\nkeyword-case = 'upper'\n")?;
        let recovered = resolver.load(Some(&new_document))?;
        assert!(recovered.config.format("select 1;").starts_with("SELECT"));
        assert!(recovered.schema.is_none());
        Ok(())
    }
}
