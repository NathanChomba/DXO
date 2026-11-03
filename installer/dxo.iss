; DXO installer (Inno Setup 6.3+). Built by scripts\package.ps1, which first
; publishes the app to dist\DXO. To build by hand:  ISCC installer\dxo.iss
;
; The drivers (ViGEmBus, HidHide) are not bundled: DXO's first-run setup
; checks for them and links to their downloads.

#ifndef MyVersion
  #define MyVersion "0.2.0"
#endif

[Setup]
AppId={{8F3B2C1A-5D4E-4F6A-9B7C-2E1D0A9F8C34}
AppName=DXO
AppVersion={#MyVersion}
AppVerName=DXO {#MyVersion}
AppPublisher=DXO
DefaultDirName={autopf}\DXO
DefaultGroupName=DXO
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=DXO-Setup-{#MyVersion}
SetupIconFile=..\ui\DXO.UI\Assets\dxo.ico
UninstallDisplayIcon={app}\DXO.UI.exe
UninstallDisplayName=DXO
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
; The running app holds this; setup asks to close DXO before replacing files.
AppMutex=Local\DXO.UI.SingleInstance
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\DXO\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\DXO"; Filename: "{app}\DXO.UI.exe"
Name: "{autodesktop}\DXO"; Filename: "{app}\DXO.UI.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\DXO.UI.exe"; Description: "{cm:LaunchProgram,DXO}"; Flags: nowait postinstall skipifsilent shellexec

[UninstallRun]
; "Start with Windows" is a scheduled task; remove it with the app.
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN DXO /F"; Flags: runhidden; RunOnceId: "RemoveStartupTask"
