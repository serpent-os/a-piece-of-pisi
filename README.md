# a-piece-of-pisi

Crazy time: We're building a tool to convert a selection of `.eopkg` packages into `.stone` packages to be consumed
by `moss-rs` to vastly accelerate the development of Serpent OS and Solus 5.

Plan:

 - CLI tool takes an `indexURI` argument
 - Group eopkg packages into source-ids
 - Map these back to the `package.yml` in the Solus monorepo
 - Using provided filter, fetch and process the relevant package groups
 - For each "source", emit `stone.yml`:
    - Explode the `install.tar.xz's` into `pkg/import`
    - Generate basic recipe to `install` files
    - Add `stone.yml` path globs and metadata
 - Use boulder to mass-produce a dirty-import-repo
 - Use dirty-import-repo as bootstrap for new Solus 5/Serpent OS with new target repo and clean recipes
 - Provide source recipe conversion tool
 - Profit.

Note: This method bypasses the need to rewrite `boulder` into Rust just yet, allowing us to reuse our existing
solutions to perform the mass conversion and rebootstrap / cleanup, as well as augmenting the bootstrap repo
with `soname`, `pkgconfig` dependencies etc.

## Timeline

This, and `moss-rs`, are effectively priority 1 for Serpent.


## License

`a-piece-of-pisi` is available under the terms of the [MPL-2.0](https://spdx.org/licenses/MPL-2.0.html)

