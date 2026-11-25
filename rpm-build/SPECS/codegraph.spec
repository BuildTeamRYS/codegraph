Name:           codegraph
Version:        0.2.0
Release:        1%{?dist}
Summary:        Local code knowledge graph for AI coding agents
License:        PolyForm Shield 1.0.0
URL:            https://github.com/BuildTeamRYS/codegraph
Source0:        codegraph

%description
A local code knowledge graph for coding agents. It indexes your repository
on your own machine. This package includes an easy-to-use interactive menu
so users do not need to memorize complex commands.

%install
# Clean up any old build files
rm -rf %{buildroot}

# Create the standard system folder for user commands
mkdir -p %{buildroot}/usr/bin

# Copy the tool into that folder and make it executable
install -m 755 %{SOURCE0} %{buildroot}/usr/bin/codegraph

%files
# Declare that this package owns this specific file path
/usr/bin/codegraph

%post
# 1. Detect the real user who invoked 'sudo', falling back to 'root' if unknown
REAL_USER="${SUDO_USER:-$(logname 2>/dev/null || echo root)}"

if [ "$REAL_USER" != "root" ]; then
    echo ""
    echo "🔧 Setting up CodeGraph globally for user: $REAL_USER..."
    # 2. 'su -' loads the user's environment (HOME, PATH) correctly.
    # 3. '--yes' prevents the installer from hanging and waiting for confirmation.
    # 4. '--no-index' prevents it from accidentally indexing the Downloads folder.
    su - "$REAL_USER" -c "codegraph init --global --yes --no-index"

    echo ""
    echo "✅ Setup complete! CodeGraph is now registered in your MCP clients."
    echo "💡 Next step: cd into your project folder and run 'codegraph index'"
    echo ""
else
    echo "⚠️ Installed as root. Please run 'codegraph init --Global --yes' as your normal user."
fi

%changelog
* Tue Nov 24 2026 BuildTeamRYS <team@buildteamrys.com> - 0.2.0-1
- Added %post script to automate global MCP setup for the installing user
