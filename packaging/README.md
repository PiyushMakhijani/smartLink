# Packaging

Distribution recipes for the `slink` command (the project/package is still
`smartlink`; `slink` is just the binary you type). These are **templates** — the
`REPLACE_WITH_*` hashes must be filled in against a real [GitHub
Release](https://github.com/piyush/smartlink/releases) produced by the
`release.yml` workflow before submitting anywhere.

## Release assets produced by CI

Pushing a `v*` tag builds and attaches:

| Platform | Asset | Layout |
| --- | --- | --- |
| Linux  | `slink-x86_64-unknown-linux-gnu.tar.gz` | `slink` + `LICENSE` + `README.md` |
| macOS (Apple Silicon) | `slink-aarch64-apple-darwin.tar.gz` | `slink` + `LICENSE` + `README.md` |
| Windows | `slink-x86_64-pc-windows-msvc.zip` | single `slink.exe` at the archive root (portable) |

Tag the version in `Cargo.toml` first, then:

```sh
git tag v0.1.0 && git push origin v0.1.0
```

## crates.io

```sh
cargo publish          # package name is `smartlink`, installs the `slink` bin
```

## Homebrew (macOS / Linux)

`homebrew/Formula/slink.rb` builds from the source tarball via `cargo install`.

1. Compute the tarball hash:
   `brew fetch --force smartlink https://github.com/piyush/smartlink/archive/refs/tags/v0.1.0.tar.gz`
   (or `shasum -a 256` on the downloaded archive) and paste it into `sha256`.
2. Test locally: `brew install --build-from-source ./homebrew/Formula/slink.rb`.
3. Publish from a personal tap `homebrew-<tap>` and let users run
   `brew tap piyush/slink && brew install slink`.

## Scoop (Windows)

`home scoop add <your>/scoop-bucket` then `scoop install slink`. Put
`scoop/slink.json` in the bucket and replace `hash` with the SHA-256 of the
Windows zip (or rely on `autoupdate`, which re-hashes automatically). Validate
with `scoop checkup -sf slink.json`.

## Winget (Windows)

`winget/` holds the three required manifests (version, installer,
defaultLocale) for `smartlink.slink`. The `portable` installer type points at
the single-file zip. Finalize with:

```sh
wingetcreate update smartlink.slink --submit
```

`wingetcreate` fills in `InstallerSha256` from the download and opens the
winget-pkgs pull request for you.

## Not yet covered

AUR, Debian and RPM recipes are planned once the project has a stable release;
they wrap the same Linux asset above.
