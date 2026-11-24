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

%changelog
* Tue Nov 24 2026 BuildTeamRYS <team@buildteamrys.com> - 0.2.0-1
- Initial RPM package with interactive CLI menu
