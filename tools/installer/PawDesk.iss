; PawDesk per-user installer (Windows 10/11 x64).
; Compiled by tools/make-installer.ps1 — do not run ISCC by hand unless DistDir exists.
;
; Default install: %LOCALAPPDATA%\Programs\PawDesk  (no admin)
; User config:     %APPDATA%\PawDesk\
; Logs:            %LOCALAPPDATA%\PawDesk\logs\

#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

#define MyAppName "PawDesk"
#define MyAppPublisher "PawDesk"
#define MyAppExeName "pawdesk.exe"

#ifndef DistDir
  #define DistDir "..\..\dist\PawDesk"
#endif

#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{8F3C2A91-4B17-4E6D-9C4A-7B2E5D1F08A3}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppCopyright=MIT
VersionInfoVersion={#MyAppVersion}
VersionInfoDescription=PawDesk 桌面互动宠物
VersionInfoProductName={#MyAppName}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=no
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=PawDesk-Setup-{#MyAppVersion}
SetupIconFile=pawdesk.ico
WizardSmallImageFile=wizard-small.bmp
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
CloseApplications=yes
RestartApplications=no
UsePreviousAppDir=yes
AllowNoIcons=yes
ShowLanguageDialog=no
InfoBeforeFile=
LicenseFile=
SetupLogging=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"

[CustomMessages]
chinesesimplified.LaunchAfterInstall=立即启动 PawDesk
chinesesimplified.CreateDesktopIcon=创建桌面快捷方式
chinesesimplified.StartupIcon=开机时启动 PawDesk（登录后自动出现桌宠）
chinesesimplified.AdditionalIcons=附加选项
chinesesimplified.DeleteUserData=是否同时删除个人配置和日志？%n%n包含快捷方式列表、提醒设置、宠物位置等。%n选择「否」则下次安装后仍可恢复。

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "{cm:StartupIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#DistDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\PawDesk"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Comment: "桌面互动宠物 — 快捷启动坞与健康提醒"; IconFilename: "{app}\pawdesk.ico"
Name: "{group}\卸载 PawDesk"; Filename: "{uninstallexe}"; IconFilename: "{app}\pawdesk.ico"
Name: "{autodesktop}\PawDesk"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Comment: "桌面互动宠物"; IconFilename: "{app}\pawdesk.ico"; Tasks: desktopicon
Name: "{userstartup}\PawDesk"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\pawdesk.ico"; Tasks: startupicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchAfterInstall}"; Flags: nowait postinstall skipifsilent; WorkingDir: "{app}"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\assets"

[Code]
procedure CloseRunningApp;
var
  ResultCode: Integer;
begin
  Exec('taskkill.exe', '/IM pawdesk.exe /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function InitializeSetup(): Boolean;
begin
  CloseRunningApp;
  Result := True;
end;

function InitializeUninstall(): Boolean;
begin
  CloseRunningApp;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    { Silent uninstall always keeps config/logs. MsgBox under /SUPPRESSMSGBOXES
      would otherwise return IDYES and wipe %APPDATA%\PawDesk. }
    if UninstallSilent then
      Exit;
    if MsgBox(CustomMessage('DeleteUserData'), mbConfirmation, MB_YESNO) = IDYES then
    begin
      DelTree(ExpandConstant('{userappdata}\PawDesk'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\PawDesk'), True, True, True);
    end;
  end;
end;
