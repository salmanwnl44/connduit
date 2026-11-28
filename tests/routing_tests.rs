//! Table-driven tests for the routing contract.
//!
//! Every case is one row in [`CASES`]. Adding a behaviour means adding a row,
//! not writing another test function, so the contract stays readable as a table.
//!
//! The matrix covers: exact match, child match, segment mismatch, root prefix,
//! case sensitivity, percent-encoded paths, trailing slash, overlapping
//! prefixes, longest-prefix selection, 404, 405, and the `Allow` list.

use connduit::routing::{Route, RouteMatch, RouteTable};
use http::Method;

/// Which route set a case runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Routes {
    /// `/api` and `/api/users` and `/api/catalog`. No catch-all.
    Api,
    /// `/` as a catch-all, plus `/api`.
    WithRoot,
}

/// The expected outcome for a case.
#[derive(Debug)]
enum Expect {
    /// The named route matched.
    Matched(&'static str),
    /// No route matched, so the caller answers 404.
    NotFound,
    /// The named route matched the path but denied the method, so the caller
    /// answers 405 with this allow list.
    Denied {
        route: &'static str,
        allow: &'static [&'static str],
    },
}

struct Case {
    routes: Routes,
    path: &'static str,
    method: Method,
    expect: Expect,
}

fn route(id: &str, path: &str, methods: Vec<Method>) -> Route {
    Route {
        id: id.to_owned(),
        path: path.to_owned(),
        upstream: "upstream".to_owned(),
        methods,
    }
}

fn table(kind: Routes) -> RouteTable {
    match kind {
        Routes::Api => RouteTable::new(vec![
            route("api", "/api", vec![Method::GET, Method::POST]),
            route("users", "/api/users", vec![Method::GET, Method::POST]),
            route("catalog", "/api/catalog", vec![Method::GET]),
        ]),
        Routes::WithRoot => RouteTable::new(vec![
            route("root", "/", vec![Method::GET]),
            route("api", "/api", vec![Method::GET]),
        ]),
    }
}

