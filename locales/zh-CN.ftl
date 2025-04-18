select-project-prompt = 请选择要发布的项目
select-project-failed = 选择项目失败
get-projects-failed = 获取项目列表失败
get-job-parameters-failed = 获取任务参数失败
get-project-failed = 获取项目信息失败

prompt-input = 请输入{$name}
prompt-select = 请选择{$name}
prompt-confirm = 请确认{$name}
prompt-password = 请输入{$name} (按回车键使用默认值)
prompt-select-branch = 请选择{$name}
manual-input = [*] 手动输入
polling-queue-item = 正在排队等待处理...

cancel-build-prompt = 当前有正在执行的构建，是否终止构建？
cancelling-build = 正在终止构建...
current-build-id = 当前构建ID
build-cancelled = 构建已终止
cancel-build-failed = 终止构建失败
check-build-status-failed = 无法检查构建状态
cancel-build-error = 取消构建失败: {$error}

bye = Bye!

load-config-failed = 加载配置失败
fill-required-config = 请填写必要的配置信息 (url, user, token)
jenkins-login-instruction = 登录Jenkins,点击右上角头像获取User ID并生成API Token
select-jenkins = 请选择Jenkins服务
select-jenkins-failed = 选择Jenkins服务失败
get-home-dir-failed = 获取主目录失败
config-file = 配置文件
read-config-file-failed = 无法读取配置文件
parse-config-file-failed = 解析配置文件失败
write-default-config-failed = 无法写入默认配置文件

last-build-params = 上次的构建参数
use-last-build-params = 是否直接使用上次的构建参数发布？
use-modified-last-build-params = 参数配置已变更，仍使用上次的构建参数？
params-changed-warning = 构建参数配置已变更
update-history-failed = 更新历史记录失败：{$error}
trigger-build-failed = 触发构建失败

git-bash-version-low = 检测到当前终端版本过低，请升级 Git Bash 到最新版本：
git-win-download-link = 下载链接: https://gitforwindows.org/
alternative-solutions = 或者，您可以尝试以下解决方案：
use-other-win-terminals = 1. 使用其他终端，例如 Git CMD、PowerShell、cmd 等。
use-winpty = 2. 使用 winpty 命令启动程序，例如：
winpty-example = winpty jenkins.exe
unsupported-terminal = 检测到不支持的终端，请检查终端设置。

new-version-available = jenkins CLI 有新版本可用：{ $version }
current-version = 当前版本：{ $version }
update-instruction = 运行 `{ $command }` 或从 { $url } 安装
