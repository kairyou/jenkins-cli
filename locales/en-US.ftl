select-project-prompt = Please select a project to deploy
select-project-failed = Failed to select project
get-project-failed = Failed to get project
get-job-parameters-failed = Failed to get job parameters

prompt-input = Please enter {$name}
prompt-select = Please select {$name}
prompt-confirm = Please confirm {$name}
prompt-password = Please enter {$name} (press Enter to use default)
prompt-select-branch = Please select {$name}
manual-input = [*] Manual input
polling-queue-item = Task is in queue, please wait...

cancel-build-prompt = There is a build in progress. Do you want to cancel it?
cancelling-build = Cancelling the build...
current-build-id = Current build ID
build-cancelled = Build cancelled
cancel-build-failed = Failed to cancel build
check-build-status-failed = Failed to check build status
cancel-build-error = Failed to cancel build: {$error}

bye = Bye!

load-config-failed = Failed to load configuration
fill-required-config = Please fill in the required configuration (url, user, token)
jenkins-login-instruction = Log in to Jenkins, click on your avatar in the top right corner to get User ID and generate API Token
select-jenkins = Please select Jenkins service
select-jenkins-failed = Failed to select Jenkins service
get-home-dir-failed = Failed to get home directory
config-file = Configuration file
read-config-file-failed = Failed to read configuration file
parse-config-file-failed = Failed to parse configuration file
write-default-config-failed = Failed to write default configuration file

last-build-params = Last build parameters
use-last-build-params = Do you want to use the last build parameters?
update-history-failed = Failed to update history: {$error}
trigger-build-failed = Failed to trigger build

git-bash-version-low = Detected low version of current terminal, please upgrade Git Bash to the latest version:
git-win-download-link = Download link: https://gitforwindows.org/
alternative-solutions = Alternatively, you can try the following solutions:
use-other-win-terminals = 1. Use other terminals, such as Git CMD, PowerShell, cmd, etc.
use-winpty = 2. Use the winpty command to start the program, for example:
winpty-example = winpty jenkins.exe
unsupported-terminal = Unsupported terminal detected, please check your terminal settings.

new-version-available = A new version of jenkins CLI is available: { $version }
current-version = Current version: { $version }
update-instruction = Run `{ $command }` or install from { $url }
