; Installer for eposctl (Inno Setup 6.1+).
;
;   iscc /DAppVersion=0.3.0 installer\eposctl.iss
;
; Expects target\release\eposctl-gui.exe and eposctl.exe; writes dist\eposctl-setup.exe.
; Per-user install, no administrator rights.
;
; When the EPOS audio driver is missing, it offers to download and run the
; official EPOS Gaming Suite installer, which carries the driver. That MSI is
; mirrored unmodified on this project's release "epos-gaming-suite-1.12.2",
; because EPOS no longer offers it; its licence allows free, unmodified
; redistribution. As an alternative it offers Equalizer APO (GPL) from its
; SourceForge release. Both downloads are checked against pinned SHA-256s.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#define EposVersion "1.12.2"
#define EposFile "EPOS-Gaming-Suite-1.12.2.1185.msi"
#define EposUrl "https://github.com/hiyufan/eposctl/releases/download/epos-gaming-suite-" + EposVersion + "/" + EposFile
#define EposSha256 "b1634892d5a1c008b2d20d259f36a51ec61751955975031ce67e24cc14e2969e"

#define EqApoVersion "1.4.2"
#define EqApoFile "EqualizerAPO-x64-" + EqApoVersion + ".exe"
#define EqApoUrl "https://sourceforge.net/projects/equalizerapo/files/" + EqApoVersion + "/" + EqApoFile + "/download"
#define EqApoSha256 "7403be7427bbe1936a40dded082829b6e217fc4f5990fee5cba501f0ae055afa"

