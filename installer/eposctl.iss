; Installer for eposctl (Inno Setup 6.1+ / 7).
;
;   iscc /DAppVersion=0.3.0 installer\eposctl.iss
;
; Expects target\release\eposctl-gui.exe and eposctl.exe; writes dist\eposctl-setup.exe.
; Per-user install, no administrator rights. The EPOS audio driver is not
; included (it is EPOS's own software); when it is missing, the installer can
; download Equalizer APO (GPL) from its official SourceForge release instead,
; checked against a pinned SHA-256, so the EQ still works.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

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
#if FileExists(CompilerPath + "Languages\ChineseSimplified.isl")
Name: "chinesesimplified"; MessagesFile: "compiler:Default.isl,compiler:Languages\ChineseSimplified.isl"
#endif

[CustomMessages]
english.AutoStart=Start eposctl with Windows (in the tray)
english.EqApoTask=Download and install Equalizer APO {#EqApoVersion} (about 12 MB) so the EQ works without the EPOS driver
english.NoDriverTitle=EPOS audio driver not found
english.NoDriverText=The EPOS audio driver (EPOS APO) is not installed. It came with the EPOS Gaming Suite, which EPOS no longer offers for download.%n%nWithout it, eposctl still shows the battery and controls sidetone and microphone noise reduction, but the headset's EQ, surround, reverb and noise gate are not available.%n%nIf you still have a Gaming Suite installer, install it, and eposctl will use the driver it adds.%n%nOtherwise, eposctl can send its parametric EQ to Equalizer APO, a free open-source equalizer. Tick it on the next page to download and install it. Its own setup will ask which playback device to use: choose the EPOS headset.
english.Downloading=Downloading Equalizer APO...
english.DownloadFailed=Equalizer APO could not be downloaded: %1%n%nYou can get it later from https://sourceforge.net/projects/equalizerapo/
english.EqApoRunFailed=The Equalizer APO setup did not run (code %1).

#if FileExists(CompilerPath + "Languages\ChineseSimplified.isl")
chinesesimplified.AutoStart=开机时启动 eposctl（在托盘运行）
chinesesimplified.EqApoTask=下载并安装 Equalizer APO {#EqApoVersion}（约 12 MB），没有 EPOS 驱动也能用 EQ
chinesesimplified.NoDriverTitle=未找到 EPOS 音效驱动
chinesesimplified.NoDriverText=这台电脑没有安装 EPOS 音效驱动（EPOS APO）。它是随 EPOS Gaming Suite 一起安装的，而 EPOS 已经不再提供 Gaming Suite 的下载。%n%n没有驱动时，eposctl 仍然可以显示电量、调节侧音和麦克风降噪，但耳机的均衡器、环绕声、混响和噪声门无法使用。%n%n如果你手上还有 Gaming Suite 安装包，先装上它，eposctl 会自动使用它带的驱动。%n%n否则，eposctl 可以把参数 EQ 交给 Equalizer APO（免费开源的均衡器）。在下一页勾选即可自动下载安装。它的安装程序会询问要用在哪个播放设备上，请选择 EPOS 耳机。
chinesesimplified.Downloading=正在下载 Equalizer APO……
chinesesimplified.DownloadFailed=Equalizer APO 下载失败：%1%n%n之后可以从 https://sourceforge.net/projects/equalizerapo/ 自行下载。
chinesesimplified.EqApoRunFailed=Equalizer APO 安装程序没有运行（代码 %1）。
#endif

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "eqapo"; Description: "{cm:EqApoTask}"; Check: OfferEqApo

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
  EqApoDownloaded: Boolean;

function EposDriverPresent: Boolean;
begin
  Result := FileExists(ExpandConstant('{sys}\EAPO.dll'));
end;

function EqApoInstalled: Boolean;
begin
  Result := RegValueExists(HKLM64, 'SOFTWARE\EqualizerAPO', 'ConfigPath');
end;

{ Offer Equalizer APO only where it is needed and not there yet. }
function OfferEqApo: Boolean;
begin
  Result := (not EposDriverPresent) and (not EqApoInstalled);
end;

procedure InitializeWizard;
var
  Info: TOutputMsgWizardPage;
begin
  if not EposDriverPresent then
    Info := CreateOutputMsgPage(wpWelcome, CustomMessage('NoDriverTitle'), '', CustomMessage('NoDriverText'));
  DownloadPage := CreateDownloadPage(SetupMessage(msgWizardPreparing), CustomMessage('Downloading'), nil);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpReady) and WizardIsTaskSelected('eqapo') then
  begin
    DownloadPage.Clear;
    DownloadPage.Add('{#EqApoUrl}', '{#EqApoFile}', '{#EqApoSha256}');
    DownloadPage.Show;
    try
      try
        DownloadPage.Download;
        EqApoDownloaded := True;
      except
        { Not fatal: eposctl itself still installs. }
        SuppressibleMsgBox(FmtMessage(CustomMessage('DownloadFailed'), [GetExceptionMessage]), mbError, MB_OK, IDOK);
      end;
    finally
      DownloadPage.Hide;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if (CurStep = ssPostInstall) and EqApoDownloaded then
  begin
    { ShellExec, not Exec: its setup asks for administrator rights. }
    if not ShellExec('', ExpandConstant('{tmp}\{#EqApoFile}'), '', '', SW_SHOW, ewWaitUntilTerminated, Code) then
      SuppressibleMsgBox(FmtMessage(CustomMessage('EqApoRunFailed'), [IntToStr(Code)]), mbError, MB_OK, IDOK);
  end;
end;

{ Also remove the autostart value when the app set it itself. Settings in
  %APPDATA%\eposctl are kept. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'eposctl');
end;
