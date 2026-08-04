# fckloud

A Kubernetes controller that gives a node the ExternalIP a cloud provider would
have given it, if there were a cloud provider. One DaemonSet pod per node asks
several public-IP providers where it lives, weighs their answers by trust
factor, and patches its own node's `status.addresses`.

This repo is a sibling of `yuki-talos`: it exists because that cluster needs it.
The rules of `yuki-talos/CLAUDE.md` apply here wherever they are about how to
work, not about how that cluster is built.

## Repo conventions

- **`README.md` and `TODO.md` are the user's.** Never edit either one, not even
  a typo, not even as a side effect of another task. If a change belongs there,
  describe the wanted modification in the chat, briefly, and ask. Silence is
  not permission.

- **Code carries no explanations.** A comment that restates what the next line
  does is noise, and a comment that narrates a whole block is a sign the block
  wants to be a function. When a comment genuinely earns its place — a
  non-obvious constraint, a trap, a decision that looks wrong until explained —
  make it one or two lines, and make them worth reading.

- Node public IPs are secrets. They live in `yuki-talos/talenv.yaml` (SOPS) and
  nowhere else — not in this repo's docs, comments, manifests, or committed
  command output. Write `<public-ip>` instead.

- Commit messages: a gitmoji, then a title, and nothing else. Past tense, as the
  existing history has it (`✨ Added ...`, `🚑️ Fixed ...`). No body unless the
  user asks for one.

- **One commit, one idea.** A branch that fixes a bug, renames a module and
  bumps a dependency is three commits, not one — a reader who wants only the bug
  fix should be able to take it and nothing else. Split along what the change
  *means*, not along which files it touched.

  The exception, and it is a real one: a split is only worth making when each
  half stands on its own. Two lines that move together, a rename and its call
  sites, a fix and the test that proves it, a dependency bump and the code that
  needed it — those are one idea wearing several diffs, and separating them
  produces commits that do not build. When in doubt, ask whether either commit
  would be reviewable alone. If not, it is one commit.

- No HTML in markdown files, except where markdown genuinely has no equivalent.

- Shell commands in docs must run unchanged in bash, PowerShell, Nushell and
  cmd: no `\` continuations, no pipes into `grep`/`sed`/`awk`, no `$(...)`, no
  `&&`/`||`, no globs. Double quotes where quoting is unavoidable. Fence them as
  plain ``` blocks.

## Build

- **`mise` is the build entry point. There is no Makefile and there will not be
  one.** Tasks live in `mise.toml`: `mise run build`, `lint`, `test`, `check`,
  `ci`, `image`. Any tool a task needs is installed locally through mise, never
  globally and never assumed present.

- The image builds from `deploy/Dockerfile` — musl static binary, rustls (no
  OpenSSL), `alpine` runtime, non-root uid 65532. `.git` is part of the build
  context because vergen stamps the commit into `--version`.

- The clippy set lives in `Cargo.toml` under `[workspace.lints]`, not in the
  `lint` task. `all` and `pedantic` are denied; the handful of allows are the
  lints written for published library crates, and each says so. `mise run lint`
  is just `cargo clippy -- -D warnings`; keep it that way so a plain `cargo
  clippy` gives the same verdict as CI.

- `vergen` and `vergen-gitcl` are pinned with `=` because they share
  `vergen-lib` and cargo cannot express that they must move together. A caret
  bump on one produces two copies of the same trait and a build script that
  will not compile. Bump both or neither.

- On Windows, the default `x86_64-pc-windows-msvc` toolchain cannot link
  anything — not even a build script — without Visual Studio's C++ tools.
  `rustup toolchain install stable-x86_64-pc-windows-gnu` brings its own linker
  and needs no Visual Studio; put its `bin` on `PATH` and `check`, `clippy`,
  `fmt` and `test` all work. It only produces Windows binaries, though — the
  shipped artifact is `linux/arm64` and comes from the container build.

## The cluster

`yuki` — three arm64 Talos control-plane nodes at netcup. Kubeconfig lives at
`yuki-talos/kubeconfig/yuki-tailnet`. Using the cluster for whatever the work
needs is pre-authorized; mutating it beyond what the work needs is not. Clean up
anything spun up along the way.

- **Verify on the cluster before publishing to the registry.** Build the image,
  deploy it to `yuki` directly, watch it do its job, and only then push a tag to
  `ghcr.io/inaneverb/fckloud`.

- **The job has to persist.** Clearing a node's ExternalIP by hand must result
  in the controller putting it back on the next tick. A patch that lands once is
  not evidence; a patch that survives is.

- The kubelet must run with `cloud-provider: external`
  (`yuki-talos/patch/72-machine-kubelet-external-cloud-provider.yaml`) or it
  overwrites `status.addresses` on every sync and nothing this controller does
  will stick.

## Layout

| Crate | Holds |
|---|---|
| `crates/cli` | The binary: clap arguments, subcommands, logging, the tick loop |
| `crates/ndhcp` | Providers, trust factors, consensus, address classification |
| `crates/kubem` | The Node addresses reconciler and its Kubernetes client |

`kubem` knows nothing about providers and `ndhcp` knows nothing about
Kubernetes; the `cli` crate is the only thing that has met both. Keep it that
way.
