# CONFIG.md

Configure a dev machine from scratch and build all release artifacts locally. This assumes a Fedora/RPM host for the Windows and macOS cross-builds.

For installing prebuilt binaries on target devices, see [SETUP.md](SETUP.md). For what codegraph is and why it exists, see [README.md](README.md).

## What this covers

Starting with no prerequisites installed, you will end with these files in `./dist/`:

- `codegraph-linux-x64.tar.gz`
- `codegraph-0.2.0-1.x86_64.rpm`
- `codegraph-windows-x64.zip`
- `codegraph-macos-arm64.tar.gz`
- `SHA256SUMS`

All commands below are run from the repository root unless stated otherwise.

## 1. Install Rust

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Add the three build targets:

```sh
rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu aarch64-apple-darwin
```

Confirm:

```sh
rustup target list --installed
```

## 2. Fedora/RPM host packages

```sh
sudo dnf install -y mingw64-gcc mingw64-binutils zip \
  clang llvm lld cmake make libxml2-devel openssl-devel xz-devel bzip2-devel
```

The Windows build uses the GNU target, not MSVC; MSVC linking requires Microsoft's toolchain and cannot be done on Linux.

## 3. Build tooling

```sh
cargo install cargo-generate-rpm
cargo generate-rpm --version
```

The build script also expects `strip`, `zip`, `file`, and `sha256sum`, all provided by the packages above or base system utilities.

## 4. osxcross and the Apple SDK

Apple does not publish a standalone SDK tarball. The SDK must come from Apple Xcode or Command Line Tools and is governed by Apple's license: https://www.apple.com/legal/sla/docs/xcode.pdf

Official downloads require an Apple ID:

- Full Xcode: https://developer.apple.com/download/all/?q=xcode
- Command Line Tools for Xcode: https://developer.apple.com/download/all/?q=Command%20Line%20Tools%20for%20Xcode

Command Line Tools are sufficient and much smaller than full Xcode. Keep only **one** SDK archive in `~/osxcross/tarballs/`; `build.sh` fails when multiple SDKs are present.

```sh
git clone https://github.com/tpoechtrager/osxcross.git ~/osxcross
mkdir -p ~/osxcross/tarballs
```

### Route A — generate the SDK on a Mac

On a Mac with Xcode or Command Line Tools installed:

```sh
git clone https://github.com/tpoechtrager/osxcross.git ~/osxcross-mac
cd ~/osxcross-mac
./tools/gen_sdk_package.sh          # full Xcode
# or
./tools/gen_sdk_package_tools.sh    # Command Line Tools
scp MacOSX*.sdk.tar.* <user>@<linux-host>:~/osxcross/tarballs/
```

### Route B — Xcode `.xip` on Fedora

Download `Xcode_16.xip` or similar from Apple, then:

```sh
cd ~/osxcross
./tools/gen_sdk_package_pbzx.sh ~/Downloads/Xcode_16.xip
mv MacOSX*.sdk.tar.* tarballs/
```

Expect a several-GB download and a large extraction footprint.

### Route C — Command Line Tools `.dmg` on Fedora (recommended)

Download `Command_Line_Tools_for_Xcode_16.2.dmg` from Apple, then:

```sh
cd ~/osxcross
./tools/gen_sdk_package_tools_dmg.sh ~/Downloads/Command_Line_Tools_for_Xcode_16.2.dmg
ls -lh ~/osxcross/MacOSX*.tar.*
mv ~/osxcross/MacOSX15.2.sdk.tar.xz ~/osxcross/tarballs/
```

Filename quirk: this osxcross checkout can produce `MacOSX15.2.sdktar.xz` instead of `MacOSX15.2.sdk.tar.xz`. Check and rename if needed:

```sh
ls -1 ~/osxcross/tarballs
mv ~/osxcross/tarballs/MacOSX15.2.sdktar.xz \
   ~/osxcross/tarballs/MacOSX15.2.sdk.tar.xz
```

Alternatively, avoid the quirk with `SDK_COMPRESSOR=gz ./tools/gen_sdk_package.sh`.

### Build the toolchain

```sh
cd ~/osxcross
./build.sh
```

Press Enter for the default options when prompted. Verify:

```sh
ls -1 ~/osxcross/target/bin | grep -E 'oa64|aarch64-apple'
ls -d ~/osxcross/target/MacOSX*.sdk
```

