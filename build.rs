use shadow_rs::{SdResult, ShadowBuilder};

/// Emits the build-time constants (package version, short commit) the binary stamps into its
/// default `User-Agent`, and, when ecCodes is linked, gives its vendored shared libraries a home
/// next to the binary.
///
/// The vendored ecCodes, eckit, and libaec are shared libraries built inside the target directory,
/// which `cargo install` deletes once it finishes. `install_runtime_libs` copies them to
/// `<install-root>/lib/cassiopeia` and points the binary's rpath at `../lib/cassiopeia` relative to
/// itself, so an installed binary keeps resolving them. It reads the `DEP_*_ROOT` paths the `-sys`
/// crates export, which cargo hands only to direct dependents; that is why this crate depends on
/// `eccodes-sys` itself. Fallible so a build without git metadata surfaces the failure rather than
/// panicking.
fn main() -> SdResult<()> {
    ShadowBuilder::builder().build()?;
    #[cfg(any(feature = "grib1", feature = "grib2-full"))]
    bindman_utils::install_runtime_libs();
    Ok(())
}
