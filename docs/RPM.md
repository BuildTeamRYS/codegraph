# RPM Packaging and Build Guide for CodeGraph

This guide explains how to build a `.rpm` installation package for CodeGraph.

Packaging the tool as an `.rpm` file allows users (or IT administrators) to install it system-wide with a single command. Once installed, the `codegraph` command becomes available from anywhere in the terminal, and users can access the friendly interactive menu without needing to install Rust or any developer tools on their machines.

## Prerequisites

Before you begin, ensure you are on a Linux machine (or a Linux environment like WSL or a Docker container) and have the following installed:

- **Rust & Cargo**: To build the CodeGraph binary.
- **rpm-build**: The standard Linux tool for creating RPM packages.
  - _Ubuntu/Debian_: `sudo apt install rpm`
  - _Fedora/RHEL_: `sudo dnf install rpm-build`

---

## Step 1: Build the Linux Binary

First, we need to compile the optimized, final version of the CodeGraph tool for Linux.

Run this command from the root of the CodeGraph repository:

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

## Step 2: Set Up the Packaging Workspace

We need a clean, organized folder structure for the RPM build tools to work correctly. Run the following commands to create this structure:

```bash
# Create the standard RPM build directories
mkdir -p rpm-build/{BUILD,RPMS,SOURCES,SPECS,BUILDROOT}

# Copy the compiled binary into the SOURCES folder and rename it simply to "codegraph"
cp target/x86_64-unknown-linux-gnu/release/codegraph rpm-build/SOURCES/codegraph
```

## Step 3: Build the .rpm File

Now, we instruct the RPM builder to read the .spec file and package everything together.
Run this command from the root of the repository:

```bash
rpmbuild --define "_topdir $(pwd)/rpm-build" -bb rpm-build/SPECS/codegraph.spec
```

If successful, the command will finish quietly or with a brief success message. Your ready-to-share installation file will be located here:
`rpm-build/RPMS/x86_64/codegraph-0.2.0-1.x86_64.rpm`

(The exact filename will change based on the Version and Release numbers in your .spec file).

## Step 4: Test the Installation

Before distributing the package, it is best practice to test it to ensure it installs correctly and the command works globally.

1. Install the package (requires administrator privileges):

```bash
sudo rpm -U ./codegraph-0.2.0-1.x86_64.rpm
```

2. Verify the installation by checking the version from any directory:

```bash
codegraph --version
```

3. Test the interactive menu by simply typing:

```bash
codegraph
```

You should see the friendly, step-by-step menu appear, allowing navigation without memorizing commands.

4. Uninstall (for cleanup/testing):

```bash
sudo rpm -e codegraph
```

## Troubleshooting

- Error: rpmbuild: `command not found`
  You need to install the RPM build tools. See the Prerequisites section above for your specific Linux distribution.

- Error: `Failed to execute command` during menu testing
  Ensure the binary was copied correctly into the `SOURCES` folder and that the `install -m 755` line is present in the `.spec` file. The file must have executable permissions.

- Building for different architectures
  This guide targets `x86_64` (standard 64-bit Intel/AMD). If you need to package for ARM64 (like Apple Silicon or Raspberry Pi), change the cargo build target to `aarch64-unknown-linux-gnu` and adjust the `rpmbuild` output path accordingly.
