use shadow_rs::shadow;

shadow!(build);

/// The default `User-Agent` a broker run identifies itself with when neither the manifest nor the
/// command line overrides it.
///
/// Derived at build time from the package version alone, with `-dirty` appended when the build came
/// from a working tree carrying uncommitted changes. Nothing about who built it (no author, email,
/// branch, or commit hash) goes into the identifier; it names the release and its provenance only.
#[must_use]
pub fn default_user_agent() -> String {
    if build::GIT_CLEAN {
        format!("cassiopeia/{}", build::PKG_VERSION)
    } else {
        format!("cassiopeia/{}-dirty", build::PKG_VERSION)
    }
}

#[cfg(test)]
mod tests {
    use crate::build_info::default_user_agent;

    #[test]
    fn the_default_user_agent_names_the_release() {
        let agent = default_user_agent();

        assert!(agent.starts_with("cassiopeia/"));
        assert!(agent.contains(env!("CARGO_PKG_VERSION")));
    }
}
