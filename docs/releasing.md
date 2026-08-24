# npm release process

The npm workflow stages immutable candidates; it never makes a package public
by itself. Staged publishing requires npm 11.15 or newer, an existing package,
and maintainer 2FA. Configure each package's trusted publisher for
`npm stage publish` only.

## Build and test locally

From a clean release commit, run:

```bash
cargo xtask check
cargo xtask npm pack
cargo xtask live
cargo xtask perf --python benchmarks/.venv/bin/python
cargo xtask npm install-candidate
eas-mail-mcp setup
```

`install-candidate` installs the root and matching native tarballs from
`dist/npm` into the normal global npm prefix. Exercise first-run setup,
multi-account setup, client configuration, MCP reads, operational CLI reads,
portable references across CLI processes, and permitted self-writes. Restart
configured clients after changing their MCP configuration.

Each active MCP stdio connection owns one server process. During the manual
check, verify that opening one connection creates one process and that closing
the connection removes it. Repeating at least 24 sessions must not accumulate
server processes; the harness enforces the same lifecycle automatically.

## Stage and inspect exact artifacts

Push the accepted commit to `main`, then run `Stage npm release` with `latest`
for a stable version. The workflow builds, audits, and submits all three
tarballs to npm staging. Nothing is public yet.

List and download each staged package:

```bash
npm stage list eas-mail-mcp
npm stage list eas-mail-mcp-darwin-arm64
npm stage list eas-mail-mcp-darwin-x64
npm stage download <stage-id>
```

Install the downloaded root tarball and the tarball matching the test Mac's
architecture. This tests the exact bytes awaiting approval:

```bash
npm install -g ./eas-mail-mcp-darwin-arm64-*.tgz ./eas-mail-mcp-0.*.tgz
eas-mail-mcp setup
```

If the setup or runtime check fails, reject every staged package and bump the
version before creating another candidate. Do not approve a partial fix under
the same version.

## Publish and registry smoke

After acceptance, approve the two native packages first and the root package
last. Approval requires maintainer 2FA and is the action that makes each package
public.

Immediately verify root-package resolution in a clean npm prefix:

```bash
PREFIX="$(mktemp -d)"
npm install -g --prefix "$PREFIX" eas-mail-mcp@latest
"$PREFIX/bin/eas-mail-mcp" --version --verbose
"$PREFIX/bin/eas-mail-mcp" native-path
```

After the registry smoke succeeds, point `next` at the same stable version.
Create the matching Git tag and source-only GitHub release only after both tags
resolve to the accepted artifacts. Provider-expansion pilots do not replace the
generic profile, security, stdio, or package gates above.
