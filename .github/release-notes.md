**[⬇ Download the installer (eposctl-setup.exe)](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-setup.exe)**: Start menu entry, optional start with Windows, uninstaller. No administrator rights.
Or the portable app: **[eposctl-gui.exe](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-gui.exe)**, one file, double-click to run.

**[⬇ 下载安装程序（eposctl-setup.exe）](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-setup.exe)**：开始菜单快捷方式、可选开机启动、可卸载，不需要管理员权限。
也可以用免安装版：**[eposctl-gui.exe](https://github.com/hiyufan/eposctl/releases/latest/download/eposctl-gui.exe)**，单个文件，双击运行。

Unofficial, open-source replacement for the discontinued EPOS Gaming Suite on Windows. Tested with the **EPOS H3PRO Hybrid** (USB dongle `1395:0303`).
开源的 EPOS Gaming Suite 替代品，已在 **EPOS H3PRO Hybrid** 上测试。

**Requirements / 要求:** Windows 10/11 x64. The headset's EQ, surround, reverb and noise gate need the EPOS audio driver, which came with the Gaming Suite installer (the Gaming Suite app itself is not needed). EPOS no longer offers it for download; without it, the installer can set up Equalizer APO (open source) for the EQ instead. Battery, sidetone and mic noise reduction work either way.
需要 Windows 10/11 64 位。耳机的均衡器、环绕声、混响、噪声门需要 EPOS 音效驱动（随 Gaming Suite 安装包安装，不需要 Gaming Suite 软件本身）。EPOS 已不再提供下载；没有驱动时，安装程序可以帮你装开源的 Equalizer APO 来实现 EQ。电量、侧音、麦克风降噪不需要驱动。

**Files / 文件**
- `eposctl-setup.exe`: installer / 安装程序
- `eposctl-gui.exe`: the app (tray + window), portable / 图形界面程序（免安装）
- `eposctl.exe`: command line / 命令行工具
- `eposctl-…-windows-x64.zip`: both exes, with the README / 两个程序加说明
- `SHA256SUMS.txt`: checksums / 校验值

Windows may warn "Windows protected your PC" because the files are not code-signed: click **More info → Run anyway**.
如果提示“Windows 已保护你的电脑”（文件未签名），点 **更多信息 → 仍要运行**。

Have a different EPOS model? Run `eposctl report --query` and open a [device report](https://github.com/hiyufan/eposctl/issues/new?template=device-report.yml). 其他型号欢迎提交设备报告。

Not affiliated with or endorsed by EPOS / Demant.
