; The Windows installer, built by tools/package.sh with Inno Setup 6:
;
;   ISCC /DAppVersion=<version> /DStageDir=<package folder> /DOutputDir=<dist> tools\windows\washoi.iss
;
; It installs for the current user only (%LOCALAPPDATA%\Programs\Washoi), so
; neither installing nor updating asks for an administrator. The settings
; (%APPDATA%\Washoi) and the game data (%LOCALAPPDATA%\Washoi) are not the
; installer's: an uninstall leaves them, and a reinstall finds them.
;
; The game updates itself by running a newer installer with
;   /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RELAUNCH=1
; which closes a running washoi.exe, replaces it and starts it again.

#ifndef AppVersion
  #error AppVersion is not defined (ISCC /DAppVersion=1.2.3)
#endif
#ifndef StageDir
  #error StageDir is not defined (the folder the zip package holds)
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
; Never change: Windows knows the installed game by it.
AppId={{5B25008F-B926-4EA9-9CB7-299ED4130ACB}
AppName=Washoi
AppVersion={#AppVersion}
AppVerName=Washoi {#AppVersion}
AppPublisher=Higher Standard Games
AppPublisherURL=https://washoi.net
AppSupportURL=https://washoi.net
AppUpdatesURL=https://washoi.net
PrivilegesRequired=lowest
DefaultDirName={autopf}\Washoi
DisableDirPage=auto
DefaultGroupName=Washoi
DisableProgramGroupPage=yes
DisableWelcomePage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=..\..\assets\icon\washoi.ico
UninstallDisplayIcon={app}\washoi.exe
UninstallDisplayName=Washoi
Compression=lzma2/max
SolidCompression=yes
; A running game is closed before its files are replaced.
CloseApplications=force
RestartApplications=no
OutputDir={#OutputDir}
OutputBaseFilename=washoi-{#AppVersion}-windows-x86_64-setup

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Washoi"; Filename: "{app}\washoi.exe"
Name: "{autodesktop}\Washoi"; Filename: "{app}\washoi.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\washoi.exe"; Description: "{cm:LaunchProgram,Washoi}"; Flags: nowait postinstall skipifsilent
; The game's own update: start the new version once it is in place.
Filename: "{app}\washoi.exe"; Flags: nowait skipifnotsilent; Check: Relaunch

[Code]
function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') <> '0';
end;
