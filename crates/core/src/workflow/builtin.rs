//! The templates onehand ships. Read-only: a person who wants one changed
//! duplicates it into a template of their own.

use super::template::Template;

/// The shipped templates' files, in the order they are offered.
const FILES: [&str; 3] = [
    include_str!("builtin/checkout.toml"),
    include_str!("builtin/branch.toml"),
    include_str!("builtin/issue.toml"),
];

/// Every shipped template.
pub fn all() -> Vec<Template> {
    FILES
        .iter()
        // A shipped file that does not parse is a build that was never
        // tested: the tests read every one.
        .filter_map(|text| super::store::parse(text).ok())
        .collect()
}
