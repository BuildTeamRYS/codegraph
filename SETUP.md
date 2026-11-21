# SETUP.md

Install the prebuilt `codegraph` binary on your machine and connect it to an MCP-capable coding agent. No Node, Python, or Rust is needed at runtime.

For building from source or preparing a dev machine from scratch, see [CONFIG.md](CONFIG.md). For what codegraph is and why it exists, see [README.md](README.md).

## What you need

| Machine                                    | Artifact                                                                    |
| ------------------------------------------ | --------------------------------------------------------------------------- |
| Linux x86_64 (Debian/Ubuntu/…)             | `codegraph-linux-x64.tar.gz`                                                |
| Fedora/RHEL/RPM x86_64                     | `codegraph-0.2.0-1.x86_64.rpm`                                              |
| Linux ARM64 (e.g. Pi 4/5)                  | `codegraph-linux-arm64.tar.gz`                                              |
| macOS Intel                                | `codegraph-macos-x64.tar.gz`                                                |
| macOS Apple Silicon (macOS 11+)            | `codegraph-macos-arm64.tar.gz`                                              |
| Windows x86_64                             | `codegraph-windows-x64.zip`                                                 |
| Windows on ARM                             | use `codegraph-windows-x64.zip` — Windows runs x64 binaries under emulation |

Download artifacts from [GitHub Releases](https://github.com/BuildTeamRYS/codegraph/releases).

## Verify your download

Release builds include `SHA256SUMS`. From the folder containing the artifacts:

```sh
sha256sum -c SHA256SUMS
```

Every line must report `OK`. If you downloaded only one artifact, compare its hash against the matching line in `SHA256SUMS`.

Set the version once for the commands below:

```sh
VER=0.2.0
BASE="https://github.com/BuildTeamRYS/codegraph/releases/download/v$VER"
```

## Linux x86_64 (tar.gz)

```sh
curl -fLO "$BASE/codegraph-linux-x64.tar.gz"
tar -xzf codegraph-linux-x64.tar.gz
chmod +x codegraph
install -d "$HOME/.codegraph/bin"
install -m755 codegraph "$HOME/.codegraph/bin/codegraph"
echo 'export PATH="$HOME/.codegraph/bin:$PATH"' >> "$HOME/.bashrc"
```

Open a new shell before continuing.

## Fedora/RHEL (RPM)

```sh
curl -fLO "$BASE/codegraph-$VER-1.x86_64.rpm"
sudo dnf install -y "./codegraph-$VER-1.x86_64.rpm"
```

The RPM installs the binary at `/usr/bin/codegraph`, which is already on `PATH`.

## macOS Apple Silicon

```sh
curl -fLO "$BASE/codegraph-macos-arm64.tar.gz"
tar -xzf codegraph-macos-arm64.tar.gz
install -d "$HOME/.codegraph/bin"
install -m755 codegraph "$HOME/.codegraph/bin/codegraph"
echo 'export PATH="$HOME/.codegraph/bin:$PATH"' >> "$HOME/.zshrc"
```

Open a new shell before continuing.

If you downloaded through a browser, Gatekeeper may block the unsigned binary. Right-click → **Open** once, or remove the quarantine attribute:

```sh
xattr -d com.apple.quarantine "$HOME/.codegraph/bin/codegraph"
```

## Windows x64

Run in PowerShell:

```powershell
$ver = '0.2.0'
$base = "https://github.com/BuildTeamRYS/codegraph/releases/download/v$ver"
Invoke-WebRequest "$base/codegraph-windows-x64.zip" -OutFile codegraph-windows-x64.zip
Expand-Archive codegraph-windows-x64.zip -Force
$dir = Join-Path $env:LOCALAPPDATA 'codegraph\bin'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item .\codegraph.exe "$dir\codegraph.exe"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$dir*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User')
}
```

Open a new terminal before continuing. If SmartScreen says "Windows protected your PC", choose **More info → Run anyway**. This is expected until signed binaries ship.

## Offline installs

Copy the release archive for the target machine into an offline folder. The recipient extracts it and follows the manual install steps for their platform above. No network access is needed after the archive has been downloaded.

Share the matching `SHA256SUMS` entry with the archive so the recipient can verify it.

## Configure your agent

codegraph is harness-agnostic: it exposes a standard MCP server over stdio. Any MCP-capable client can launch it with a command and arguments; no codegraph-specific runtime is required.

From your project folder:

```sh
codegraph init
```

This indexes the repo, detects installed agents, shows exactly what it will change in each agent's MCP config, and asks before applying. Most agents need a restart or reload afterwards, and some ask you to approve the new server. `codegraph init` prints the exact steps for each agent it configures.

### Supported agents

Config formats were re-verified against each agent's official docs on 2026-11-19. Statuses reflect real-install verification; see [COMPAT.md](COMPAT.md).

| Agent                                          | Setup                                                         | Status                                                                        |
| ---------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Claude Code (CLI, desktop app, IDE extensions) | `codegraph init --agent claude-code`                          | verified 2026-11-19 (pending interactive approval on first project-scope run) |
| Cursor                                         | `codegraph init --agent cursor`                               | unverified                                                                    |
| VS Code with GitHub Copilot                    | `codegraph init --agent vscode-copilot`                       | unverified                                                                    |
| Codex CLI                                      | `codegraph init --agent codex-cli`                            | unverified                                                                    |
| Gemini CLI                                     | `codegraph init --agent gemini-cli`                           | unverified                                                                    |
| Google Antigravity (IDE and CLI)               | `codegraph init --agent antigravity` (user scope recommended) | unverified                                                                    |
| OpenCode                                       | `codegraph init --agent opencode`                             | verified 2026-11-19 (`opencode mcp list` → connected)                         |
| Windsurf / Devin Desktop                       | `codegraph init --agent windsurf`                             | unverified                                                                    |
| Claude Desktop                                 | `codegraph init --agent claude-desktop` (user scope only)     | unverified                                                                    |

### Any other MCP client

`codegraph mcp-config --format json` (or `toml`, `yaml`) prints a snippet for your client with the right command path. The shape differs between agents, so prefer that output over copying an example. A typical JSON entry looks like this:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "/absolute/path/to/codegraph",
      "args": ["serve", "/absolute/path/to/repo"]
    }
  }
}
```

If your agent has a shell tool but no MCP support, it can call the same tools from the command line: `codegraph query <tool> <args>`.

## Verify

```sh
codegraph --version
codegraph doctor
```

`doctor` connects to codegraph's own server as an MCP client and reports, per agent, what is wrong: config missing, wrong path, server crashing, tools hidden, or index stale. Then ask your agent:

> Use codegraph to find where login is handled and what calls it.

You should see it call a `codegraph` tool instead of opening many files.

## Troubleshooting

- **`codegraph: command not found`** — the install directory is not on `PATH`, or you have not opened a new shell.
- **Wrong architecture error** — download the artifact matching your CPU. Windows on ARM can use the x64 zip under emulation.
- **macOS quarantined binary** — apply the `xattr -d com.apple.quarantine ...` command above.
- **Windows SmartScreen warning** — choose **More info → Run anyway** until signed binaries ship.
- **Stale index** — run `codegraph index` to refresh it.
- **Agent not seeing tools** — restart the agent, then run `codegraph doctor`.

## Uninstall

1. Delete the copied binary (for example, `~/.codegraph/bin/codegraph` or `%LOCALAPPDATA%\codegraph\bin\codegraph.exe`). For RPM installs, use `sudo dnf remove codegraph`.
2. Remove the install directory from your `PATH` if you added it manually.
3. Undo agent configuration with `codegraph agents remove <id>` for each agent you set up.
