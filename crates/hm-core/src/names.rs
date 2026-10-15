//! Small helpers for program names that work the same on every OS.

/// The last path component, splitting on both `/` and `\`.
pub fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Lowercase basename without a Windows executable extension:
/// `/usr/bin/Python3` → `python3`, `C:\\Tools\\Node.EXE` → `node`.
pub fn exe_stem(path: &str) -> String {
    let base = basename(path).to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("/usr/local/bin/node"), "node");
        assert_eq!(basename(r"C:\Program Files\nodejs\node.exe"), "node.exe");
        assert_eq!(basename("vite"), "vite");
    }

    #[test]
    fn exe_stem_lowercases_and_strips_windows_extensions() {
        assert_eq!(exe_stem("/usr/bin/Python3"), "python3");
        assert_eq!(exe_stem(r"C:\Tools\Node.EXE"), "node");
        assert_eq!(exe_stem("npm.cmd"), "npm");
        assert_eq!(exe_stem("vite.js"), "vite.js");
    }
}
