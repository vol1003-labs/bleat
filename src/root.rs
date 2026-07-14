use std::path::{Path, PathBuf};

use crate::error::BleatError;

pub fn find_project_root(start: &Path) -> Result<PathBuf, BleatError> {
    start
        .ancestors()
        .find(|candidate| bleat_dir(candidate).is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            BleatError::Usage(format!(
                "no .bleat directory found from `{}`",
                start.display()
            ))
        })
}

pub fn bleat_dir(root: &Path) -> PathBuf {
    root.join(".bleat")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn finds_the_nearest_bleat_directory_in_ancestors() {
        let sandbox = tempdir().expect("sandbox should be created");
        let project = sandbox.path().join("project");
        let nested = project.join("src/deep");
        fs::create_dir_all(sandbox.path().join(".bleat"))
            .expect("outer bleat dir should be created");
        fs::create_dir_all(project.join(".bleat")).expect("project bleat dir should be created");
        fs::create_dir_all(&nested).expect("nested dir should be created");

        let root = find_project_root(&nested).expect("project root should be found");

        assert_eq!(root, project);
        assert_eq!(bleat_dir(&root), root.join(".bleat"));
    }

    #[test]
    fn reports_a_usage_error_when_no_bleat_directory_exists() {
        let sandbox = tempdir().expect("sandbox should be created");
        let nested = sandbox.path().join("src/deep");
        fs::create_dir_all(&nested).expect("nested dir should be created");

        let error = find_project_root(&nested).expect_err("lookup should fail");

        assert!(matches!(error, BleatError::Usage(_)));
        assert!(error.to_string().contains("no .bleat directory"));
    }
}
