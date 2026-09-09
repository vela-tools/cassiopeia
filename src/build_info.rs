use cassiopeia_common::user_agent::UserAgent;
use shadow_rs::shadow;

shadow!(build);

/// The default `User-Agent` a run identifies itself with, when fetching a remote source and when
/// pushing to a broker, and neither the manifest nor the command line overrides it.
///
/// Derived at build time from the package version alone, with `-dirty` appended when the build came
/// from a working tree carrying uncommitted changes. Nothing about who built it (no author, email,
/// branch, or commit hash) goes into the identifier; it names the release and its provenance only.
#[must_use]
pub fn default_user_agent() -> UserAgent {
    if build::GIT_CLEAN {
        UserAgent::from(format!("cassiopeia/{}", build::PKG_VERSION))
    } else {
        UserAgent::from(format!("cassiopeia/{}-dirty", build::PKG_VERSION))
    }
}

#[cfg(test)]
mod tests {
    use crate::build_info::default_user_agent;

    #[test]
    fn the_default_user_agent_names_the_release() {
        let agent = default_user_agent();

        assert!(agent.as_str().starts_with("cassiopeia/"));
        assert!(agent.as_str().contains(env!("CARGO_PKG_VERSION")));
    }
}
