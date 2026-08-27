use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonUpdateCheck {
    pub id: &'static str,
    pub latest_version: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
enum UpdateSource {
    Pypi { project: &'static str },
    Npm { package: &'static str },
    GithubRelease { repository: &'static str },
}

const MARKITDOWN: UpdateSource = UpdateSource::Pypi {
    project: "markitdown",
};
const HEADROOM: UpdateSource = UpdateSource::Pypi {
    project: "headroom-ai",
};
const SERENA: UpdateSource = UpdateSource::Pypi {
    project: "serena-agent",
};
const CONTEXT7: UpdateSource = UpdateSource::Npm {
    package: "%40upstash%2Fcontext7-mcp",
};
const RTK: UpdateSource = UpdateSource::GithubRelease {
    repository: "rtk-ai/rtk",
};
const CODEBASE_MEMORY: UpdateSource = UpdateSource::GithubRelease {
    repository: "DeusData/codebase-memory-mcp",
};
const PONYTAIL: UpdateSource = UpdateSource::GithubRelease {
    repository: "DietrichGebert/ponytail",
};
const CAVEMAN: UpdateSource = UpdateSource::GithubRelease {
    repository: "JuliusBrussee/caveman",
};

fn normalize_version(raw: &str) -> Option<String> {
    let value = raw.trim().trim_start_matches(['v', 'V']);
    let mut parts = value.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    if major.is_empty()
        || minor.is_empty()
        || !major.chars().all(|c| c.is_ascii_digit())
        || !minor.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(value.to_string())
}

fn version_from_release_url(path: &str) -> Option<String> {
    normalize_version(path.rsplit('/').next()?)
}

fn json_version(value: &Value, pointer: &str) -> Option<String> {
    normalize_version(value.pointer(pointer)?.as_str()?)
}

async fn fetch_latest(client: &reqwest::Client, source: UpdateSource) -> anyhow::Result<String> {
    match source {
        UpdateSource::Pypi { project } => {
            let value: Value = client
                .get(format!("https://pypi.org/pypi/{project}/json"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            json_version(&value, "/info/version")
                .ok_or_else(|| anyhow::anyhow!("PyPI response did not contain a valid version"))
        }
        UpdateSource::Npm { package } => {
            let value: Value = client
                .get(format!("https://registry.npmjs.org/{package}/latest"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            json_version(&value, "/version")
                .ok_or_else(|| anyhow::anyhow!("npm response did not contain a valid version"))
        }
        UpdateSource::GithubRelease { repository } => {
            let response = client
                .head(format!("https://github.com/{repository}/releases/latest"))
                .send()
                .await?
                .error_for_status()?;
            version_from_release_url(response.url().path()).ok_or_else(|| {
                anyhow::anyhow!("GitHub latest-release redirect did not contain a valid version")
            })
        }
    }
}

async fn check_one(
    client: &reqwest::Client,
    id: &'static str,
    source: UpdateSource,
) -> AddonUpdateCheck {
    match fetch_latest(client, source).await {
        Ok(latest_version) => AddonUpdateCheck {
            id,
            latest_version: Some(latest_version),
            error: None,
        },
        Err(error) => AddonUpdateCheck {
            id,
            latest_version: None,
            error: Some(error.to_string()),
        },
    }
}

pub async fn check_all() -> Vec<AddonUpdateCheck> {
    let client = match reqwest::Client::builder()
        .user_agent(concat!(
            "headroom-local-community/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let message = error.to_string();
            return [
                "headroom",
                "rtk",
                "markitdown",
                "serena",
                "codebase-memory",
                "context7",
                "ponytail",
                "caveman",
            ]
            .into_iter()
            .map(|id| AddonUpdateCheck {
                id,
                latest_version: None,
                error: Some(message.clone()),
            })
            .collect();
        }
    };

    let (headroom, rtk, markitdown, serena, codebase_memory, context7, ponytail, caveman) = tokio::join!(
        check_one(&client, "headroom", HEADROOM),
        check_one(&client, "rtk", RTK),
        check_one(&client, "markitdown", MARKITDOWN),
        check_one(&client, "serena", SERENA),
        check_one(&client, "codebase-memory", CODEBASE_MEMORY),
        check_one(&client, "context7", CONTEXT7),
        check_one(&client, "ponytail", PONYTAIL),
        check_one(&client, "caveman", CAVEMAN),
    );
    vec![
        headroom,
        rtk,
        markitdown,
        serena,
        codebase_memory,
        context7,
        ponytail,
        caveman,
    ]
}

#[cfg(test)]
mod tests {
    use super::{json_version, normalize_version, version_from_release_url};
    use serde_json::json;

    #[test]
    fn normalizes_registry_and_github_versions() {
        assert_eq!(normalize_version("v0.46.0").as_deref(), Some("0.46.0"));
        assert_eq!(normalize_version(" 4.0.3 ").as_deref(), Some("4.0.3"));
        assert_eq!(normalize_version("latest"), None);
        assert_eq!(
            version_from_release_url("/rtk-ai/rtk/releases/tag/v0.46.0").as_deref(),
            Some("0.46.0")
        );
    }

    #[test]
    fn extracts_versions_from_registry_responses() {
        assert_eq!(
            json_version(&json!({ "info": { "version": "0.1.7" } }), "/info/version").as_deref(),
            Some("0.1.7")
        );
        assert_eq!(
            json_version(&json!({ "version": "4.0.3" }), "/version").as_deref(),
            Some("4.0.3")
        );
    }
}