[Setup]
AppId={{188726F5-7065-4FBB-9FD3-AD931666C7FC}
AppName=eposctl
AppVersion={#AppVersion}
AppVerName=eposctl {#AppVersion}
AppPublisher=eposctl contributors
AppPublisherURL=https://github.com/hiyufan/eposctl
AppSupportURL=https://github.com/hiyufan/eposctl/issues
AppUpdatesURL=https://github.com/hiyufan/eposctl/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\eposctl
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=eposctl-setup
SetupIconFile=..\assets\eposctl.ico
UninstallDisplayIcon={app}\eposctl-gui.exe
UninstallDisplayName=eposctl
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=auto
CloseApplications=force
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
; ChineseSimplified.isl: from Inno Setup's repository (tag is-6_7_1, Files\Languages\Unofficial).
Name: "chinesesimplified"; MessagesFile: "compiler:Default.isl,ChineseSimplified.isl"

[CustomMessages]
english.AutoStart=Start eposctl with Windows (in the tray)
english.DriverGroup=EPOS audio driver (needed for EQ, surround, reverb and noise gate):
english.EposTask=Download and install the official EPOS driver (EPOS Gaming Suite {#EposVersion} installer, about 19 MB; asks for administrator rights)
english.EqApoTask=Or: download and install Equalizer APO {#EqApoVersion} (open source, about 12 MB) for the EQ only
english.NoDriverTitle=EPOS audio driver not found
english.NoDriverText=The EPOS audio driver (EPOS APO) is not installed. It comes with the EPOS Gaming Suite, which EPOS no longer offers for download.%n%nWithout it, eposctl still shows the battery and controls sidetone and microphone noise reduction, but the headset's EQ, surround, reverb and noise gate are not available.%n%nOn the next page you can let this setup download the official Gaming Suite installer (signed by EPOS, unmodified, mirrored on eposctl's GitHub) and run it. You accept EPOS's licence in its own setup. Afterwards you can quit the Gaming Suite app; do not uninstall it, since that also removes the driver.%n%nAlternatively, eposctl can send its parametric EQ to Equalizer APO, a free open-source equalizer.
english.Downloading=Downloading...
english.DownloadFailed=Download failed: %1%n%neposctl itself is still installed.
english.RunFailed=%1 did not run (code %2).

chinesesimplified.AutoStart=开机时启动 eposctl（在托盘运行）
chinesesimplified.DriverGroup=EPOS 音效驱动（均衡器、环绕声、混响、噪声门需要它）：
chinesesimplified.EposTask=下载并安装 EPOS 官方驱动（EPOS Gaming Suite {#EposVersion} 原版安装包，约 19 MB，需要管理员权限）
chinesesimplified.EqApoTask=或者：下载并安装 Equalizer APO {#EqApoVersion}（开源，约 12 MB），只提供均衡器
chinesesimplified.NoDriverTitle=未找到 EPOS 音效驱动
chinesesimplified.NoDriverText=这台电脑没有安装 EPOS 音效驱动（EPOS APO）。它随 EPOS Gaming Suite 一起安装，而 EPOS 已经不再提供下载。%n%n没有驱动时，eposctl 仍然可以显示电量、调节侧音和麦克风降噪，但耳机的均衡器、环绕声、混响和噪声门无法使用。%n%n在下一页可以让本安装程序下载 Gaming Suite 官方安装包（EPOS 签名、未经修改，存放在 eposctl 的 GitHub 上）并运行它。EPOS 的许可协议会在它自己的安装界面里确认。装好后可以退出 Gaming Suite 软件，但不要卸载它，否则驱动也会被删除。%n%n也可以改用 Equalizer APO（免费开源的均衡器），eposctl 的参数 EQ 会通过它生效。
chinesesimplified.Downloading=正在下载……
chinesesimplified.DownloadFailed=下载失败：%1%n%neposctl 本身已经正常安装。
chinesesimplified.RunFailed=%1 没有运行（代码 %2）。

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "eposdriver"; Description: "{cm:EposTask}"; GroupDescription: "{cm:DriverGroup}"; Check: NeedsDriver
Name: "eqapo"; Description: "{cm:EqApoTask}"; GroupDescription: "{cm:DriverGroup}"; Check: OfferEqApo; Flags: unchecked

[Files]
Source: "..\target\release\eposctl-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\eposctl.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.zh-CN.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\eposctl"; Filename: "{app}\eposctl-gui.exe"
Name: "{autodesktop}\eposctl"; Filename: "{app}\eposctl-gui.exe"; Tasks: desktopicon

[Registry]
; The same value the app's own "Start with Windows" switch writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "eposctl"; ValueData: """{app}\eposctl-gui.exe"" --minimized"; Tasks: autostart

[Run]
Filename: "{app}\eposctl-gui.exe"; Description: "{cm:LaunchProgram,eposctl}"; Flags: nowait postinstall skipifsilent

[Code]
var
  DownloadPage: TDownloadWizardPage;
  EposDownloaded, EqApoDownloaded: Boolean;

function NeedsDriver: Boolean;
begin
  Result := not FileExists(ExpandConstant('{sys}\EAPO.dll'));
end;

function EqApoInstalled: Boolean;
begin
  Result := RegValueExists(HKLM64, 'SOFTWARE\EqualizerAPO', 'ConfigPath');
end;

{ Offer Equalizer APO only where the EPOS driver is missing and it is not there yet. }
function OfferEqApo: Boolean;
begin
  Result := NeedsDriver and (not EqApoInstalled);
end;

procedure InitializeWizard;
var
  Info: TOutputMsgWizardPage;
begin
  if NeedsDriver then
    Info := CreateOutputMsgPage(wpWelcome, CustomMessage('NoDriverTitle'), '', CustomMessage('NoDriverText'));
  DownloadPage := CreateDownloadPage(SetupMessage(msgWizardPreparing), CustomMessage('Downloading'), nil);
end;

{ Download one file into {tmp}; False (with a message) when it fails. }
function Fetch(Url, FileName, Sha256: String): Boolean;
begin
  Result := False;
  DownloadPage.Clear;
  DownloadPage.Add(Url, FileName, Sha256);
  DownloadPage.Show;
  try
    try
      DownloadPage.Download;
      Result := True;
    except
      { Not fatal: eposctl itself still installs. }
      SuppressibleMsgBox(FmtMessage(CustomMessage('DownloadFailed'), [GetExceptionMessage]), mbError, MB_OK, IDOK);
    end;
  finally
    DownloadPage.Hide;
  end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = wpReady then
  begin
    if WizardIsTaskSelected('eposdriver') then
      EposDownloaded := Fetch('{#EposUrl}', '{#EposFile}', '{#EposSha256}');
    if WizardIsTaskSelected('eqapo') then
      EqApoDownloaded := Fetch('{#EqApoUrl}', '{#EqApoFile}', '{#EqApoSha256}');
  end;
end;

{ ShellExec, not Exec: both setups ask for administrator rights. }
procedure RunSetup(FileName, Params: String);
var
  Code: Integer;
begin
  if not ShellExec('', FileName, Params, '', SW_SHOW, ewWaitUntilTerminated, Code) then
    SuppressibleMsgBox(FmtMessage(CustomMessage('RunFailed'), [FileName, IntToStr(Code)]), mbError, MB_OK, IDOK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if EposDownloaded then
      RunSetup('msiexec.exe', '/i "' + ExpandConstant('{tmp}\{#EposFile}') + '"');
    if EqApoDownloaded then
      RunSetup(ExpandConstant('{tmp}\{#EqApoFile}'), '');
  end;
end;

{ Also remove the autostart value when the app set it itself. Settings in
  %APPDATA%\eposctl are kept. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'eposctl');
end;
