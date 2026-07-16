use semver::Version;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_ID: Option<&str> = option_env!("LUXEPIX_BUILD_ID");

pub fn current() -> Version {
    Version::parse(VERSION).expect("Cargo package version must be valid semantic versioning")
}

pub fn display() -> String {
    BUILD_ID
        .filter(|build| !build.trim().is_empty())
        .map(|build| format!("{VERSION}.{}", build.trim()))
        .unwrap_or_else(|| VERSION.to_string())
}

pub fn parse_release_tag(tag: &str) -> Result<Version, String> {
    let value = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
    Version::parse(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_release_tag;
    use semver::Version;

    #[test]
    fn release_tags_accept_a_single_v_prefix() {
        assert_eq!(
            parse_release_tag("v1.2.3").expect("parse release tag"),
            Version::new(1, 2, 3)
        );
        assert!(parse_release_tag("release-1.2.3").is_err());
    }
}
