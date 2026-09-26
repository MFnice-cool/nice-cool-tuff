use super::{
    ContentProvider, ModDependencySpec, ProjectInfo, ProviderError, ProviderFileHashes,
    ProviderFileInfo, ProviderSearchQuery, SearchPage, VersionInfo,
};
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::{fs, path::PathBuf};

pub struct LocalJarProvider {
    path: PathBuf,
}

impl LocalJarProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn compute_hashes(&self) -> Result<ProviderFileHashes, ProviderError> {
        let bytes = fs::read(&self.path)?;
        let mut sha1_hasher = Sha1::new();
        let mut sha512_hasher = Sha512::new();
        sha1_hasher.update(&bytes);
        sha512_hasher.update(&bytes);
        let sha1_result = sha1_hasher.finalize();
        let sha512_result = sha512_hasher.finalize();
        Ok(ProviderFileHashes {
            sha1: Some(hex::encode(sha1_result)),
            sha512: Some(hex::encode(sha512_result)),
        })
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "local.jar".to_string())
    }

    fn mod_id(&self) -> String {
        self.file_name().trim_end_matches(".jar").to_string()
    }
}

impl ContentProvider for LocalJarProvider {
    fn search(&self, _query: &ProviderSearchQuery) -> Result<SearchPage, ProviderError> {
        Ok(SearchPage {
            results: vec![self.get_project("local")?],
            total: 1,
        })
    }

    fn get_project(&self, _id: &str) -> Result<ProjectInfo, ProviderError> {
        Ok(ProjectInfo {
            id: self.mod_id(),
            slug: self.mod_id(),
            name: self.file_name(),
            description: String::new(),
            project_type: "mod".to_string(),
            icon_url: None,
            author: None,
            downloads: None,
            follows: None,
            date_modified: None,
            categories: Vec::new(),
            license: None,
            client_side: None,
            server_side: None,
            issues_url: None,
            source_url: None,
            date_created: None,
        })
    }

    fn get_versions(
        &self,
        _id: &str,
        _query: &ProviderSearchQuery,
    ) -> Result<Vec<VersionInfo>, ProviderError> {
        Ok(vec![self.get_version("")?])
    }

    fn get_version(&self, _version_id: &str) -> Result<VersionInfo, ProviderError> {
        Ok(VersionInfo {
            id: self.mod_id(),
            project_id: self.mod_id(),
            version_number: "local".to_string(),
            game_versions: Vec::new(),
            loaders: Vec::new(),
            files: vec![self.get_file("", &self.file_name())?],
            dependencies: Vec::new(),
            name: None,
            changelog: None,
            date_published: None,
            version_type: None,
        })
    }

    fn get_file(
        &self,
        _version_id: &str,
        filename: &str,
    ) -> Result<ProviderFileInfo, ProviderError> {
        let hashes = self.compute_hashes()?;
        Ok(ProviderFileInfo {
            url: self.path.to_string_lossy().to_string(),
            filename: if filename.is_empty() {
                self.file_name()
            } else {
                filename.to_string()
            },
            primary: true,
            hashes,
        })
    }

    fn resolve_dependencies(
        &self,
        _version_id: &str,
    ) -> Result<Vec<ModDependencySpec>, ProviderError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn computes_hashes_for_local_jar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.jar");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"fake jar content").unwrap();

        let provider = LocalJarProvider::new(&path);
        let file_info = provider.get_file("", "example.jar").unwrap();

        assert_eq!(file_info.filename, "example.jar");
        assert!(file_info.hashes.sha1.is_some());
        assert!(file_info.hashes.sha512.is_some());
    }
}
