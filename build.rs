use anyhow::Result;
use vergen::{vergen, Config, SemverKind, ShaKind};

fn main() -> Result<()> {
    // Generate the default 'cargo:' instruction output
    let mut config = Config::default();
    *config.git_mut().sha_kind_mut() = ShaKind::Both;
    *config.git_mut().semver_kind_mut() = SemverKind::Normal;
    *config.git_mut().semver_dirty_mut() = Some("-dirty");
    *config.git_mut().rerun_on_head_change_mut() = true;
    *config.git_mut().sha_mut() = true;
    *config.git_mut().semver_mut() = true;
    vergen(config)?;

    Ok(())
}
