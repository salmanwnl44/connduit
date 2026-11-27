//! Path-prefix matching for the routing table.
//!
//! The contract is `HTTP_BEHAVIOR.md` §3: a prefix matches itself and its
//! descendants **at a segment boundary**. Matching is case-sensitive and runs on
//! the raw encoded path, so `/api/us%65rs` never matches `/api/users`.
//!
//! This module is pure: no I/O, no allocation, and no method awareness.

/// Returns `true` when `path` is `prefix` itself or a descendant of it.
///
/// A match requires the following character after the prefix to be `/`, so
/// `/api/users` does not match `/api/users-old`.
///
/// The root prefix `/` matches every absolute path. A path that does not begin
/// with `/` never matches, because a request target used for routing is always
/// origin-form.
#[must_use]
pub fn matches_prefix(prefix: &str, path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }

    if prefix == "/" {
        return true;
    }

    if !path.starts_with(prefix) {
        return false;
    }

    // Validation rejects non-root prefixes that end in `/`, so this is a
    // defensive branch: the slash is already the segment boundary.
    if prefix.ends_with('/') {
        return true;
    }

    // Exact match, or the next character must begin a new segment. `get` keeps
    // this in-bounds without indexing or panicking.
    path.len() == prefix.len() || path.as_bytes().get(prefix.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::matches_prefix;

    /// The table from `HTTP_BEHAVIOR.md` §3, verbatim.
    const CONTRACT: &[(&str, &str, bool)] = &[
        ("/api/users", "/api/users", true),
        ("/api/users", "/api/users/7", true),
        ("/api/users", "/api/users-old", false),
        ("/api", "/api/users", true),
        ("/api", "/apix", false),
    ];

    #[test]
    fn matches_the_documented_table() {
        let mut failures = Vec::new();

        for (prefix, path, expected) in CONTRACT {
            let actual = matches_prefix(prefix, path);
            if actual != *expected {
                failures.push(format!(
                    "prefix {prefix:?} path {path:?}: expected {expected}, got {actual}"
                ));
            }
        }

        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn root_prefix_matches_every_absolute_path() {
        for path in ["/", "/api", "/deep/nested/path", "/a?b=c", "/ "] {
            assert!(matches_prefix("/", path), "root must match {path:?}");
        }
    }

    #[test]
    fn root_prefix_rejects_non_absolute_paths() {
        // A routing target is always origin-form, so anything else is a defect.
        assert!(!matches_prefix("/", "api"));
        assert!(!matches_prefix("/api", "api/users"));
        assert!(!matches_prefix("/api", ""));
    }

    #[test]
    fn matches_the_exact_prefix() {
        assert!(matches_prefix("/api/users", "/api/users"));
        assert!(matches_prefix("/", "/"));
    }

    #[test]
    fn matches_descendants_at_a_segment_boundary() {
        assert!(matches_prefix("/api/users", "/api/users/7"));
        assert!(matches_prefix("/api/users", "/api/users/7/posts"));
        assert!(matches_prefix("/api", "/api/"));
    }

    #[test]
    fn rejects_a_partial_segment() {
        assert!(!matches_prefix("/api/users", "/api/users-old"));
        assert!(!matches_prefix("/api", "/apix"));
        assert!(!matches_prefix("/api/v1", "/api/v2"));
    }

    #[test]
    fn rejects_a_shorter_path() {
        assert!(!matches_prefix("/api/users", "/api"));
        assert!(!matches_prefix("/api", "/"));
    }

    #[test]
    fn is_case_sensitive() {
        assert!(!matches_prefix("/API", "/api/users"));
        assert!(!matches_prefix("/api/Users", "/api/users"));
        assert!(matches_prefix("/API", "/API/users"));
    }

    #[test]
    fn does_not_percent_decode() {
        // The raw encoded path is compared, so an encoded segment is a different
        // path even when it decodes to the configured prefix.
        assert!(!matches_prefix("/api/users", "/api/us%65rs"));
        assert!(matches_prefix("/api/us%65rs", "/api/us%65rs/7"));
    }

    #[test]
    fn a_trailing_slash_prefix_treats_the_slash_as_a_boundary() {
        // Validation rejects such prefixes. If that rule ever relaxes, the
        // matcher must still behave coherently rather than silently failing.
        assert!(matches_prefix("/api/", "/api/users"));
        assert!(matches_prefix("/api/", "/api/"));
        assert!(matches_prefix("/api/", "/api/7"));
        assert!(!matches_prefix("/api/", "/apix"));
    }

    #[test]
    fn is_pure_and_does_not_allocate() {
        // Exercised for behaviour; allocation-freedom is structural: the function
        // only borrows and returns a bool.
        for _ in 0..3 {
            assert!(matches_prefix("/api", "/api/users"));
        }
    }
}
