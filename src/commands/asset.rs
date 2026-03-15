use crate::cli::AssetCategory;
use crate::error::{VidgenError, VidgenResult};
use crate::scene;
use colored::*;
use std::path::Path;

/// Add an asset to the project: download a URL or copy a local file into assets/.
pub fn add(source: &str, project_path: &Path, category: &AssetCategory) -> VidgenResult<()> {
    let subdir = match category {
        AssetCategory::Images => "images",
        AssetCategory::Audio => "audio",
        AssetCategory::Fonts => "fonts",
    };
    let target_dir = project_path.join("assets").join(subdir);
    std::fs::create_dir_all(&target_dir)?;

    if scene::is_url(source) {
        // Download URL
        eprintln!(
            "{} Downloading {}...",
            "asset:".cyan().bold(),
            source
        );
        let response = ureq::get(source)
            .call()
            .map_err(|e| VidgenError::Other(format!("Failed to download {source}: {e}")))?;

        // Extract filename from URL
        let filename = source
            .split('?')
            .next()
            .unwrap_or(source)
            .split('/')
            .last()
            .unwrap_or("download.bin");
        let target = target_dir.join(filename);

        let mut reader = response.into_body().into_reader();
        let mut file = std::fs::File::create(&target)?;
        std::io::copy(&mut reader, &mut file)?;

        eprintln!(
            "{} Saved to {}",
            "done:".green().bold(),
            target.display()
        );
        eprintln!(
            "  Reference in templates: @assets/{}/{}",
            subdir, filename
        );
    } else {
        // Copy local file
        let source_path = Path::new(source);
        if !source_path.exists() {
            return Err(VidgenError::Other(format!(
                "Source file not found: {source}"
            )));
        }
        let filename = source_path
            .file_name()
            .ok_or_else(|| VidgenError::Other("Invalid source path".into()))?;
        let target = target_dir.join(filename);
        std::fs::copy(source_path, &target)?;

        eprintln!(
            "{} Copied to {}",
            "done:".green().bold(),
            target.display()
        );
        eprintln!(
            "  Reference in templates: @assets/{}/{}",
            subdir,
            filename.to_string_lossy()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        // Create a source file
        let source_file = dir.path().join("test-image.png");
        std::fs::write(&source_file, b"fake png data").unwrap();

        add(
            source_file.to_str().unwrap(),
            &project,
            &AssetCategory::Images,
        )
        .unwrap();

        assert!(project
            .join("assets/images/test-image.png")
            .exists());
    }

    #[test]
    fn test_add_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = add("/nonexistent/file.png", dir.path(), &AssetCategory::Images);
        assert!(result.is_err());
    }
}
