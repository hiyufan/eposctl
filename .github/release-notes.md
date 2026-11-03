**[⬇ Download eposctl-gui.exe](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-gui.exe)**: one file, no installer. Double-click to run.

**[⬇ 下载 eposctl-gui.exe](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-gui.exe)**：单个文件，免安装，双击运行。

Unofficial, open-source replacement for the discontinued EPOS Gaming Suite on Windows. Tested with the **EPOS H3PRO Hybrid** (USB dongle `1395:0303`).
开源的 EPOS Gaming Suite 替代品，已在 **EPOS H3PRO Hybrid** 上测试。

**Requirements / 要求:** Windows 10/11 x64 and the EPOS audio driver (installed by the Gaming Suite installer; the Gaming Suite app itself is not needed). No administrator rights. 需要 EPOS 音效驱动（随 Gaming Suite 安装包安装），不需要 Gaming Suite 软件本身，不需要管理员权限。

**Files / 文件**
- `eposctl-gui.exe`: the app (tray + window) / 图形界面程序
- `eposctl.exe`: command line / 命令行工具
- `eposctl-…-windows-x64.zip`: both, with the README / 两个程序加说明
- `SHA256SUMS.txt`: checksums / 校验值

Windows may warn "Windows protected your PC" because the exe is not code-signed: click **More info → Run anyway**.
如果提示“Windows 已保护你的电脑”（exe 未签名），点 **更多信息 → 仍要运行**。

Have a different EPOS model? Run `eposctl report --query` and open a [device report](https://github.com/hiyufan/eposctl/issues/new?template=device-report.yml). 其他型号欢迎提交设备报告。

Not affiliated with or endorsed by EPOS / Demant.
