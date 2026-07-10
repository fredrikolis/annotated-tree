// Util: Small pure helpers — exclude-glob compilation and relative-time formatting. NOT concerned with domain logic. | I/O: (values) -> values

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

/// Render a relative path with forward slashes on every platform, so lint output
/// (`path:line: message`) is stable and editor/CI-parseable on Windows too.
pub fn to_unix_path(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Compile `-I` patterns into a matcher. Each pattern may bundle several globs
/// with `|`, tree-style: `-I 'node_modules|dist|*.pyc'`.
pub fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        for part in pattern.split('|') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            builder.add(Glob::new(part).with_context(|| format!("invalid exclude glob `{part}`"))?);
        }
    }
    builder.build().context("building exclude matcher")
}

/// Format a file age as a compact relative string: `5m ago`, `2h ago`, `3d ago`.
pub fn format_relative_time(age_seconds: i64) -> String {
    if age_seconds < 0 {
        return "future".to_string();
    }
    let minutes = age_seconds / 60;
    let hours = age_seconds / 3600;
    let days = age_seconds / 86400;
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;
    if minutes < 60 {
        format!("{minutes}m ago")
    } else if hours < 24 {
        format!("{hours}h ago")
    } else if days < 7 {
        format!("{days}d ago")
    } else if weeks < 5 {
        format!("{weeks}w ago")
    } else if days < 365 {
        // Gate the months/years split on DAYS, not `months < 12`: `months` reaches 12
        // at day 360 but `years` only reaches 1 at day 365, so days 360-364 would
        // otherwise fall through and render "0y ago".
        format!("{months}mo ago")
    } else {
        format!("{years}y ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(format_relative_time(-1), "future");
        assert_eq!(format_relative_time(300), "5m ago");
        assert_eq!(format_relative_time(7200), "2h ago");
        assert_eq!(format_relative_time(3 * 86400), "3d ago");
        // The `w` (weeks) and `mo` (months) buckets sit between days and years.
        assert_eq!(format_relative_time(10 * 86400), "1w ago");
        assert_eq!(format_relative_time(60 * 86400), "2mo ago");
        assert_eq!(format_relative_time(400 * 86400), "1y ago");
    }

    #[test]
    fn months_years_boundary_never_reads_zero_years() {
        // The regression: `months` hits 12 at day 360 but `years` only hits 1 at day
        // 365, so days 360-364 must stay "12mo ago", never "0y ago".
        assert_eq!(format_relative_time(360 * 86400), "12mo ago");
        assert_eq!(format_relative_time(364 * 86400), "12mo ago");
        assert_eq!(format_relative_time(365 * 86400), "1y ago");
    }

    #[test]
    fn to_unix_path_normalizes_separators() {
        // Its whole reason to exist: forward slashes on every platform, even for a
        // multi-component relative path built with the OS separator.
        let rel = Path::new("src").join("render").join("md.rs");
        assert_eq!(to_unix_path(&rel), "src/render/md.rs");
    }

    #[test]
    fn globset_splits_on_pipe() {
        let set = build_globset(&["node_modules|*.pyc".to_string()]).unwrap();
        assert!(set.is_match("node_modules"));
        assert!(set.is_match("x.pyc"));
        assert!(!set.is_match("main.rs"));
    }
}
