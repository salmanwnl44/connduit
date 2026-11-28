//! Path-prefix matching and the route table.
//!
//! The contract is `HTTP_BEHAVIOR.md` §3: a prefix matches itself and its
//! descendants **at a segment boundary**, the longest matching prefix wins, and
//! matching is case-sensitive on the raw encoded path.
//!
//! This module is pure: no I/O, no allocation per lookup, and no method
//! awareness. Method policy is applied by the caller.

use http::Method;

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

/// One configured route, without any request-specific state.
#[derive(Debug, Clone)]
pub struct Route {
    /// Stable identifier from the configuration.
    pub id: String,
    /// Absolute prefix, matched at a segment boundary.
    pub path: String,
    /// Identifier of the upstream that serves this route.
    pub upstream: String,
    /// Methods this route accepts. Never empty for a validated route.
    pub methods: Vec<Method>,
}

impl Route {
    /// Returns `true` when `method` is explicitly allowed by this route.
    ///
    /// The comparison is exact. `HEAD` and `OPTIONS` are never implied by
    /// another method, which keeps route policy explicit.
    #[must_use]
    pub fn allows(&self, method: &Method) -> bool {
        self.methods.contains(method)
    }
}

/// The outcome of matching one request against the route table.
///
/// The caller maps this to a response: `404` for [`RouteMatch::NotFound`], `405`
/// with the `Allow` header for [`RouteMatch::MethodNotAllowed`].
#[derive(Debug)]
pub enum RouteMatch<'a> {
    /// The path matched and the method is allowed.
    Matched(&'a Route),
    /// No route prefix matched the path.
    NotFound,
    /// The path matched but the method is not allowed.
    ///
    /// `allow` holds exactly the configured methods, for the `Allow` header.
    MethodNotAllowed {
        /// The route whose prefix matched.
        route: &'a Route,
        /// Methods the route accepts, in configuration order.
        allow: &'a [Method],
    },
}

