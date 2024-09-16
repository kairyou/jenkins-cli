# jenkins-cli

A powerful and efficient Jenkins CLI tool written in Rust. Simplifies deployment of Jenkins jobs with an intuitive command-line experience.

[中文文档](README_zh.md)

## Features

- Rapid deployment of Jenkins jobs
- Intuitive command-line interface
- Support for multiple Jenkins environments
- Project filtering capabilities
- Support for common Jenkins operations (e.g., triggering builds, printing console output)
- Real-time console output during job execution
- High performance, guaranteed by Rust language

## Installation

To install the Jenkins CLI tool, use the following command:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/kairyou/jenkins-cli/main/scripts/install.sh)
```

## Usage

After setting up the configuration file (see [Configuration](#configuration) section), you can simply run:

```bash
jenkins
```

This command will:

1. Prompt you to select an environment (if multiple are configured)
2. Display a list of available projects
3. Select a project and set build parameters
4. Trigger the build and show real-time console output

## Configuration

Create a file named `.jenkins.yaml` in your home directory with the following content:

```yaml
# $HOME/.jenkins.yaml
- name: "SIT"
  url: "https://jenkins-sit.your-company.com"
  user: "your-username"
  token: "your-api-token"
  includes: []
  excludes: []
# - name: "PROD"
#   url: "https://jenkins-prod.your-company.com"
#   user: "your-username"
#   token: "your-api-token"
#   includes: ["frontend", "backend"]
#   excludes: ["test"]
```

### Configuration Options

- `name`: Environment name (e.g., "SIT", "UAT"，"PROD")
- `url`: Jenkins server URL
- `user`: Your Jenkins user ID
- `token`: Your Jenkins API token
- `includes`: List of strings or regex patterns to include projects (optional)
- `excludes`: List of strings or regex patterns to exclude projects (optional)

### Project Filtering

You can use `includes` or `excludes` to filter projects:

- `includes: ["frontend", "backend", "^api-"]` # Include projects containing [frontend, backend, api-]
- `excludes: ["test", "dev", ".*-deprecated$"]` # Exclude projects containing [test, dev, *-deprecated]

Note: Regex patterns are case-sensitive unless specified otherwise (e.g., `(?i)` for case-insensitive matching).

### Username and API Token

Your Jenkins username is typically the same as your login username for the Jenkins web interface.

To generate an API token:

1. Log in to your Jenkins server
2. Click on your name in the top right corner
3. Click on "Configure" in the left sidebar
4. In the API Token section, click "Add new Token"
5. Give your token a name and click "Generate"
6. Copy the generated token and paste it into your `.jenkins.yaml` file

Note: Keep your API token secure. Do not share it or commit it to version control.

## TODOs

- [x] Support multiple Jenkins environments
- [x] Support string and text parameter types
- [x] Support choice parameter type
- [x] Support boolean parameter type
- [x] Support password parameter type
- [x] Auto-detect current directory's git branch
- [x] Remember last selected project and build parameters
- [x] i18n support (fluent)
- [ ] Auto-upgrade feature (self_update)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
