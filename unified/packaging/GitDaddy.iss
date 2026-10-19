; Inno Setup script for the GitDaddy Windows installer.
; Built by package.ps1, which passes /DAppVersion=<version from Cargo.toml>.
; Installs per-user by default (no administrator rights needed); the wizard also offers an all-users install.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\GitDaddy.exe"
#endif

#define AppName "GitDaddy"
#define AppExe "GitDaddy.exe"
#define AppUrl "https://github.com/Duyfrom/GitDaddy"

[Setup]
; Keep this AppId fixed forever: upgrades and uninstall rely on it.
AppId={{9CB12CE5-520C-45E5-ACBA-63AE0523926B}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppName}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\..\LICENSE
SetupIconFile=..\assets\GitDaddy.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
WizardSmallImageFile=..\assets\wizard-small-55.bmp,..\assets\wizard-small-110.bmp
; egui/winit windows ignore Restart Manager's polite shutdown, so close GitDaddy forcibly if needed.
CloseApplications=force
RestartApplications=no
Compression=lzma2/max
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename=GitDaddy-{#AppVersion}-windows-x64-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Monitor, pull, and push all of your Git repositories"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; In-app updates run this installer with /SILENT /RELAUNCH=1, then expect GitDaddy to come back.
Filename: "{app}\{#AppExe}"; Flags: nowait runasoriginaluser; Check: RelaunchRequested

[Code]
function RelaunchRequested(): Boolean;
begin
  Result := ExpandConstant('{param:relaunch|0}') = '1';
end;

// In-app updates start Setup and then close GitDaddy. Give it a moment to exit on its own
// (it saves settings on the way out) before Restart Manager force-closes it.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  WindowTitle: String;
  Waited: Integer;
begin
  Result := '';
  if RelaunchRequested() then
  begin
    WindowTitle := '{#AppName} ' + #$2014 + ' Git Repository Update Manager';
    Waited := 0;
    while (FindWindowByWindowName(WindowTitle) <> 0) and (Waited < 15000) do
    begin
      Sleep(250);
      Waited := Waited + 250;
    end;
  end;
end;

// GitDaddy shells out to git.exe; point people at Git for Windows if it is missing.
function GitOnPath(): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C where git.exe >NUL 2>&1', '', SW_HIDE, ewWaitUntilTerminated, ResultCode)
    and (ResultCode = 0);
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and (not WizardSilent) and (not GitOnPath()) then
    MsgBox('GitDaddy needs Git for Windows, which was not found on this PC.' + #13#10 + #13#10 +
      'Download it from https://git-scm.com/download/win, then start GitDaddy.', mbInformation, MB_OK);
end;
