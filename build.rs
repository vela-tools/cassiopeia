use shadow_rs::{SdResult, ShadowBuilder};

/// Emits the build-time constants (package version, short commit) the binary stamps into its
/// default `User-Agent`. Fallible so a build without git metadata surfaces the failure rather than
/// panicking.
fn main() -> SdResult<()> {
    ShadowBuilder::builder().build()?;
    Ok(())
}