/// One row per behaviour in `HTTP_BEHAVIOR.md` §3 and the 404/405 contract.
const CASES: &[Case] = &[
    // ----- exact and child matches -----
    Case {
        routes: Routes::Api,
        path: "/api/users",
        method: Method::GET,
        expect: Expect::Matched("users"),
    },
    Case {
        routes: Routes::Api,
        path: "/api/users/7",
        method: Method::GET,
        expect: Expect::Matched("users"),
    },
    Case {
        routes: Routes::Api,
        path: "/api/users/7/posts",
        method: Method::GET,
        expect: Expect::Matched("users"),
    },
    Case {
        routes: Routes::Api,
        path: "/api/users/",
        method: Method::GET,
        expect: Expect::Matched("users"),
    },
    // ----- segment boundary -----
    Case {
        routes: Routes::Api,
        path: "/api/users-old",
        method: Method::GET,
        // Does not match `users`, but the shorter `/api` prefix still applies.
        expect: Expect::Matched("api"),
    },
    Case {
        routes: Routes::Api,
        path: "/api-users",
        method: Method::GET,
        expect: Expect::NotFound,
    },
    Case {
        routes: Routes::Api,
        path: "/apix",
        method: Method::GET,
        expect: Expect::NotFound,
    },
    // ----- longest prefix wins -----
    Case {
        routes: Routes::Api,
        path: "/api/orders",
        method: Method::GET,
        expect: Expect::Matched("api"),
    },
    Case {
        routes: Routes::Api,
        path: "/api/catalog/42",
        method: Method::GET,
        expect: Expect::Matched("catalog"),
    },
    // ----- case sensitivity -----
    Case {
        routes: Routes::Api,
        path: "/API/users",
        method: Method::GET,
        expect: Expect::NotFound,
    },
    Case {
        routes: Routes::Api,
        path: "/api/Users",
        method: Method::GET,
        // Case-sensitive: does not reach `users`, falls back to `/api`.
        expect: Expect::Matched("api"),
    },
    // ----- no percent-decoding -----
    Case {
        routes: Routes::Api,
        path: "/api/us%65rs",
        method: Method::GET,
        // Would decode to `/api/users`, but no decoding happens, so it falls
        // back to the shorter `/api` prefix.
        expect: Expect::Matched("api"),
    },
    // ----- 404 -----
    Case {
        routes: Routes::Api,
        path: "/other",
        method: Method::GET,
        expect: Expect::NotFound,
    },
    Case {
        routes: Routes::Api,
        path: "/",
        method: Method::GET,
        expect: Expect::NotFound,
    },
    // ----- root catch-all -----
    Case {
        routes: Routes::WithRoot,
        path: "/",
        method: Method::GET,
        expect: Expect::Matched("root"),
    },
    Case {
        routes: Routes::WithRoot,
        path: "/anything/at/all",
        method: Method::GET,
        expect: Expect::Matched("root"),
    },
    Case {
        routes: Routes::WithRoot,
        path: "/api/orders",
        method: Method::GET,
        expect: Expect::Matched("api"),
    },
    // ----- 405 with the exact allow list -----
    Case {
        routes: Routes::Api,
        path: "/api/users",
        method: Method::DELETE,
        expect: Expect::Denied {
            route: "users",
            allow: &["GET", "POST"],
        },
    },
    Case {
        routes: Routes::Api,
        path: "/api/catalog/9",
        method: Method::PUT,
        expect: Expect::Denied {
            route: "catalog",
            allow: &["GET"],
        },
    },
    // ----- HEAD and OPTIONS are not implied -----
    Case {
        routes: Routes::Api,
        path: "/api/users",
        method: Method::HEAD,
        expect: Expect::Denied {
            route: "users",
            allow: &["GET", "POST"],
        },
    },
    Case {
        routes: Routes::Api,
        path: "/api/catalog",
        method: Method::OPTIONS,
        expect: Expect::Denied {
            route: "catalog",
            allow: &["GET"],
        },
    },
    // ----- a denied method on the longest prefix does not fall through -----
    Case {
        routes: Routes::Api,
        path: "/api/users/7",
        method: Method::PATCH,
        expect: Expect::Denied {
            route: "users",
            allow: &["GET", "POST"],
        },
    },
    // ----- root allows GET but denies others -----
    Case {
        routes: Routes::WithRoot,
        path: "/anything",
        method: Method::DELETE,
        expect: Expect::Denied {
            route: "root",
            allow: &["GET"],
        },
    },
];

#[test]
fn routing_table_matches_the_contract() {
    let mut failures = Vec::new();

    for case in CASES {
        let table = table(case.routes);
        let actual = table.resolve(case.path, &case.method);
        let matches = match (&case.expect, &actual) {
            (Expect::Matched(expected), RouteMatch::Matched(route)) => route.id == *expected,
            (Expect::NotFound, RouteMatch::NotFound) => true,
            (
                Expect::Denied {
                    route: expected_route,
                    allow: expected_allow,
                },
                RouteMatch::MethodNotAllowed { route, allow },
            ) => {
                let actual_allow: Vec<&str> = allow.iter().map(Method::as_str).collect();
                route.id == *expected_route && actual_allow == *expected_allow
            }
            _ => false,
        };

        if !matches {
            failures.push(format!(
                "{:?} {} {}: expected {:?}, got {:?}",
                case.routes, case.method, case.path, case.expect, actual
            ));
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_table_covers_every_outcome() {
    // Guards against a future edit that silently drops a whole outcome class.
    let mut matched = 0;
    let mut not_found = 0;
    let mut denied = 0;

    for case in CASES {
        match case.expect {
            Expect::Matched(_) => matched += 1,
            Expect::NotFound => not_found += 1,
            Expect::Denied { .. } => denied += 1,
        }
    }

    assert!(
        matched >= 5,
        "expected several Matched cases, got {matched}"
    );
    assert!(
        not_found >= 3,
        "expected several NotFound cases, got {not_found}"
    );
    assert!(denied >= 5, "expected several Denied cases, got {denied}");
}