You should see `oa64-clang`, `oa64-clang++`, and an `aarch64-apple-darwin*-ar` wrapper.

The build script resolves that `ar` wrapper with `/bin/ls`. Interactive `ls` may be aliased to `lsd`, which annotates symlinks with `⇒` and corrupts the path.

## 5. Build all artifacts

```sh
./scripts/build-releases.sh
```

`--help` prints prerequisites without building:

```sh
./scripts/build-releases.sh --help
```

The script builds, strips, and packages every target into `./dist/`:

| Artifact | Source target | Notes |
| --- | --- | --- |
| `codegraph-linux-x64.tar.gz` | `x86_64-unknown-linux-gnu` | stripped Linux binary |
| `codegraph-0.2.0-1.x86_64.rpm` | `x86_64-unknown-linux-gnu` | metadata from `crates/cli/Cargo.toml` |
| `codegraph-windows-x64.zip` | `x86_64-pc-windows-gnu` | stripped `codegraph.exe` |
| `codegraph-macos-arm64.tar.gz` | `aarch64-apple-darwin` | macOS 11+ via osxcross |
| `SHA256SUMS` | all above | bare filenames |

The script is idempotent; rerun it after any source or dependency change.

## 6. Verify artifacts

```sh
(cd dist && sha256sum -c SHA256SUMS)
```

Check the RPM:

```sh
rpm -qpl dist/codegraph-0.2.0-1.x86_64.rpm
rpm -qp --info dist/codegraph-0.2.0-1.x86_64.rpm
```

It should install only `/usr/bin/codegraph`, with name `codegraph`, version `0.2.0`, release `1`, architecture `x86_64`.

Check the three binaries:

```sh
file target/x86_64-unknown-linux-gnu/release/codegraph \
     target/x86_64-pc-windows-gnu/release/codegraph.exe \
     target/aarch64-apple-darwin/release/codegraph
```

Expect `ELF 64-bit ... x86-64`, `PE32+ executable ... x86-64`, and `Mach-O 64-bit executable arm64`.

Optional macOS deployment target check:

```sh
tar -xOzf dist/codegraph-macos-arm64.tar.gz codegraph > /tmp/codegraph-mac
~/osxcross/target/bin/aarch64-apple-darwin*-otool -l /tmp/codegraph-mac | grep -A3 LC_BUILD_VERSION
```

Expect `minos 11.0`.

## 7. Regenerating after changes

| Change | Where to edit | Then |
| --- | --- | --- |
| Source code or dependencies | Rust code / `Cargo.lock` | rerun `./scripts/build-releases.sh` |
| Version | `[workspace.package]` in `Cargo.toml` | rerun the script; RPM filename and checksums update automatically |
| RPM metadata | `[package.metadata.generate-rpm]` in `crates/cli/Cargo.toml` | rerun the script, then recheck `rpm -qpl` / `rpm -qp --info` |
| Artifact filenames | `scripts/build-releases.sh` and [SETUP.md](SETUP.md) | rerun the script and update SETUP.md |
| macOS minimum version | `MACOSX_DEPLOYMENT_TARGET` in `scripts/build-releases.sh` | rerun the script |

Always regenerate `SHA256SUMS` after changing any artifact. The build script does this automatically.

## 8. Troubleshooting

- **`lsd` alias corrupts `OSX_AR`** — the script uses `/bin/ls`; do not replace it with plain `ls`.
- **osxcross `build.sh` fails** — ensure `~/osxcross/tarballs/` contains exactly one `MacOSX*.sdk.tar.xz`.
- **Missing `ar` wrapper** — finish `~/osxcross/build.sh`; verify with `ls ~/osxcross/target/bin/aarch64-apple-*-ar`.
- **`cargo generate-rpm` looks for `codegraph-cli/Cargo.toml`** — this workspace needs the crate path: `-p crates/cli`, which the script already uses.
- **Windows link errors** — confirm `x86_64-w64-mingw32-gcc` and `x86_64-w64-mingw32-ar` are installed.
- **Preflight failure** — run `./scripts/build-releases.sh --help` for the full prerequisite list.

## 9. CI note

`.github/workflows/release.yml` publishes release archives on `v*` tags. The local script is the source of truth for `./dist/` builds in this repository. CI changes are out of scope for this document.
