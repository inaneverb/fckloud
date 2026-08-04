use {
    anyhow::Result,
    vergen::{BuildBuilder, Emitter},
    vergen_gitcl::GitclBuilder,
};

fn main() -> Result<()> {
    // https://crates.io/crates/vergen
    // https://crates.io/crates/vergen-gitcl

    let build = BuildBuilder::default()
        .build_date(true)
        .build_timestamp(true)
        .build()?;

    let gitcl = GitclBuilder::default()
        .all()
        .describe(true, true, None)
        .build()?;

    Emitter::default()
        .add_instructions(&gitcl)?
        .add_instructions(&build)?
        .emit()?;

    Ok(())
}
