//! Pull requests as `gh pr list --json` prints them: one opened from a
//! branch, with its checks, or a repository's many, without.

use onehand_core::connector::{Check, CheckState, PrState, PullRequest, PullRequests};
use serde::Deserialize;

/// `gh pr list --json url,number,state,isDraft,headRefOid,mergeable,statusCheckRollup`
/// as the first pull request in it, if any.
///
/// A rollup holds two kinds of entry: an Actions check run, which has a
/// status and, once completed, a conclusion; and a commit status, which has a
/// state alone. Only success, neutral and skipped pass — a cancelled or timed
/// out check is no evidence the change works.
pub(super) fn pull_request(json: &str) -> Result<Option<PullRequest>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        context: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        conclusion: Option<String>,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        details_url: Option<String>,
        #[serde(default)]
        target_url: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        url: String,
        number: u64,
        state: String,
        is_draft: bool,
        head_ref_oid: String,
        #[serde(default)]
        mergeable: String,
        #[serde(default)]
        status_check_rollup: Vec<Entry>,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let checks = row
        .status_check_rollup
        .into_iter()
        .map(|e| {
            let state = match (
                e.status.as_deref(),
                e.conclusion.as_deref(),
                e.state.as_deref(),
            ) {
                (Some(status), _, _) if status != "COMPLETED" => CheckState::Pending,
                (Some(_), Some("SUCCESS" | "NEUTRAL" | "SKIPPED"), _) => CheckState::Passed,
                (Some(_), _, _) => CheckState::Failed,
                (None, _, Some("SUCCESS")) => CheckState::Passed,
                (None, _, Some("PENDING" | "EXPECTED") | None) => CheckState::Pending,
                (None, _, Some(_)) => CheckState::Failed,
            };
            Check {
                name: e.name.or(e.context).unwrap_or_default(),
                state,
                link: e.details_url.or(e.target_url).filter(|l| !l.is_empty()),
            }
        })
        .collect();
    Ok(Some(PullRequest {
        url: row.url,
        number: row.number,
        state: state(&row.state),
        draft: row.is_draft,
        head: row.head_ref_oid,
        conflicting: row.mergeable == "CONFLICTING",
        checks,
    }))
}

/// What [`pull_requests`] asks `gh pr list` for: no checks, which are read
/// per branch when a run needs them.
pub(super) const LIST_FIELDS: &str = "url,number,state,isDraft,headRefOid,headRefName,mergeable";

/// `gh pr list --json` with [`LIST_FIELDS`] as a repository's pull requests,
/// at most `limit` asked for: a list that long reached its cap.
pub(super) fn pull_requests(json: &str, limit: usize) -> Result<PullRequests, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        url: String,
        number: u64,
        state: String,
        is_draft: bool,
        head_ref_oid: String,
        head_ref_name: String,
        #[serde(default)]
        mergeable: String,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    let capped = rows.len() >= limit;
    let by_branch = rows
        .into_iter()
        .map(|row| {
            let pr = PullRequest {
                url: row.url,
                number: row.number,
                state: state(&row.state),
                draft: row.is_draft,
                head: row.head_ref_oid,
                conflicting: row.mergeable == "CONFLICTING",
                checks: Vec::new(),
            };
            (row.head_ref_name, pr)
        })
        .collect();
    Ok(PullRequests { by_branch, capped })
}

fn state(said: &str) -> PrState {
    match said {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    }
}