/// Routes ordered so that the longest prefix is considered first.
///
/// The table is built once at startup and is immutable afterwards, so a lookup
/// is a linear scan over a short, pre-sorted list. Duplicate prefixes are
/// impossible here because configuration validation rejects them.
#[derive(Debug, Clone)]
pub struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    /// Builds a table from validated routes.
    ///
    /// Routes are sorted by descending prefix length, so the first match found
    /// by a linear scan is the longest one. The sort is stable, so equal lengths
    /// keep their input order; validation guarantees equal lengths cannot occur.
    #[must_use]
    pub fn new(mut routes: Vec<Route>) -> Self {
        // Descending length, so the first match found is the longest one.
        routes.sort_by_key(|route| std::cmp::Reverse(route.path.len()));
        Self { routes }
    }

    /// Returns the route with the longest prefix matching `path`.
    ///
    /// Returns `None` when no route matches, which the caller maps to `404`.
    #[must_use]
    pub fn route_for(&self, path: &str) -> Option<&Route> {
        self.routes
            .iter()
            .find(|route| matches_prefix(&route.path, path))
    }

    /// Resolves a request to a routing outcome.
    ///
    /// A path match with a disallowed method is [`RouteMatch::MethodNotAllowed`],
    /// never `NotFound`: the path exists, the method does not.
    #[must_use]
    pub fn resolve(&self, path: &str, method: &Method) -> RouteMatch<'_> {
        match self.route_for(path) {
            None => RouteMatch::NotFound,
            Some(route) if route.allows(method) => RouteMatch::Matched(route),
            Some(route) => RouteMatch::MethodNotAllowed {
                route,
                allow: &route.methods,
            },
        }
    }

    /// Returns the number of routes in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Returns `true` when the table holds no routes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::{Route, RouteMatch, RouteTable, matches_prefix};

    /// The table from `HTTP_BEHAVIOR.md` §3, verbatim.
    const CONTRACT: &[(&str, &str, bool)] = &[
        ("/api/users", "/api/users", true),
        ("/api/users", "/api/users/7", true),
        ("/api/users", "/api/users-old", false),
        ("/api", "/api/users", true),
        ("/api", "/apix", false),
    ];

    fn route(id: &str, path: &str) -> Route {
        Route {
            id: id.to_owned(),
            path: path.to_owned(),
            upstream: "upstream".to_owned(),
            methods: vec![Method::GET],
        }
    }

    // ---------- prefix matching ----------

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

    // ---------- route table ----------

    #[test]
    fn selects_the_longest_matching_prefix() {
        let table = RouteTable::new(vec![
            route("root", "/"),
            route("api", "/api"),
            route("users", "/api/users"),
        ]);

        assert_eq!(
            table
                .route_for("/api/users/7")
                .map(|route| route.id.as_str()),
            Some("users")
        );
        assert_eq!(
            table
                .route_for("/api/orders")
                .map(|route| route.id.as_str()),
            Some("api")
        );
        assert_eq!(
            table.route_for("/health").map(|route| route.id.as_str()),
            Some("root")
        );
    }

    #[test]
    fn input_order_does_not_change_the_result() {
        let ascending = RouteTable::new(vec![
            route("root", "/"),
            route("api", "/api"),
            route("users", "/api/users"),
        ]);
        let descending = RouteTable::new(vec![
            route("users", "/api/users"),
            route("api", "/api"),
            route("root", "/"),
        ]);

        for path in ["/api/users/7", "/api/orders", "/health", "/"] {
            assert_eq!(
                ascending.route_for(path).map(|route| route.id.as_str()),
                descending.route_for(path).map(|route| route.id.as_str()),
                "path {path} must resolve identically regardless of input order"
            );
        }
    }

    #[test]
    fn an_exact_match_beats_its_parent() {
        let table = RouteTable::new(vec![route("api", "/api"), route("users", "/api/users")]);

        assert_eq!(
            table.route_for("/api/users").map(|route| route.id.as_str()),
            Some("users")
        );
    }

    #[test]
    fn returns_none_when_no_route_matches() {
        let table = RouteTable::new(vec![route("api", "/api")]);

        assert!(table.route_for("/other").is_none());
        assert!(table.route_for("/apix").is_none());
    }

    #[test]
    fn an_empty_table_matches_nothing() {
        let table = RouteTable::new(Vec::new());

        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert!(table.route_for("/").is_none());
    }

    #[test]
    fn reports_its_size() {
        let table = RouteTable::new(vec![route("api", "/api"), route("root", "/")]);

        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());
    }

    // ---------- method decision ----------

    fn table_with(methods: Vec<Method>) -> RouteTable {
        RouteTable::new(vec![Route {
            id: "api".to_owned(),
            path: "/api".to_owned(),
            upstream: "upstream".to_owned(),
            methods,
        }])
    }

    #[test]
    fn a_matching_path_with_an_allowed_method_is_matched() {
        let table = table_with(vec![Method::GET, Method::POST]);

        match table.resolve("/api/users", &Method::GET) {
            RouteMatch::Matched(route) => assert_eq!(route.id, "api"),
            other => panic!("expected Matched, got {other:?}"),
        }
    }

    #[test]
    fn no_matching_path_is_not_found() {
        let table = table_with(vec![Method::GET]);

        assert!(matches!(
            table.resolve("/other", &Method::GET),
            RouteMatch::NotFound
        ));
    }

    #[test]
    fn a_matching_path_with_a_denied_method_is_method_not_allowed() {
        let table = table_with(vec![Method::GET, Method::POST]);

        match table.resolve("/api/users", &Method::DELETE) {
            RouteMatch::MethodNotAllowed { route, allow } => {
                assert_eq!(route.id, "api");
                assert_eq!(allow, [Method::GET, Method::POST]);
            }
            other => panic!("expected MethodNotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn a_denied_method_is_405_not_404() {
        let table = table_with(vec![Method::GET]);

        // The path exists, so this must not be reported as a missing route.
        assert!(!matches!(
            table.resolve("/api/users", &Method::PUT),
            RouteMatch::NotFound
        ));
    }

    #[test]
    fn the_allow_list_holds_exactly_the_configured_methods() {
        let table = table_with(vec![Method::GET]);

        match table.resolve("/api", &Method::POST) {
            RouteMatch::MethodNotAllowed { allow, .. } => {
                assert_eq!(allow, [Method::GET], "HEAD and OPTIONS must not be implied");
            }
            other => panic!("expected MethodNotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn head_is_not_implied_by_get() {
        let table = table_with(vec![Method::GET]);

        assert!(matches!(
            table.resolve("/api", &Method::HEAD),
            RouteMatch::MethodNotAllowed { .. }
        ));
    }

    #[test]
    fn options_is_not_implied() {
        let table = table_with(vec![Method::GET, Method::POST]);

        assert!(matches!(
            table.resolve("/api", &Method::OPTIONS),
            RouteMatch::MethodNotAllowed { .. }
        ));
    }

    #[test]
    fn method_matching_is_case_sensitive() {
        let table = table_with(vec![Method::GET]);

        // `Method::from_bytes(b"get")` is a legitimate extension method, but it
        // is not `GET`.
        let lowercase = Method::from_bytes(b"get").expect("extension method parses");
        assert!(matches!(
            table.resolve("/api", &lowercase),
            RouteMatch::MethodNotAllowed { .. }
        ));
    }

    #[test]
    fn resolve_uses_the_longest_prefix_before_checking_the_method() {
        let table = RouteTable::new(vec![
            Route {
                id: "api".to_owned(),
                path: "/api".to_owned(),
                upstream: "upstream".to_owned(),
                methods: vec![Method::GET],
            },
            Route {
                id: "users".to_owned(),
                path: "/api/users".to_owned(),
                upstream: "upstream".to_owned(),
                methods: vec![Method::POST],
            },
        ]);

        // `/api` would allow GET, but the longer `/api/users` wins and denies it.
        match table.resolve("/api/users", &Method::GET) {
            RouteMatch::MethodNotAllowed { route, .. } => assert_eq!(route.id, "users"),
            other => panic!("expected MethodNotAllowed for the longer route, got {other:?}"),
        }
    }
}
