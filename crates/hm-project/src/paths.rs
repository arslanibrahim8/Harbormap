//! Path comparisons that behave the same for real paths and recorded fixtures.
//! Windows paths compare case-insensitively.

use std::path::Path;

fn normalized(path: &Path, windows: bool) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    if windows {
        text = text.to_lowercase();
    }
    text
}

/// True when `child` is `parent` or lies inside it.
pub fn within(child: &Path, parent: &Path, windows: bool) -> bool {
    let (child, parent) = (normalized(child, windows), normalized(parent, windows));
    child == parent
        || (child.starts_with(&parent)
            && (parent.ends_with('/') || child[parent.len()..].starts_with('/')))
}

/// Directories that must never become a project: the filesystem root, a drive root,
/// the home directory and its ancestors.
pub fn is_boundary(dir: &Path, home: Option<&Path>, windows: bool) -> bool {
    if dir.parent().is_none() || normalized(dir, windows).ends_with(":/") {
        return true;
    }
    match home {
        Some(home) => within(home, dir, windows),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_respects_component_boundaries() {
        assert!(within(
            Path::new("/work/shop/apps"),
            Path::new("/work/shop"),
            false
        ));
        assert!(within(
            Path::new("/work/shop"),
            Path::new("/work/shop/"),
            false
        ));
        assert!(!within(
            Path::new("/work/shopping"),
            Path::new("/work/shop"),
            false
        ));
        assert!(within(Path::new("/anything"), Path::new("/"), false));
    }

    #[test]
    fn windows_paths_ignore_case_and_separators() {
        assert!(within(
            Path::new(r"C:\Work\Shop\apps\"),
            Path::new("c:/work/shop"),
            true
        ));
        assert!(!within(
            Path::new(r"C:\Work\Shop"),
            Path::new("c:/work/shop"),
            false
        ));
    }

    #[test]
    fn home_and_its_ancestors_are_boundaries() {
        let home = Some(Path::new("/home/me"));
        assert!(is_boundary(Path::new("/"), home, false));
        assert!(is_boundary(Path::new("/home"), home, false));
        assert!(is_boundary(Path::new("/home/me"), home, false));
        assert!(!is_boundary(Path::new("/home/me/shop"), home, false));
        assert!(is_boundary(Path::new("C:/"), None, true));
    }
}
